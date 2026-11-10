//! Small SigV4 S3 client using only the component's HTTP import. Multipart writes
//! bound memory to one part; conditional completion implements create-new.
use super::*;
use ah_vfs_storage::{Entry, EntryType, StoragePath};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

#[derive(Clone)]
pub(super) struct Client {
    pub(super) context: api::ErrorContext,
    pub endpoint: String,
    pub bucket: String,
    region: String,
    access: String,
    secret: String,
}
fn text(fields: &p::JsonObject, name: &str) -> Result<String> {
    let value = fields
        .get(name)
        .ok_or_else(|| invalid(name, p::ConfigErrorKind::MissingField))?;
    let text = value
        .as_str()
        .ok_or_else(|| invalid(name, p::ConfigErrorKind::InvalidType))?;
    if text.is_empty() {
        return Err(invalid(name, p::ConfigErrorKind::InvalidValue));
    }
    Ok(text.into())
}
fn percent(value: &str, slash: bool) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~')
            || (slash && b == b'/')
        {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn hmac(context: &api::ErrorContext, key: &[u8], bytes: &[u8]) -> Result<Vec<u8>> {
    let mut h = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| context.failure(Failure::Backend(BackendFailure::Other)))?;
    h.update(bytes);
    Ok(h.finalize().into_bytes().to_vec())
}
fn xml<'a>(context: &api::ErrorContext, bytes: &'a [u8]) -> Result<&'a str> {
    std::str::from_utf8(bytes)
        .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))
}
pub(super) fn header(response: &http::Response<Vec<u8>>, key: &str) -> Option<String> {
    response
        .headers()
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
impl Client {
    pub(super) fn for_operation(&self, operation: Operation) -> Self {
        let mut client = self.clone();
        client.context = api::ErrorContext::storage(operation);
        client
    }

    pub fn parse(fields: &p::JsonObject) -> Result<Self> {
        if fields.keys().any(|k| {
            !["endpoint", "bucket", "region", "access_key", "secret_key"].contains(&k.as_str())
        }) {
            return Err(invalid("", p::ConfigErrorKind::UnknownField));
        }
        let endpoint = text(fields, "endpoint")?;
        let url = url::Url::parse(&endpoint)
            .map_err(|_| invalid("endpoint", p::ConfigErrorKind::InvalidValue))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid("endpoint", p::ConfigErrorKind::InvalidValue));
        }
        let bucket = text(fields, "bucket")?;
        if !(3..=63).contains(&bucket.len())
            || !bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
        {
            return Err(invalid("bucket", p::ConfigErrorKind::InvalidValue));
        }
        let region = text(fields, "region")?;
        if region.len() > 64
            || !region
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(invalid("region", p::ConfigErrorKind::InvalidValue));
        }
        Ok(Self {
            context: api::ErrorContext::lifecycle(),
            endpoint: url.origin().ascii_serialization(),
            bucket,
            region,
            access: text(fields, "access_key")?,
            secret: text(fields, "secret_key")?,
        })
    }
    pub fn request(
        &self,
        method: &str,
        key: &str,
        query: Vec<(String, String)>,
        extra: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Result<http::Response<Vec<u8>>> {
        let context = &self.context;
        let publishes = (method == "DELETE" && !query.iter().any(|(key, _)| key == "uploadId"))
            || (method == "PUT" && !query.iter().any(|(key, _)| key == "uploadId"))
            || (method == "POST" && query.iter().any(|(key, _)| key == "uploadId"));
        let guarded_write = publishes
            && extra
                .iter()
                .any(|(header, _)| matches!(header.as_str(), "if-match" | "if-none-match"));
        let request = self.sign(
            method,
            key,
            query,
            extra,
            body,
            i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| context.failure(Failure::Backend(BackendFailure::Other)))?
                    .as_secs(),
            )
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::Other)))?,
        )?;
        if publishes {
            context.dispatched();
        }
        let response =
            vfs_plugin_guest::wasi_http::send(request).map_err(|cause| context.failure(cause))?;
        match response.status().as_u16() {
            200..=299 => Ok(response),
            404 | 409 | 412 if guarded_write => Err(ah_vfs_storage::StorageError::without_effects(
                Operation::Write,
                Failure::Rejected(Rejection::Conflict),
                None,
            )
            .into()),
            403 => Err(context.failure(Failure::Rejected(Rejection::AccessDenied))),
            404 => Err(context.failure(Failure::Rejected(Rejection::NotFound))),
            409 => Err(context.failure(Failure::Rejected(Rejection::Conflict))),
            412 if method == "GET" => Err(context.failure(Failure::Rejected(Rejection::Conflict))),
            412 => Err(context.failure(Failure::Rejected(Rejection::AlreadyExists))),
            429 => Err(context.failure(Failure::Backend(BackendFailure::RateLimited))),
            500..=599 => Err(context.failure(Failure::Backend(BackendFailure::Unavailable))),
            _ => Err(context.failure(Failure::Backend(BackendFailure::Other))),
        }
    }
    fn sign(
        &self,
        method: &str,
        key: &str,
        query: Vec<(String, String)>,
        extra: Vec<(String, String)>,
        body: Vec<u8>,
        now: i64,
    ) -> Result<http::Request<Vec<u8>>> {
        let context = &self.context;
        let time = OffsetDateTime::from_unix_timestamp(now)
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
        let format = time::format_description::parse_borrowed::<2>(
            "[year][month][day]T[hour][minute][second]Z",
        )
        .map_err(|_| context.failure(Failure::Backend(BackendFailure::Other)))?;
        let timestamp = time
            .format(&format)
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::Other)))?;
        let date = &timestamp[..8];
        let url = url::Url::parse(&self.endpoint)
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
        let host = url[url::Position::BeforeHost..url::Position::AfterPort].to_owned();
        let uri = format!("/{}/{}", self.bucket, percent(key, true));
        let mut query: Vec<_> = query
            .into_iter()
            .map(|(k, v)| (percent(&k, false), percent(&v, false)))
            .collect();
        query.sort();
        let query = query
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let hash = hex::encode(Sha256::digest(&body));
        let mut headers = BTreeMap::from([
            ("host".to_owned(), host),
            ("x-amz-content-sha256".to_owned(), hash.clone()),
            ("x-amz-date".to_owned(), timestamp.clone()),
        ]);
        for (k, v) in extra {
            headers.insert(k.to_ascii_lowercase(), v.trim().to_owned());
        }
        let canonical_headers = headers
            .iter()
            .map(|(k, v)| format!("{k}:{v}\n"))
            .collect::<String>();
        let signed = headers.keys().cloned().collect::<Vec<_>>().join(";");
        let canonical = format!("{method}\n{uri}\n{query}\n{canonical_headers}\n{signed}\n{hash}");
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );
        let k = hmac(
            context,
            format!("AWS4{}", self.secret).as_bytes(),
            date.as_bytes(),
        )?;
        let k = hmac(context, &k, self.region.as_bytes())?;
        let k = hmac(context, &k, b"s3")?;
        let k = hmac(context, &k, b"aws4_request")?;
        let signature = hex::encode(hmac(context, &k, to_sign.as_bytes())?);
        headers.remove("host");
        headers.insert("authorization".into(),format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}",self.access));
        let url = format!(
            "{}{uri}{}",
            self.endpoint,
            if query.is_empty() {
                String::new()
            } else {
                format!("?{query}")
            }
        );
        let mut request = http::Request::builder().method(method).uri(url);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        request
            .body(body)
            .map_err(|_| context.failure(Failure::Rejected(Rejection::Conflict)))
    }
    pub fn check_bucket(&self) -> Result<()> {
        self.request("HEAD", "", vec![], vec![], vec![])?;
        Ok(())
    }
    pub fn head(&self, name: &str) -> Result<Option<(Entry, Option<String>)>> {
        let context = &self.context;
        path(context, name)?;
        let response = match self.request("HEAD", name, vec![], vec![], vec![]) {
            Ok(r) => r,
            Err(e)
                if e.storage_error().is_some_and(|error| {
                    error.failure() == Failure::Rejected(Rejection::NotFound)
                }) =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let size = header(&response, "content-length")
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
        let modified = header(&response, "last-modified")
            .and_then(|s| {
                OffsetDateTime::parse(&s, &time::format_description::well_known::Rfc2822).ok()
            })
            .map(|time| p::decode_time(time.unix_timestamp(), time.nanosecond()))
            .transpose()
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
        let etag = header(&response, "etag").filter(|tag| {
            tag.len() >= 2
                && tag.len() <= 4096
                && tag.starts_with('"')
                && tag.ends_with('"')
                && tag.as_bytes()[1..tag.len() - 1]
                    .iter()
                    .all(|byte| *byte >= 0x21 && *byte != b'"' && *byte != 0x7f)
        });
        Ok(Some((
            entry(context, name, EntryType::file(Some(size)), modified)?,
            etag,
        )))
    }
    fn prefix_exists(&self, prefix: &str) -> Result<bool> {
        let query = vec![
            ("list-type".into(), "2".into()),
            ("delimiter".into(), "/".into()),
            ("prefix".into(), prefix.into()),
            ("max-keys".into(), "1".into()),
        ];
        let reply = self.request("GET", "", query, vec![], vec![])?;
        let listed: Listing =
            quick_xml::de::from_str(xml(&self.context, reply.body())?).map_err(|_| {
                self.context
                    .failure(Failure::Backend(BackendFailure::CorruptData))
            })?;
        Ok(!listed.contents.is_empty() || !listed.common_prefixes.is_empty())
    }
    pub fn stat(&self, name: &str) -> Result<Entry> {
        self.stat_object(name).map(|(entry, _)| entry)
    }

    pub fn stat_object(&self, name: &str) -> Result<(Entry, Option<String>)> {
        let context = &self.context;
        path(context, name)?;
        if name.is_empty() {
            return Ok((entry(context, "", EntryType::Directory, None)?, None));
        }
        let file = self.head(name)?;
        let prefix = format!("{name}/");
        let directory = self.prefix_exists(&prefix)?;
        match (file, directory) {
            (Some(_), true) => Err(context.failure(Failure::Backend(BackendFailure::CorruptData))),
            (Some(file), false) => Ok(file),
            (None, true) => Ok((entry(context, name, EntryType::Directory, None)?, None)),
            (None, false) => Err(context.failure(Failure::Rejected(Rejection::NotFound))),
        }
    }
    pub fn start_upload(&self, name: &str) -> Result<String> {
        let context = &self.context;
        let r = self.request(
            "POST",
            name,
            vec![("uploads".into(), String::new())],
            vec![],
            vec![],
        )?;
        let upload: Initiated = quick_xml::de::from_str(xml(context, r.body())?)
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
        if upload.upload_id.is_empty() {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        Ok(upload.upload_id)
    }
    pub fn upload_part(
        &self,
        name: &str,
        upload: &str,
        part: usize,
        data: Vec<u8>,
    ) -> Result<String> {
        let context = &self.context;
        let r = self.request(
            "PUT",
            name,
            vec![
                ("partNumber".into(), part.to_string()),
                ("uploadId".into(), upload.into()),
            ],
            vec![],
            data,
        )?;
        header(&r, "etag")
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))
    }
    pub fn abort_upload(&self, name: &str, upload: &str) -> Result<()> {
        match self.request(
            "DELETE",
            name,
            vec![("uploadId".into(), upload.into())],
            vec![],
            vec![],
        ) {
            Ok(_) => Ok(()),
            Err(e)
                if e.storage_error().is_some_and(|error| {
                    error.failure() == Failure::Rejected(Rejection::NotFound)
                }) =>
            {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
    pub fn complete_upload(
        &self,
        name: &str,
        upload: &str,
        parts: &[String],
        target_etag: Option<&str>,
    ) -> Result<()> {
        let context = &self.context;
        let mut body = String::from("<CompleteMultipartUpload>");
        for (index, etag) in parts.iter().enumerate() {
            body.push_str(&format!(
                "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
                index + 1,
                quick_xml::escape::escape(etag)
            ));
        }
        body.push_str("</CompleteMultipartUpload>");
        let headers = vec![
            ("content-type".into(), "application/xml".into()),
            write_condition(target_etag),
        ];
        let r = self.request(
            "POST",
            name,
            vec![("uploadId".into(), upload.into())],
            headers,
            body.into_bytes(),
        )?;
        let body = xml(context, r.body())?;
        let mut document = quick_xml::Reader::from_str(body);
        document.config_mut().trim_text(true);
        loop {
            use quick_xml::events::Event;
            match document.read_event() {
                Ok(Event::Decl(_) | Event::Comment(_)) => {}
                Ok(Event::Start(event))
                    if event.local_name().as_ref() == b"CompleteMultipartUploadResult" =>
                {
                    break;
                }
                Ok(Event::Start(event)) if event.local_name().as_ref() == b"Error" => {
                    return Err(context.failure(Failure::Backend(BackendFailure::Other)));
                }
                _ => return Err(context.failure(Failure::Backend(BackendFailure::CorruptData))),
            }
        }
        let complete: Completed = quick_xml::de::from_str(body)
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
        if complete.etag.is_empty() {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct Completed {
    #[serde(rename = "ETag")]
    etag: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Initiated {
    upload_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Listing {
    #[serde(default)]
    contents: Vec<serde::de::IgnoredAny>,
    #[serde(default)]
    common_prefixes: Vec<serde::de::IgnoredAny>,
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

pub(super) fn write_condition(etag: Option<&str>) -> (String, String) {
    match etag {
        Some(etag) => ("if-match".into(), etag.into()),
        None => ("if-none-match".into(), "*".into()),
    }
}
