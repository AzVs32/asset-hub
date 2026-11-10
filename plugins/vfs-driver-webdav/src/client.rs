//! WebDAV/HTTP behavior kept inside the plugin, with no host-specific branches.
//! See RFC 4918 sections 9.1, 9.3, 9.6 and 9.9, and RFC 9110 conditional requests.
use super::*;
use ah_vfs_storage::{Entry, EntryName, EntryType, PageSize, StoragePath};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use percent_encoding::percent_decode_str;
use time::{OffsetDateTime, format_description::well_known::Rfc2822};
use url::Url;

const PROPERTIES: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:getetag/></d:prop></d:propfind>"#;

#[derive(Clone)]
pub(super) struct Client {
    pub(super) context: api::ErrorContext,
    pub endpoint: Url,
    pub max_write_bytes: usize,
    authorization: Option<String>,
    root_segments: Vec<String>,
}
pub(super) struct Metadata {
    pub entry: Entry,
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
fn decoded_segments(context: &api::ErrorContext, url: &Url) -> Result<Vec<String>> {
    let mut segments = vec![];
    let raw = url
        .path()
        .strip_prefix('/')
        .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
    let raw = raw.strip_suffix('/').unwrap_or(raw);
    if raw.is_empty() {
        return Ok(segments);
    }
    for segment in raw.split('/') {
        if segment.is_empty() {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        let bytes = segment.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'%'
                && (index + 2 >= bytes.len()
                    || !bytes[index + 1].is_ascii_hexdigit()
                    || !bytes[index + 2].is_ascii_hexdigit())
            {
                return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
            }
        }
        let decoded = percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::UnrepresentableName)))?;
        if decoded.contains(['/', '\\', '\0']) || matches!(decoded.as_ref(), "." | "..") {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        segments.push(decoded.into_owned());
    }
    Ok(segments)
}
fn modified(
    context: &api::ErrorContext,
    value: Option<&str>,
) -> Result<Option<std::time::SystemTime>> {
    value
        .map(|value| {
            let time = OffsetDateTime::parse(value.trim(), &Rfc2822)
                .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
            p::decode_time(time.unix_timestamp(), time.nanosecond())
                .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))
        })
        .transpose()
}

fn header<'a>(response: &'a http::Response<Vec<u8>>, name: &str) -> Option<&'a str> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}
fn status_error(context: &api::ErrorContext, status: u16) -> api::Failure {
    context.failure(match status {
        400 | 422 => Failure::Rejected(Rejection::Conflict),
        401 | 403 => Failure::Rejected(Rejection::AccessDenied),
        404 | 410 => Failure::Rejected(Rejection::NotFound),
        405 | 415 | 501 => Failure::Rejected(Rejection::Unsupported),
        408 => Failure::Backend(BackendFailure::TimedOut),
        409 | 412 | 423 | 424 => Failure::Rejected(Rejection::Conflict),
        413 => Failure::Rejected(Rejection::LimitExceeded),
        429 => Failure::Backend(BackendFailure::RateLimited),
        507 => Failure::Backend(BackendFailure::CapacityExceeded),
        500..=599 => Failure::Backend(BackendFailure::Unavailable),
        _ => Failure::Backend(BackendFailure::Other),
    })
}
impl Client {
    pub(super) fn for_operation(&self, operation: Operation) -> Self {
        let mut client = self.clone();
        client.context = api::ErrorContext::storage(operation);
        client
    }

    pub fn parse(fields: &p::JsonObject) -> Result<Self> {
        let context = &api::ErrorContext::lifecycle();
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
        let root_segments = decoded_segments(context, &endpoint)
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
            context: api::ErrorContext::lifecycle(),
            endpoint,
            max_write_bytes,
            authorization,
            root_segments,
        })
    }
    fn url(&self, relative: &str, collection: bool) -> Result<Url> {
        let context = &self.context;
        path(context, relative)?;
        let mut url = self.endpoint.clone();
        if !relative.is_empty() {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
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
        let context = &self.context;
        if url.origin() != self.endpoint.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        let segments = decoded_segments(context, url)?;
        if !segments.starts_with(&self.root_segments) {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        let relative = segments[self.root_segments.len()..].join("/");
        path(context, &relative)
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::UnrepresentableName)))?;
        Ok(relative)
    }
    fn request(
        &self,
        method: &str,
        url: Url,
        extra: Vec<(&str, String)>,
        body: Vec<u8>,
    ) -> Result<http::Response<Vec<u8>>> {
        let context = &self.context;
        if body.len() > p::MAX_DATA_BYTES {
            return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
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
        let request = request
            .body(body)
            .map_err(|_| context.failure(Failure::Contract(ContractViolation::InvalidResponse)))?;
        if mutation {
            context.dispatched();
        }
        vfs_plugin_guest::wasi_http::send(request).map_err(|cause| context.failure(cause))
    }

    fn propfind(&self, relative: &str, depth: u8) -> Result<Vec<Metadata>> {
        let context = &self.context;
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
            return Err(status_error(context, reply.status().as_u16()));
        }
        let mut metadata = vec![];
        for response in multistatus::parse(context, reply.body())? {
            let href = response
                .href
                .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
            let url = base
                .join(href.trim())
                .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
            let path = self.relative(&url)?;
            if let Some(status) = response.status {
                let status = multistatus::status(context, &status)?;
                if !(200..300).contains(&status) {
                    return Err(status_error(context, status));
                }
            }
            let properties = response.properties;
            let collection = properties
                .collection
                .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
            let kind = if properties.other_type {
                EntryType::Other
            } else if collection {
                EntryType::Directory
            } else {
                let size = properties
                    .length
                    .map(|value| {
                        value.trim().parse::<u64>().map_err(|_| {
                            context.failure(Failure::Backend(BackendFailure::CorruptData))
                        })
                    })
                    .transpose()?;
                EntryType::file(size)
            };
            metadata.push(Metadata {
                entry: entry(
                    context,
                    &path,
                    kind,
                    modified(context, properties.modified.as_deref())?,
                )?,
                etag: properties.etag.map(|value| value.trim().to_owned()),
            });
        }
        Ok(metadata)
    }
    pub fn stat(&self, relative: &str) -> Result<Metadata> {
        let context = &self.context;
        path(context, relative)?;
        let mut entries = self.propfind(relative, 0)?;
        if entries.len() != 1 || entries[0].entry.path().as_str() != relative {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        entries
            .pop()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))
    }
    pub fn check_root(&self) -> Result<()> {
        let context = &self.context;
        if self.stat("")?.entry.entry_type() != EntryType::Directory {
            return Err(context.failure(Failure::Rejected(Rejection::NotDirectory)));
        }
        Ok(())
    }
    pub fn list(
        &self,
        relative: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<p::Entry>, Option<String>)> {
        let context = &self.context;
        path(context, relative)?;
        if usize::try_from(limit)
            .ok()
            .and_then(|limit| PageSize::new(limit).ok())
            .is_none()
        {
            return Err(context.failure(Failure::Rejected(Rejection::Conflict)));
        }
        if let Some(after) = after {
            let directory = StoragePath::parse(relative)
                .map_err(|_| context.failure(Failure::Rejected(Rejection::UnsupportedName)))?;
            let name = EntryName::parse(after)
                .map_err(|_| context.failure(Failure::Rejected(Rejection::UnsupportedName)))?;
            directory
                .join(&name)
                .map_err(|_| context.failure(Failure::Rejected(Rejection::UnsupportedName)))?;
        }
        if self.stat(relative)?.entry.entry_type() != EntryType::Directory {
            return Err(context.failure(Failure::Rejected(Rejection::NotDirectory)));
        }
        let mut parent = false;
        let mut children = BTreeMap::new();
        let prefix = if relative.is_empty() {
            String::new()
        } else {
            format!("{relative}/")
        };
        for metadata in self.propfind(relative, 1)? {
            if metadata.entry.path().as_str() == relative {
                if parent {
                    return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
                }
                if metadata.entry.entry_type() != EntryType::Directory {
                    return Err(context.failure(Failure::Rejected(Rejection::NotDirectory)));
                }
                parent = true;
                continue;
            }
            let name = metadata
                .entry
                .path()
                .as_str()
                .strip_prefix(&prefix)
                .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?
                .to_owned();
            if name.is_empty()
                || name.contains('/')
                || children.insert(name, metadata.entry).is_some()
            {
                return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
            }
        }
        if !parent {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        let mut candidates = children
            .into_iter()
            .filter(|(name, _)| after.is_none_or(|after| name.as_str() > after))
            .map(|(name, entry)| Ok((name, crate::encode_entry(context, &entry)?)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
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
                .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
            entries.push(entry);
        }
        if entries.is_empty() && candidates.peek().is_some() {
            return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
        }
        let next = if candidates.peek().is_some() {
            last_name
        } else {
            None
        };
        Ok((entries, next))
    }
    fn parent(&self, relative: &str) -> Result<()> {
        let context = &self.context;
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        if self.stat(parent)?.entry.entry_type() != EntryType::Directory {
            return Err(context.failure(Failure::Rejected(Rejection::NotDirectory)));
        }
        Ok(())
    }
    pub fn write_condition(&self, relative: &str, replace: bool) -> Result<WriteCondition> {
        let context = &self.context;
        path(context, relative)?;
        if relative.is_empty() {
            return Err(context.failure(Failure::Rejected(Rejection::RootProtected)));
        }
        self.parent(relative)?;
        match self.stat(relative) {
            Ok(_) if !replace => Err(context.failure(Failure::Rejected(Rejection::AlreadyExists))),
            Ok(metadata) if metadata.entry.entry_type() == EntryType::Directory => {
                Err(context.failure(Failure::Rejected(Rejection::IsDirectory)))
            }
            Ok(metadata) if matches!(metadata.entry.entry_type(), EntryType::File { .. }) => {
                metadata
                    .strong_etag()
                    .map(WriteCondition::Match)
                    .ok_or_else(|| context.failure(Failure::Rejected(Rejection::Unsupported)))
            }
            Ok(_) => Err(context.failure(Failure::Rejected(Rejection::Unsupported))),
            Err(error)
                if error.storage_error().is_some_and(|error| {
                    error.failure() == Failure::Rejected(Rejection::NotFound)
                }) =>
            {
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
    ) -> Result<Entry> {
        let context = &self.context;
        let size = bytes.len();
        if size > self.max_write_bytes {
            return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
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
            return Err(context.failure(if absent {
                Failure::Rejected(Rejection::AlreadyExists)
            } else {
                Failure::Rejected(Rejection::Conflict)
            }));
        }
        if !matches!(reply.status().as_u16(), 200 | 201 | 204) {
            return Err(status_error(context, reply.status().as_u16()));
        }
        // A successful PUT is already a known publication. Avoid a follow-up
        // PROPFIND turning a successful commit into a misleading read failure.
        entry(
            context,
            relative,
            EntryType::file(Some(size as u64)),
            modified(context, header(&reply, "last-modified"))?,
        )
    }
    fn make_collection(&self, relative: &str) -> Result<Entry> {
        let context = &self.context;
        let reply = self.request("MKCOL", self.url(relative, true)?, vec![], vec![])?;
        if reply.status().as_u16() == 405 {
            return match self.stat(relative) {
                Ok(_) => Err(context.failure(Failure::Rejected(Rejection::AlreadyExists))),
                Err(error)
                    if error.storage_error().is_some_and(|error| {
                        error.failure() == Failure::Rejected(Rejection::NotFound)
                    }) =>
                {
                    Err(context.failure(Failure::Rejected(Rejection::Unsupported)))
                }
                Err(error) => Err(error),
            };
        }
        if reply.status().as_u16() != 201 {
            return Err(status_error(context, reply.status().as_u16()));
        }
        entry(context, relative, EntryType::Directory, None)
    }
    pub fn create_dir(&self, relative: &str, recursive: bool) -> Result<Entry> {
        let context = &self.context;
        path(context, relative)?;
        if relative.is_empty() {
            if !recursive {
                return Err(context.failure(Failure::Rejected(Rejection::AlreadyExists)));
            }
            let root = self.stat("")?.entry;
            if root.entry_type() != EntryType::Directory {
                return Err(context.failure(Failure::Rejected(Rejection::NotDirectory)));
            }
            return Ok(root);
        }
        if !recursive {
            match self.stat(relative) {
                Ok(_) => return Err(context.failure(Failure::Rejected(Rejection::AlreadyExists))),
                Err(error)
                    if error.storage_error().is_some_and(|error| {
                        error.failure() == Failure::Rejected(Rejection::NotFound)
                    }) => {}
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
                Err(error)
                    if error.storage_error().is_some_and(|error| {
                        error.failure() == Failure::Rejected(Rejection::NotFound)
                    }) =>
                {
                    match self.make_collection(&prefix) {
                        Ok(entry) => entry,
                        Err(error)
                            if error.storage_error().is_some_and(|error| {
                                error.failure() == Failure::Rejected(Rejection::AlreadyExists)
                            }) =>
                        {
                            self.stat(&prefix)?.entry
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            };
            if metadata.entry_type() != EntryType::Directory {
                return Err(context.failure(Failure::Rejected(Rejection::NotDirectory)));
            }
            result = Some(metadata);
        }
        result.ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))
    }
    pub fn read_range(
        &self,
        relative: &str,
        offset: u64,
        length: u64,
        size: u64,
        etag: Option<&str>,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        let context = &self.context;
        let end = offset
            .checked_add(length)
            .and_then(|n| n.checked_sub(1))
            .ok_or_else(|| context.failure(Failure::Rejected(Rejection::Conflict)))?;
        let mut headers = vec![("range", format!("bytes={offset}-{end}"))];
        if let Some(etag) = etag {
            headers.push(("if-match", etag.into()));
        }
        let response = self.request("GET", self.url(relative, false)?, headers, vec![])?;
        if response.status().as_u16() == 412 || response.status().as_u16() == 416 {
            return Err(context.failure(Failure::Rejected(Rejection::Conflict)));
        }
        if !matches!(response.status().as_u16(), 200 | 206) {
            return Err(status_error(context, response.status().as_u16()));
        }
        if let Some(encoding) = header(&response, "content-encoding") {
            if !encoding.eq_ignore_ascii_case("identity") {
                return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
            }
        }
        if let Some(etag) = etag {
            match header(&response, "etag").as_deref() {
                Some(actual) if actual == etag => {}
                Some(_) => return Err(context.failure(Failure::Rejected(Rejection::Conflict))),
                None => return Err(context.failure(Failure::Rejected(Rejection::Unsupported))),
            }
        }
        match response.status().as_u16() {
            206 => {
                let range = header(&response, "content-range")
                    .and_then(|value| value.strip_prefix("bytes "))
                    .ok_or_else(|| {
                        context.failure(Failure::Backend(BackendFailure::CorruptData))
                    })?;
                let (bounds, total) = range.split_once('/').ok_or_else(|| {
                    context.failure(Failure::Backend(BackendFailure::CorruptData))
                })?;
                let (start, last) = bounds.split_once('-').ok_or_else(|| {
                    context.failure(Failure::Backend(BackendFailure::CorruptData))
                })?;
                let parse = |value: &str| {
                    value
                        .parse::<u64>()
                        .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))
                };
                if parse(total)? != size {
                    return Err(context.failure(Failure::Rejected(Rejection::Conflict)));
                }
                if parse(start)? != offset
                    || parse(last)? != end
                    || response.body().len() as u64 != length
                {
                    return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
                }
                Ok((response.into_body(), None))
            }
            200 => {
                // Servers may ignore Range. Only accept an entire, bounded body
                // whose size matches the discovered representation; never return
                // the beginning of the file for a nonzero requested offset.
                if response.body().len() as u64 != size {
                    return Err(context.failure(Failure::Rejected(Rejection::Conflict)));
                }
                let start = usize::try_from(offset)
                    .map_err(|_| context.failure(Failure::Rejected(Rejection::LimitExceeded)))?;
                let end = start.checked_add(length as usize).ok_or_else(|| {
                    context.failure(Failure::Backend(BackendFailure::CorruptData))
                })?;
                let bytes = response
                    .body()
                    .get(start..end)
                    .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?
                    .to_vec();
                Ok((bytes, Some(response.into_body())))
            }
            status => Err(status_error(context, status)),
        }
    }
}

fn entry(
    context: &api::ErrorContext,
    path: &str,
    kind: EntryType,
    modified: Option<std::time::SystemTime>,
) -> Result<Entry> {
    let path = StoragePath::parse(path)
        .map_err(|_| context.failure(Failure::Backend(BackendFailure::UnrepresentableName)))?;
    Entry::new(path, kind, modified)
        .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))
}
