//! WebDAV/HTTP behavior kept inside the plugin, with no host-specific branches.
//! See RFC 4918 sections 9.1, 9.3, 9.6 and 9.9, and RFC 9110 conditional requests.
use super::*;
use ah_vfs_storage::{Entry, EntryType, MutationOutcome, StoragePath};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use percent_encoding::percent_decode_str;
use time::{OffsetDateTime, format_description::well_known::Rfc2822};
use url::Url;

const PROPERTIES: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:getetag/><d:resource-id/></d:prop></d:propfind>"#;

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
    resource_id: Option<String>,
}
impl PartialEq for Metadata {
    fn eq(&self, other: &Self) -> bool {
        if self.entry.entry_type() == EntryType::Directory {
            return match (&self.resource_id, &other.resource_id) {
                (Some(left), Some(right)) => left == right,
                _ => self.entry == other.entry && self.etag == other.etag,
            };
        }
        self.entry == other.entry
            && self.etag == other.etag
            && self.resource_id == other.resource_id
    }
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
pub(super) struct WriteCondition {
    parent: String,
    target_etag: Option<String>,
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
                resource_id: properties.resource_id.map(|value| value.trim().to_owned()),
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
    pub fn select_observed_directory(
        &self,
        metadata: &Metadata,
        generation: &[u8; 16],
    ) -> Result<ah_vfs_storage::DirectorySelection> {
        if metadata.entry.entry_type() != EntryType::Directory {
            return Err(self
                .context
                .failure(Failure::Rejected(Rejection::NotDirectory)));
        }
        let identity =
            directory_identity(generation, metadata.resource_id.as_deref()).ok_or_else(|| {
                self.context
                    .failure(Failure::Rejected(Rejection::Unsupported))
            })?;
        Ok(ah_vfs_storage::DirectorySelection::from_lookup(
            metadata.entry.path().clone(),
            identity,
        ))
    }

    pub fn resume_directory(
        &self,
        cursor: &ah_vfs_storage::DirectoryCursor,
        generation: &[u8; 16],
    ) -> Result<ah_vfs_storage::DirectorySelection> {
        let stale = || {
            self.context
                .failure(Failure::Backend(BackendFailure::Stale))
        };
        let selection = cursor.selection();
        let metadata = self
            .stat(selection.address().as_str())
            .map_err(|error| self.list_replaced(error, true))?;
        if metadata.entry.entry_type() != EntryType::Directory
            || directory_identity(generation, metadata.resource_id.as_deref())
                != Some(selection.identity())
        {
            return Err(stale());
        }
        Ok(selection.clone())
    }

    fn list_replaced(&self, error: api::Failure, resuming: bool) -> api::Failure {
        if error.storage_error().is_some_and(|error| {
            matches!(
                error.failure(),
                Failure::Rejected(Rejection::NotFound | Rejection::NotDirectory)
            )
        }) {
            self.context.failure(list_replacement_failure(resuming))
        } else {
            error
        }
    }

    pub fn list_page(
        &self,
        bound: &ah_vfs_storage::BoundListRequest,
        generation: &[u8; 16],
    ) -> Result<ah_vfs_storage::DirectoryPage> {
        let context = &self.context;
        let invalid = || context.failure(Failure::Contract(ContractViolation::InvalidPage));
        let resuming = bound.after().is_some();
        let replaced = || context.failure(list_replacement_failure(resuming));
        let selection = bound.selection();
        let mut parent = false;
        let mut seen = std::collections::BTreeSet::new();
        let mut children = BTreeMap::new();
        for metadata in self
            .propfind(selection.address().as_str(), 1)
            .map_err(|error| self.list_replaced(error, resuming))?
        {
            if metadata.entry.path() == selection.address() {
                if parent {
                    return Err(invalid());
                }
                if metadata.entry.entry_type() != EntryType::Directory
                    || directory_identity(generation, metadata.resource_id.as_deref())
                        != Some(selection.identity())
                {
                    return Err(replaced());
                }
                parent = true;
                continue;
            }
            let name = metadata.entry.path().name().ok_or_else(invalid)?;
            if metadata.entry.path().parent().as_ref() != Some(selection.address())
                || !seen.insert(name.clone())
            {
                return Err(invalid());
            }
            if bound.after().is_none_or(|after| &name > after) {
                children.insert(name, metadata.entry);
                if children.len() > bound.size().get() + 1 {
                    children.pop_last();
                }
            }
        }
        if !parent {
            return Err(invalid());
        }
        let more = children.len() > bound.size().get();
        let entries = children.into_values().take(bound.size().get()).collect();
        ah_vfs_storage::DirectoryPage::new(bound, entries, more).map_err(|_| invalid())
    }

    pub fn write_condition(
        &self,
        parent: &Metadata,
        target: Option<&Metadata>,
    ) -> Result<WriteCondition> {
        let unsupported = || {
            self.context
                .failure(Failure::Rejected(Rejection::Unsupported))
        };
        let parent_etag = parent.strong_etag().ok_or_else(unsupported)?;
        let parent_url = self.url(parent.entry.path().as_str(), true)?;
        let target_etag = target
            .map(|metadata| metadata.strong_etag().ok_or_else(unsupported))
            .transpose()?;
        Ok(WriteCondition {
            parent: format!("<{parent_url}> ([{parent_etag}])"),
            target_etag,
        })
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
        let conditional = match condition.target_etag {
            None => ("if-none-match", "*".into()),
            Some(etag) => ("if-match", etag),
        };
        let reply = self.request(
            "PUT",
            self.url(relative, false)?,
            vec![
                conditional,
                ("if", condition.parent),
                ("content-type", "application/octet-stream".into()),
            ],
            bytes,
        )?;
        if matches!(reply.status().as_u16(), 409 | 412) {
            return Err(ah_vfs_storage::StorageError::without_effects(
                Operation::Write,
                Failure::Rejected(Rejection::Conflict),
                None,
            )
            .into());
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
    pub fn make_collection(&self, relative: &StoragePath, parent: &Metadata) -> Result<bool> {
        let context = &self.context;
        let tag = parent
            .strong_etag()
            .ok_or_else(|| context.failure(Failure::Rejected(Rejection::Unsupported)))?;
        let parent_url = self.url(parent.entry.path().as_str(), true)?;
        let condition = format!("<{parent_url}> ([{tag}])");
        // Keep per-request dispatch evidence separate from earlier successful
        // MKCOL steps so a known refusal cannot erase a visible parent.
        let attempt = self.for_operation(Operation::CreateDirectory);
        let reply = attempt
            .request(
                "MKCOL",
                self.url(relative.as_str(), true)?,
                vec![("if", condition)],
                vec![],
            )
            .map_err(|error| {
                if let Some(storage) = error.storage_error() {
                    if storage.outcome() == MutationOutcome::Unknown {
                        context.dispatched();
                    }
                    return context.failure(storage.failure());
                }
                error
            })?;
        match reply.status().as_u16() {
            201 => {
                context.dispatched();
                Ok(true)
            }
            405 => Ok(false),
            412 => Err(context.failure(Failure::Rejected(Rejection::Conflict))),
            // RFC 4918 MKCOL refusals do not create this collection.
            status @ (403 | 409 | 415 | 507) => Err(status_error(context, status)),
            status => {
                context.dispatched();
                Err(status_error(context, status))
            }
        }
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
        if let Some(encoding) = header(&response, "content-encoding")
            && !encoding.eq_ignore_ascii_case("identity")
        {
            return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
        }
        if let Some(etag) = etag {
            match header(&response, "etag") {
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

fn directory_identity(
    generation: &[u8; 16],
    resource_id: Option<&str>,
) -> Option<ah_vfs_storage::DirectoryIdentity> {
    use sha2::{Digest, Sha256};
    let resource_id = resource_id?;
    if resource_id.len() > 4096 || Url::parse(resource_id).is_err() {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(generation);
    hash.update(resource_id.as_bytes());
    let hash = hash.finalize();
    Some(ah_vfs_storage::DirectoryIdentity::from_bytes(
        hash[..16].try_into().ok()?,
    ))
}

fn list_replacement_failure(resuming: bool) -> Failure {
    if resuming {
        Failure::Backend(BackendFailure::Stale)
    } else {
        Failure::Rejected(Rejection::Conflict)
    }
}
