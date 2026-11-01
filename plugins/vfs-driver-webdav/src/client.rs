//! WebDAV/HTTP behavior kept inside the plugin, with no host-specific branches.
//! See RFC 4918 sections 9.1, 9.3, 9.6 and 9.9, and RFC 9110 conditional requests.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use percent_encoding::percent_decode_str;
use time::{OffsetDateTime, format_description::well_known::Rfc2822};
use url::Url;

const PROPERTIES: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:getetag/></d:prop></d:propfind>"#;

#[derive(Clone)]
pub(super) struct Client {
    pub endpoint: Url,
    pub max_write_bytes: usize,
    authorization: Option<String>,
    root_segments: Vec<String>,
}
pub(super) struct Metadata { pub entry: p::Entry, etag: Option<String> }
impl Metadata {
    pub fn strong_etag(&self) -> Option<String> {
        self.etag.as_ref().filter(|value| {
            value.len() >= 2 && value.len() <= 4096 && value.starts_with('"') && value.ends_with('"')
                && value.as_bytes()[1..value.len() - 1].iter().all(|byte| *byte >= 0x21 && *byte != b'"' && *byte != 0x7f)
        }).cloned()
    }
}
pub(super) enum WriteCondition { Absent, Match(String) }
struct Reply { url: Url, response: http::Response }
fn optional_text(fields: &p::Object, field: &str) -> Result<Option<String>> {
    fields.get(field).map(|value| {
        let value = value.as_str().ok_or_else(|| invalid(field, "invalid_type"))?;
        if value.len() > 4096 { return Err(invalid(field, "invalid_value")); }
        Ok(value.to_owned())
    }).transpose()
}
fn decoded_segments(url: &Url) -> Result<Vec<String>> {
    let mut segments = vec![];
    let raw = url.path().strip_prefix('/').ok_or_else(|| failure("invalid_data"))?;
    let raw = raw.strip_suffix('/').unwrap_or(raw);
    if raw.is_empty() { return Ok(segments); }
    for segment in raw.split('/') {
        if segment.is_empty() { return Err(failure("invalid_data")); }
        let bytes = segment.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'%' && (index + 2 >= bytes.len() || !bytes[index + 1].is_ascii_hexdigit() || !bytes[index + 2].is_ascii_hexdigit()) {
                return Err(failure("invalid_data"));
            }
        }
        let decoded = percent_decode_str(segment).decode_utf8().map_err(|_| failure("invalid_name_encoding"))?;
        if decoded.contains(['/', '\\', '\0']) || matches!(decoded.as_ref(), "." | "..") { return Err(failure("invalid_data")); }
        segments.push(decoded.into_owned());
    }
    Ok(segments)
}
pub(super) fn modified(value: &str) -> Option<p::Timestamp> {
    OffsetDateTime::parse(value.trim(), &Rfc2822).ok().map(|time| p::Timestamp { seconds: time.unix_timestamp(), nanos: time.nanosecond() })
}
fn header<'a>(response: &'a http::Response, name: &str) -> Option<&'a str> {
    response.headers.iter().find(|header| header.name.eq_ignore_ascii_case(name)).map(|header| header.value.as_str())
}
fn status_error(status: u16, mutation: bool) -> api::Failure {
    failure(match status {
        400 | 422 => "invalid_request",
        401 | 403 => "permission_denied",
        404 | 410 => "not_found",
        405 | 415 | 501 => "unsupported_operation",
        408 if !mutation => "timed_out",
        409 | 412 | 423 | 424 => "conflict",
        413 => "too_large",
        429 => "rate_limited",
        507 => "capacity_exceeded",
        500..=599 if !mutation => "unavailable",
        _ if mutation => "outcome_unknown",
        _ => "backend",
    })
}
impl Client {
    pub fn parse(fields: &p::Object) -> Result<Self> {
        if let Some(field) = fields.keys().find(|field| !["endpoint", "username", "password", "bearer_token", "max_write_bytes"].contains(&field.as_str())) {
            return Err(invalid(field, "unknown_field"));
        }
        let endpoint = optional_text(fields, "endpoint")?.ok_or_else(|| invalid("endpoint", "missing_field"))?;
        let mut endpoint = Url::parse(&endpoint).map_err(|_| invalid("endpoint", "invalid_value"))?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none()
            || !endpoint.username().is_empty() || endpoint.password().is_some() || endpoint.query().is_some() || endpoint.fragment().is_some() {
            return Err(invalid("endpoint", "invalid_value"));
        }
        let root_segments = decoded_segments(&endpoint).map_err(|_| invalid("endpoint", "invalid_value"))?;
        if !endpoint.path().ends_with('/') { endpoint.path_segments_mut().map_err(|_| invalid("endpoint", "invalid_value"))?.push(""); }
        let username = optional_text(fields, "username")?;
        let password = optional_text(fields, "password")?;
        let bearer = optional_text(fields, "bearer_token")?;
        let authorization = match (username, password, bearer) {
            (None, None, None) => None,
            (Some(username), Some(password), None) => {
                if username.is_empty() || username.contains(':') || username.chars().any(char::is_control) {
                    return Err(invalid("username", "invalid_value"));
                }
                if password.chars().any(char::is_control) { return Err(invalid("password", "invalid_value")); }
                Some(format!("Basic {}", STANDARD.encode(format!("{username}:{password}"))))
            }
            (None, None, Some(token)) => {
                if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) { return Err(invalid("bearer_token", "invalid_value")); }
                Some(format!("Bearer {token}"))
            }
            (Some(_), None, None) => return Err(invalid("password", "missing_field")),
            (None, Some(_), None) => return Err(invalid("username", "missing_field")),
            _ => return Err(invalid("bearer_token", "invalid_value")),
        };
        let max_write_bytes = match fields.get("max_write_bytes") {
            None => p::MAX_DATA_BYTES,
            Some(value) => {
                let value = value.as_u64().ok_or_else(|| invalid("max_write_bytes", if value.is_number() { "invalid_value" } else { "invalid_type" }))?;
                if value == 0 || value > p::MAX_DATA_BYTES as u64 { return Err(invalid("max_write_bytes", "invalid_value")); }
                value as usize
            }
        };
        Ok(Self { endpoint, max_write_bytes, authorization, root_segments })
    }
    fn url(&self, relative: &str, collection: bool) -> Result<Url> {
        path(relative)?;
        let mut url = self.endpoint.clone();
        if !relative.is_empty() {
            let mut segments = url.path_segments_mut().map_err(|_| failure("invalid_data"))?;
            segments.pop_if_empty();
            for segment in relative.split('/') { segments.push(segment); }
            if collection { segments.push(""); }
        }
        Ok(url)
    }
    fn relative(&self, url: &Url) -> Result<String> {
        if url.origin() != self.endpoint.origin() || !url.username().is_empty() || url.password().is_some()
            || url.query().is_some() || url.fragment().is_some() { return Err(failure("invalid_data")); }
        let segments = decoded_segments(url)?;
        if !segments.starts_with(&self.root_segments) { return Err(failure("invalid_data")); }
        let relative = segments[self.root_segments.len()..].join("/");
        path(&relative).map_err(|_| failure("invalid_name_encoding"))?;
        Ok(relative)
    }
    fn request(&self, method: &str, mut url: Url, extra: Vec<(&str, String)>, body: Vec<u8>) -> Result<Reply> {
        if body.len() > p::MAX_DATA_BYTES { return Err(failure("too_large")); }
        let mutation = !matches!(method, "GET" | "HEAD" | "PROPFIND" | "OPTIONS");
        let original = self.relative(&url)?;
        for attempt in 0..=3 {
            let mut headers: Vec<_> = extra.iter().map(|(name, value)| http::Header { name: (*name).into(), value: value.clone() }).collect();
            headers.push(http::Header { name: "accept-encoding".into(), value: "identity".into() });
            if let Some(authorization) = &self.authorization { headers.push(http::Header { name: "authorization".into(), value: authorization.clone() }); }
            let request = http::Request { method: method.into(), url: url.as_str().into(), headers, body: body.clone() };
            let response = http::request_http(&request).map_err(|code| {
                // Host limits may fail while reading a response after a mutation
                // was applied; do not misreport these as safe-to-retry failures.
                if mutation && !matches!(code.as_str(), "permission_denied" | "invalid_request") { failure("outcome_unknown") } else { failure(&code) }
            })?;
            if matches!(response.status, 301 | 302 | 307 | 308) && !mutation {
                if attempt == 3 { return Err(failure("invalid_data")); }
                let destination = url.join(header(&response, "location").ok_or_else(|| failure("invalid_data"))?).map_err(|_| failure("invalid_data"))?;
                // Only canonical-slash redirects of the same confined resource
                // are followed. Credentials never follow an origin/base change.
                if self.relative(&destination)? != original { return Err(failure("permission_denied")); }
                url = destination;
                continue;
            }
            return Ok(Reply { url, response });
        }
        Err(failure("invalid_data"))
    }
    fn propfind(&self, relative: &str, depth: u8) -> Result<Vec<Metadata>> {
        let reply = self.request("PROPFIND", self.url(relative, depth == 1)?, vec![("depth", depth.to_string()), ("content-type", "application/xml; charset=utf-8".into())], PROPERTIES.to_vec())?;
        if reply.response.status != 207 { return Err(status_error(reply.response.status, false)); }
        let mut metadata = vec![];
        for response in multistatus::parse(&reply.response.body)? {
            let href = response.href.ok_or_else(|| failure("invalid_data"))?;
            let url = reply.url.join(href.trim()).map_err(|_| failure("invalid_data"))?;
            let path = self.relative(&url)?;
            if let Some(status) = response.status {
                let status = multistatus::status(&status)?;
                if !(200..300).contains(&status) { return Err(status_error(status, false)); }
            }
            let properties = response.properties;
            let collection = properties.collection.ok_or_else(|| failure("invalid_data"))?;
            let kind = if properties.other_type { p::EntryKind::Other } else if collection { p::EntryKind::Dir } else { p::EntryKind::File };
            let size = if kind == p::EntryKind::File {
                properties.length.map(|value| value.trim().parse::<u64>().map_err(|_| failure("invalid_data"))).transpose()?
            } else { None };
            metadata.push(Metadata {
                entry: p::Entry { path, kind, size, modified: properties.modified.as_deref().and_then(modified) },
                etag: properties.etag.map(|value| value.trim().to_owned()),
            });
        }
        Ok(metadata)
    }
    pub fn stat(&self, relative: &str) -> Result<Metadata> {
        path(relative)?;
        let mut entries = self.propfind(relative, 0)?;
        if entries.len() != 1 || entries[0].entry.path != relative { return Err(failure("invalid_data")); }
        entries.pop().ok_or_else(|| failure("invalid_data"))
    }
    pub fn check_root(&self) -> Result<()> {
        if self.stat("")?.entry.kind != p::EntryKind::Dir { return Err(failure("not_dir")); }
        Ok(())
    }
    pub fn list(&self, relative: &str, after: Option<&str>, limit: u32) -> Result<(Vec<p::Entry>, Option<String>)> {
        path(relative)?;
        if limit == 0 || limit > 1000 { return Err(failure("invalid_request")); }
        if let Some(after) = after {
            if after.is_empty() || after.contains('/') { return Err(failure("invalid_path")); }
            path(&if relative.is_empty() { after.into() } else { format!("{relative}/{after}") })?;
        }
        if self.stat(relative)?.entry.kind != p::EntryKind::Dir { return Err(failure("not_dir")); }
        let mut parent = false;
        let mut children = BTreeMap::new();
        let prefix = if relative.is_empty() { String::new() } else { format!("{relative}/") };
        for metadata in self.propfind(relative, 1)? {
            if metadata.entry.path == relative {
                if parent { return Err(failure("invalid_data")); }
                if metadata.entry.kind != p::EntryKind::Dir { return Err(failure("not_dir")); }
                parent = true;
                continue;
            }
            let name = metadata.entry.path.strip_prefix(&prefix).ok_or_else(|| failure("invalid_data"))?.to_owned();
            if name.is_empty() || name.contains('/') || children.insert(name, metadata.entry).is_some() { return Err(failure("invalid_data")); }
        }
        if !parent { return Err(failure("invalid_data")); }
        let mut candidates = children.into_iter().filter(|(name, _)| after.is_none_or(|after| name.as_str() > after)).peekable();
        let mut entries = vec![];
        let mut bytes = 0usize;
        let mut last_name = None;
        while let Some((name, entry)) = candidates.peek() {
            // The requested count is an upper bound. Keep full-path metadata
            // within the common control-frame limit even for deep directories.
            let size = serde_json::to_vec(entry).map_err(|_| failure("invalid_data"))?.len();
            if entries.len() == limit as usize || bytes.saturating_add(size) > p::MAX_CONTROL_BYTES / 2 { break; }
            last_name = Some(name.clone());
            bytes += size + 1;
            let (_, entry) = candidates.next().ok_or_else(|| failure("invalid_data"))?;
            entries.push(entry);
        }
        if entries.is_empty() && candidates.peek().is_some() { return Err(failure("too_large")); }
        let next = if candidates.peek().is_some() { last_name } else { None };
        Ok((entries, next))
    }
    fn parent(&self, relative: &str) -> Result<()> {
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        if self.stat(parent)?.entry.kind != p::EntryKind::Dir { return Err(failure("not_dir")); }
        Ok(())
    }
    pub fn write_condition(&self, relative: &str, replace: bool) -> Result<WriteCondition> {
        path(relative)?;
        if relative.is_empty() { return Err(failure("root_protected")); }
        self.parent(relative)?;
        match self.stat(relative) {
            Ok(_) if !replace => Err(failure("already_exists")),
            Ok(metadata) if metadata.entry.kind == p::EntryKind::Dir => Err(failure("is_dir")),
            Ok(metadata) if metadata.entry.kind == p::EntryKind::File => metadata.strong_etag().map(WriteCondition::Match).ok_or_else(|| failure("unsupported_operation")),
            Ok(_) => Err(failure("unsupported_operation")),
            Err(error) if error.code == "not_found" => Ok(WriteCondition::Absent),
            Err(error) => Err(error),
        }
    }
    pub fn put_file(&self, relative: &str, condition: WriteCondition, bytes: Vec<u8>) -> Result<p::Entry> {
        let size = bytes.len();
        if size > self.max_write_bytes { return Err(failure("too_large")); }
        let absent = matches!(&condition, WriteCondition::Absent);
        let conditional = match condition { WriteCondition::Absent => ("if-none-match", "*".into()), WriteCondition::Match(etag) => ("if-match", etag) };
        let reply = self.request("PUT", self.url(relative, false)?, vec![conditional, ("content-type", "application/octet-stream".into())], bytes)?;
        if reply.response.status == 412 { return Err(failure(if absent { "already_exists" } else { "conflict" })); }
        if !matches!(reply.response.status, 200 | 201 | 204) { return Err(status_error(reply.response.status, true)); }
        // A successful PUT is already a known publication. Avoid a follow-up
        // PROPFIND turning a successful commit into a misleading read failure.
        Ok(p::Entry { path: relative.into(), kind: p::EntryKind::File, size: Some(size as u64), modified: header(&reply.response, "last-modified").and_then(modified) })
    }
    fn make_collection(&self, relative: &str) -> Result<p::Entry> {
        let reply = self.request("MKCOL", self.url(relative, true)?, vec![], vec![])?;
        if reply.response.status == 405 {
            return match self.stat(relative) {
                Ok(_) => Err(failure("already_exists")),
                Err(error) if error.code == "not_found" => Err(failure("unsupported_operation")),
                Err(error) => Err(error),
            };
        }
        if reply.response.status != 201 { return Err(status_error(reply.response.status, true)); }
        Ok(p::Entry { path: relative.into(), kind: p::EntryKind::Dir, size: None, modified: None })
    }
    pub fn create_dir(&self, relative: &str, recursive: bool) -> Result<p::Entry> {
        path(relative)?;
        if relative.is_empty() {
            if !recursive { return Err(failure("already_exists")); }
            let root = self.stat("")?.entry;
            if root.kind != p::EntryKind::Dir { return Err(failure("not_dir")); }
            return Ok(root);
        }
        if !recursive {
            match self.stat(relative) {
                Ok(_) => return Err(failure("already_exists")),
                Err(error) if error.code == "not_found" => {},
                Err(error) => return Err(error),
            }
            self.parent(relative)?;
            return self.make_collection(relative);
        }
        let mut prefix = String::new();
        let mut result = None;
        for name in relative.split('/') {
            if !prefix.is_empty() { prefix.push('/'); }
            prefix.push_str(name);
            let metadata = match self.stat(&prefix) {
                Ok(metadata) => metadata.entry,
                Err(error) if error.code == "not_found" => match self.make_collection(&prefix) {
                    Ok(entry) => entry,
                    Err(error) if error.code == "already_exists" => self.stat(&prefix)?.entry,
                    Err(error) => return Err(error),
                },
                Err(error) => return Err(error),
            };
            if metadata.kind != p::EntryKind::Dir { return Err(failure("not_dir")); }
            result = Some(metadata);
        }
        result.ok_or_else(|| failure("invalid_data"))
    }
    pub fn read_range(&self, relative: &str, offset: u64, length: u64, size: u64, etag: Option<&str>) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        let end = offset.checked_add(length).and_then(|n| n.checked_sub(1)).ok_or_else(|| failure("invalid_request"))?;
        let mut headers = vec![("range", format!("bytes={offset}-{end}"))];
        if let Some(etag) = etag { headers.push(("if-match", etag.into())); }
        let reply = self.request("GET", self.url(relative, false)?, headers, vec![])?;
        let response = reply.response;
        if response.status == 412 || response.status == 416 { return Err(failure("conflict")); }
        if !matches!(response.status, 200 | 206) { return Err(status_error(response.status, false)); }
        if let Some(encoding) = header(&response, "content-encoding") {
            if !encoding.eq_ignore_ascii_case("identity") { return Err(failure("unsupported_operation")); }
        }
        if etag.is_some_and(|etag| header(&response, "etag").is_some_and(|actual| actual != etag)) { return Err(failure("conflict")); }
        match response.status {
            206 => {
                let range = header(&response, "content-range").and_then(|value| value.strip_prefix("bytes ")).ok_or_else(|| failure("invalid_data"))?;
                let (bounds, total) = range.split_once('/').ok_or_else(|| failure("invalid_data"))?;
                let (start, last) = bounds.split_once('-').ok_or_else(|| failure("invalid_data"))?;
                let parse = |value: &str| value.parse::<u64>().map_err(|_| failure("invalid_data"));
                if parse(total)? != size { return Err(failure("conflict")); }
                if parse(start)? != offset || parse(last)? != end || response.body.len() as u64 != length { return Err(failure("invalid_data")); }
                Ok((response.body, None))
            }
            200 => {
                // Servers may ignore Range. Only accept an entire, bounded body
                // whose size matches the discovered representation; never return
                // the beginning of the file for a nonzero requested offset.
                if response.body.len() as u64 != size { return Err(failure("conflict")); }
                let start = usize::try_from(offset).map_err(|_| failure("too_large"))?;
                let end = start.checked_add(length as usize).ok_or_else(|| failure("invalid_data"))?;
                let bytes = response.body.get(start..end).ok_or_else(|| failure("invalid_data"))?.to_vec();
                Ok((bytes, Some(response.body)))
            }
            status => Err(status_error(status, false)),
        }
    }
}
