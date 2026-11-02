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
pub(super) struct Metadata {
    pub entry: p::Entry,
    etag: Option<String>,
}
impl Metadata {
    pub fn strong_etag(&self) -> Option<String> {
        self.etag
            .as_ref()
            .filter(|value| {
                value.len() >= 2
                    && value.len() <= 4096
                    && value.starts_with('"')
                    && value.ends_with('"')
                    && value.as_bytes()[1..value.len() - 1]
                        .iter()
                        .all(|byte| *byte >= 0x21 && *byte != b'"' && *byte != 0x7f)
            })
            .cloned()
    }
}
pub(super) enum WriteCondition {
    Absent,
    Match(String),
}
fn optional_text(fields: &p::JsonObject, field: &str) -> Result<Option<String>> {
    fields
        .get(field)
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| invalid(field, p::ConfigErrorKind::InvalidType))?;
            if value.len() > 4096 {
                return Err(invalid(field, p::ConfigErrorKind::InvalidValue));
            }
            Ok(value.to_owned())
        })
        .transpose()
}
fn decoded_segments(url: &Url) -> Result<Vec<String>> {
    let mut segments = vec![];
    let raw = url
        .path()
        .strip_prefix('/')
        .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
    let raw = raw.strip_suffix('/').unwrap_or(raw);
    if raw.is_empty() {
        return Ok(segments);
    }
    for segment in raw.split('/') {
        if segment.is_empty() {
            return Err(failure(p::FailureCode::InvalidData));
        }
        let bytes = segment.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'%'
                && (index + 2 >= bytes.len()
                    || !bytes[index + 1].is_ascii_hexdigit()
                    || !bytes[index + 2].is_ascii_hexdigit())
            {
                return Err(failure(p::FailureCode::InvalidData));
            }
        }
        let decoded = percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| failure(p::FailureCode::InvalidNameEncoding))?;
        if decoded.contains(['/', '\\', '\0']) || matches!(decoded.as_ref(), "." | "..") {
            return Err(failure(p::FailureCode::InvalidData));
        }
        segments.push(decoded.into_owned());
    }
    Ok(segments)
}
pub(super) fn modified(value: &str) -> Option<p::Timestamp> {
    OffsetDateTime::parse(value.trim(), &Rfc2822)
        .ok()
        .map(|time| p::Timestamp {
            seconds: time.unix_timestamp(),
            nanos: time.nanosecond(),
        })
}
fn header<'a>(response: &'a http::Response<Vec<u8>>, name: &str) -> Option<&'a str> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}
fn status_error(status: u16, mutation: bool) -> api::Failure {
    failure(match status {
        400 | 422 => p::FailureCode::InvalidRequest,
        401 | 403 => p::FailureCode::PermissionDenied,
        404 | 410 => p::FailureCode::NotFound,
        405 | 415 | 501 => p::FailureCode::UnsupportedOperation,
        408 if !mutation => p::FailureCode::TimedOut,
        409 | 412 | 423 | 424 => p::FailureCode::Conflict,
        413 => p::FailureCode::TooLarge,
        429 => p::FailureCode::RateLimited,
        507 => p::FailureCode::CapacityExceeded,
        500..=599 if !mutation => p::FailureCode::Unavailable,
        _ if mutation => p::FailureCode::OutcomeUnknown,
        _ => p::FailureCode::Backend,
    })
}
impl Client {
    pub fn parse(fields: &p::JsonObject) -> Result<Self> {
        if let Some(field) = fields.keys().find(|field| {
            ![
                "endpoint",
                "username",
                "password",
                "bearer_token",
                "max_write_bytes",
            ]
            .contains(&field.as_str())
        }) {
            return Err(invalid(field, p::ConfigErrorKind::UnknownField));
        }
        let endpoint = optional_text(fields, "endpoint")?
            .ok_or_else(|| invalid("endpoint", p::ConfigErrorKind::MissingField))?;
        let mut endpoint = Url::parse(&endpoint)
            .map_err(|_| invalid("endpoint", p::ConfigErrorKind::InvalidValue))?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(invalid("endpoint", p::ConfigErrorKind::InvalidValue));
        }
        let root_segments = decoded_segments(&endpoint)
            .map_err(|_| invalid("endpoint", p::ConfigErrorKind::InvalidValue))?;
        if !endpoint.path().ends_with('/') {
            endpoint
                .path_segments_mut()
                .map_err(|_| invalid("endpoint", p::ConfigErrorKind::InvalidValue))?
                .push("");
        }
        let username = optional_text(fields, "username")?;
        let password = optional_text(fields, "password")?;
        let bearer = optional_text(fields, "bearer_token")?;
        let authorization = match (username, password, bearer) {
            (None, None, None) => None,
            (Some(username), Some(password), None) => {
                if username.is_empty()
                    || username.contains(':')
                    || username.chars().any(char::is_control)
                {
                    return Err(invalid("username", p::ConfigErrorKind::InvalidValue));
                }
                if password.chars().any(char::is_control) {
                    return Err(invalid("password", p::ConfigErrorKind::InvalidValue));
                }
                Some(format!(
                    "Basic {}",
                    STANDARD.encode(format!("{username}:{password}"))
                ))
            }
            (None, None, Some(token)) => {
                if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
                    return Err(invalid("bearer_token", p::ConfigErrorKind::InvalidValue));
                }
                Some(format!("Bearer {token}"))
            }
            (Some(_), None, None) => {
                return Err(invalid("password", p::ConfigErrorKind::MissingField));
            }
            (None, Some(_), None) => {
                return Err(invalid("username", p::ConfigErrorKind::MissingField));
            }
            _ => return Err(invalid("bearer_token", p::ConfigErrorKind::InvalidValue)),
        };
        let max_write_bytes = match fields.get("max_write_bytes") {
            None => p::MAX_DATA_BYTES,
            Some(value) => {
                let value = value.as_u64().ok_or_else(|| {
                    invalid(
                        "max_write_bytes",
                        if value.is_number() {
                            p::ConfigErrorKind::InvalidValue
                        } else {
                            p::ConfigErrorKind::InvalidType
                        },
                    )
                })?;
                if value == 0 || value > p::MAX_DATA_BYTES as u64 {
                    return Err(invalid("max_write_bytes", p::ConfigErrorKind::InvalidValue));
                }
                value as usize
            }
        };
        Ok(Self {
            endpoint,
            max_write_bytes,
            authorization,
            root_segments,
        })
    }
    fn url(&self, relative: &str, collection: bool) -> Result<Url> {
        path(relative)?;
        let mut url = self.endpoint.clone();
        if !relative.is_empty() {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| failure(p::FailureCode::InvalidData))?;
            segments.pop_if_empty();
            for segment in relative.split('/') {
                segments.push(segment);
            }
            if collection {
                segments.push("");
            }
        }
        Ok(url)
    }
    fn relative(&self, url: &Url) -> Result<String> {
        if url.origin() != self.endpoint.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(failure(p::FailureCode::InvalidData));
        }
        let segments = decoded_segments(url)?;
        if !segments.starts_with(&self.root_segments) {
            return Err(failure(p::FailureCode::InvalidData));
        }
        let relative = segments[self.root_segments.len()..].join("/");
        path(&relative).map_err(|_| failure(p::FailureCode::InvalidNameEncoding))?;
        Ok(relative)
    }
    fn request(
        &self,
        method: &str,
        url: Url,
        extra: Vec<(&str, String)>,
        body: Vec<u8>,
    ) -> Result<http::Response<Vec<u8>>> {
        if body.len() > p::MAX_DATA_BYTES {
            return Err(failure(p::FailureCode::TooLarge));
        }
        self.relative(&url)?;
        let mutation = !matches!(method, "GET" | "HEAD" | "PROPFIND" | "OPTIONS");
        let mut request = http::Request::builder()
            .method(method)
            .uri(url.as_str())
            .header("accept-encoding", "identity");
        for (name, value) in extra {
            request = request.header(name, value);
        }
        if let Some(authorization) = &self.authorization {
            request = request.header("authorization", authorization);
        }
        vfs_plugin_guest::wasi_http::send(
            request
                .body(body)
                .map_err(|_| failure(p::FailureCode::InvalidRequest))?,
        )
        .map_err(|error| {
            if mutation
                && !matches!(
                    error.code(),
                    p::FailureCode::PermissionDenied | p::FailureCode::InvalidRequest
                )
            {
                failure(p::FailureCode::OutcomeUnknown)
            } else {
                error
            }
        })
    }
    fn propfind(&self, relative: &str, depth: u8) -> Result<Vec<Metadata>> {
        let base = self.url(relative, depth == 1)?;
        let reply = self.request(
            "PROPFIND",
            base.clone(),
            vec![
                ("depth", depth.to_string()),
                ("content-type", "application/xml; charset=utf-8".into()),
            ],
            PROPERTIES.to_vec(),
        )?;
        if reply.status().as_u16() != 207 {
            return Err(status_error(reply.status().as_u16(), false));
        }
        let mut metadata = vec![];
        for response in multistatus::parse(reply.body())? {
            let href = response
                .href
                .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
            let url = base
                .join(href.trim())
                .map_err(|_| failure(p::FailureCode::InvalidData))?;
            let path = self.relative(&url)?;
            if let Some(status) = response.status {
                let status = multistatus::status(&status)?;
                if !(200..300).contains(&status) {
                    return Err(status_error(status, false));
                }
            }
            let properties = response.properties;
            let collection = properties
                .collection
                .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
            let kind = if properties.other_type {
                p::EntryKind::Other as i32
            } else if collection {
                p::EntryKind::Dir as i32
            } else {
                p::EntryKind::File as i32
            };
            let size = if kind == p::EntryKind::File as i32 {
                properties
                    .length
                    .map(|value| {
                        value
                            .trim()
                            .parse::<u64>()
                            .map_err(|_| failure(p::FailureCode::InvalidData))
                    })
                    .transpose()?
            } else {
                None
            };
            metadata.push(Metadata {
                entry: p::Entry {
                    path,
                    kind,
                    size,
                    modified: properties.modified.as_deref().and_then(modified),
                },
                etag: properties.etag.map(|value| value.trim().to_owned()),
            });
        }
        Ok(metadata)
    }
    pub fn stat(&self, relative: &str) -> Result<Metadata> {
        path(relative)?;
        let mut entries = self.propfind(relative, 0)?;
        if entries.len() != 1 || entries[0].entry.path != relative {
            return Err(failure(p::FailureCode::InvalidData));
        }
        entries
            .pop()
            .ok_or_else(|| failure(p::FailureCode::InvalidData))
    }
    pub fn check_root(&self) -> Result<()> {
        if self.stat("")?.entry.kind != p::EntryKind::Dir as i32 {
            return Err(failure(p::FailureCode::NotDir));
        }
        Ok(())
    }
    pub fn list(
        &self,
        relative: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<p::Entry>, Option<String>)> {
        path(relative)?;
        if limit == 0 || limit > 1000 {
            return Err(failure(p::FailureCode::InvalidRequest));
        }
        if let Some(after) = after {
            if after.is_empty() || after.contains('/') {
                return Err(failure(p::FailureCode::InvalidPath));
            }
            path(&if relative.is_empty() {
                after.into()
            } else {
                format!("{relative}/{after}")
            })?;
        }
        if self.stat(relative)?.entry.kind != p::EntryKind::Dir as i32 {
            return Err(failure(p::FailureCode::NotDir));
        }
        let mut parent = false;
        let mut children = BTreeMap::new();
        let prefix = if relative.is_empty() {
            String::new()
        } else {
            format!("{relative}/")
        };
        for metadata in self.propfind(relative, 1)? {
            if metadata.entry.path == relative {
                if parent {
                    return Err(failure(p::FailureCode::InvalidData));
                }
                if metadata.entry.kind != p::EntryKind::Dir as i32 {
                    return Err(failure(p::FailureCode::NotDir));
                }
                parent = true;
                continue;
            }
            let name = metadata
                .entry
                .path
                .strip_prefix(&prefix)
                .ok_or_else(|| failure(p::FailureCode::InvalidData))?
                .to_owned();
            if name.is_empty()
                || name.contains('/')
                || children.insert(name, metadata.entry).is_some()
            {
                return Err(failure(p::FailureCode::InvalidData));
            }
        }
        if !parent {
            return Err(failure(p::FailureCode::InvalidData));
        }
        let mut candidates = children
            .into_iter()
            .filter(|(name, _)| after.is_none_or(|after| name.as_str() > after))
            .peekable();
        let mut entries = vec![];
        let mut bytes = 0usize;
        let mut last_name = None;
        while let Some((name, entry)) = candidates.peek() {
            // The requested count is an upper bound. Keep full-path metadata
            // within the common control-frame limit even for deep directories.
            let size = p::entry_encoded_len(entry);
            if entries.len() == limit as usize
                || bytes.saturating_add(size) > p::MAX_CONTROL_BYTES / 2
            {
                break;
            }
            last_name = Some(name.clone());
            bytes += size + 1;
            let (_, entry) = candidates
                .next()
                .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
            entries.push(entry);
        }
        if entries.is_empty() && candidates.peek().is_some() {
            return Err(failure(p::FailureCode::TooLarge));
        }
        let next = if candidates.peek().is_some() {
            last_name
        } else {
            None
        };
        Ok((entries, next))
    }
    fn parent(&self, relative: &str) -> Result<()> {
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        if self.stat(parent)?.entry.kind != p::EntryKind::Dir as i32 {
            return Err(failure(p::FailureCode::NotDir));
        }
        Ok(())
    }
    pub fn write_condition(&self, relative: &str, replace: bool) -> Result<WriteCondition> {
        path(relative)?;
        if relative.is_empty() {
            return Err(failure(p::FailureCode::RootProtected));
        }
        self.parent(relative)?;
        match self.stat(relative) {
            Ok(_) if !replace => Err(failure(p::FailureCode::AlreadyExists)),
            Ok(metadata) if metadata.entry.kind == p::EntryKind::Dir as i32 => {
                Err(failure(p::FailureCode::IsDir))
            }
            Ok(metadata) if metadata.entry.kind == p::EntryKind::File as i32 => metadata
                .strong_etag()
                .map(WriteCondition::Match)
                .ok_or_else(|| failure(p::FailureCode::UnsupportedOperation)),
            Ok(_) => Err(failure(p::FailureCode::UnsupportedOperation)),
            Err(error) if error.code == p::FailureCode::NotFound as i32 => {
                Ok(WriteCondition::Absent)
            }
            Err(error) => Err(error),
        }
    }
    pub fn put_file(
        &self,
        relative: &str,
        condition: WriteCondition,
        bytes: Vec<u8>,
    ) -> Result<p::Entry> {
        let size = bytes.len();
        if size > self.max_write_bytes {
            return Err(failure(p::FailureCode::TooLarge));
        }
        let absent = matches!(&condition, WriteCondition::Absent);
        let conditional = match condition {
            WriteCondition::Absent => ("if-none-match", "*".into()),
            WriteCondition::Match(etag) => ("if-match", etag),
        };
        let reply = self.request(
            "PUT",
            self.url(relative, false)?,
            vec![
                conditional,
                ("content-type", "application/octet-stream".into()),
            ],
            bytes,
        )?;
        if reply.status().as_u16() == 412 {
            return Err(failure(if absent {
                p::FailureCode::AlreadyExists
            } else {
                p::FailureCode::Conflict
            }));
        }
        if !matches!(reply.status().as_u16(), 200 | 201 | 204) {
            return Err(status_error(reply.status().as_u16(), true));
        }
        // A successful PUT is already a known publication. Avoid a follow-up
        // PROPFIND turning a successful commit into a misleading read failure.
        Ok(p::Entry {
            path: relative.into(),
            kind: p::EntryKind::File as i32,
            size: Some(size as u64),
            modified: header(&reply, "last-modified").and_then(modified),
        })
    }
    fn make_collection(&self, relative: &str) -> Result<p::Entry> {
        let reply = self.request("MKCOL", self.url(relative, true)?, vec![], vec![])?;
        if reply.status().as_u16() == 405 {
            return match self.stat(relative) {
                Ok(_) => Err(failure(p::FailureCode::AlreadyExists)),
                Err(error) if error.code == p::FailureCode::NotFound as i32 => {
                    Err(failure(p::FailureCode::UnsupportedOperation))
                }
                Err(error) => Err(error),
            };
        }
        if reply.status().as_u16() != 201 {
            return Err(status_error(reply.status().as_u16(), true));
        }
        Ok(p::Entry {
            path: relative.into(),
            kind: p::EntryKind::Dir as i32,
            size: None,
            modified: None,
        })
    }
    pub fn create_dir(&self, relative: &str, recursive: bool) -> Result<p::Entry> {
        path(relative)?;
        if relative.is_empty() {
            if !recursive {
                return Err(failure(p::FailureCode::AlreadyExists));
            }
            let root = self.stat("")?.entry;
            if root.kind != p::EntryKind::Dir as i32 {
                return Err(failure(p::FailureCode::NotDir));
            }
            return Ok(root);
        }
        if !recursive {
            match self.stat(relative) {
                Ok(_) => return Err(failure(p::FailureCode::AlreadyExists)),
                Err(error) if error.code == p::FailureCode::NotFound as i32 => {}
                Err(error) => return Err(error),
            }
            self.parent(relative)?;
            return self.make_collection(relative);
        }
        let mut prefix = String::new();
        let mut result = None;
        for name in relative.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(name);
            let metadata = match self.stat(&prefix) {
                Ok(metadata) => metadata.entry,
                Err(error) if error.code == p::FailureCode::NotFound as i32 => {
                    match self.make_collection(&prefix) {
                        Ok(entry) => entry,
                        Err(error) if error.code == p::FailureCode::AlreadyExists as i32 => {
                            self.stat(&prefix)?.entry
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            };
            if metadata.kind != p::EntryKind::Dir as i32 {
                return Err(failure(p::FailureCode::NotDir));
            }
            result = Some(metadata);
        }
        result.ok_or_else(|| failure(p::FailureCode::InvalidData))
    }
    pub fn read_range(
        &self,
        relative: &str,
        offset: u64,
        length: u64,
        size: u64,
        etag: Option<&str>,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        let end = offset
            .checked_add(length)
            .and_then(|n| n.checked_sub(1))
            .ok_or_else(|| failure(p::FailureCode::InvalidRequest))?;
        let mut headers = vec![("range", format!("bytes={offset}-{end}"))];
        if let Some(etag) = etag {
            headers.push(("if-match", etag.into()));
        }
        let response = self.request("GET", self.url(relative, false)?, headers, vec![])?;
        if response.status().as_u16() == 412 || response.status().as_u16() == 416 {
            return Err(failure(p::FailureCode::Conflict));
        }
        if !matches!(response.status().as_u16(), 200 | 206) {
            return Err(status_error(response.status().as_u16(), false));
        }
        if let Some(encoding) = header(&response, "content-encoding") {
            if !encoding.eq_ignore_ascii_case("identity") {
                return Err(failure(p::FailureCode::UnsupportedOperation));
            }
        }
        if etag.is_some_and(|etag| header(&response, "etag").is_some_and(|actual| actual != etag)) {
            return Err(failure(p::FailureCode::Conflict));
        }
        match response.status().as_u16() {
            206 => {
                let range = header(&response, "content-range")
                    .and_then(|value| value.strip_prefix("bytes "))
                    .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
                let (bounds, total) = range
                    .split_once('/')
                    .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
                let (start, last) = bounds
                    .split_once('-')
                    .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
                let parse = |value: &str| {
                    value
                        .parse::<u64>()
                        .map_err(|_| failure(p::FailureCode::InvalidData))
                };
                if parse(total)? != size {
                    return Err(failure(p::FailureCode::Conflict));
                }
                if parse(start)? != offset
                    || parse(last)? != end
                    || response.body().len() as u64 != length
                {
                    return Err(failure(p::FailureCode::InvalidData));
                }
                Ok((response.into_body(), None))
            }
            200 => {
                // Servers may ignore Range. Only accept an entire, bounded body
                // whose size matches the discovered representation; never return
                // the beginning of the file for a nonzero requested offset.
                if response.body().len() as u64 != size {
                    return Err(failure(p::FailureCode::Conflict));
                }
                let start =
                    usize::try_from(offset).map_err(|_| failure(p::FailureCode::TooLarge))?;
                let end = start
                    .checked_add(length as usize)
                    .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
                let bytes = response
                    .body()
                    .get(start..end)
                    .ok_or_else(|| failure(p::FailureCode::InvalidData))?
                    .to_vec();
                Ok((bytes, Some(response.into_body())))
            }
            status => Err(status_error(status, false)),
        }
    }
}
