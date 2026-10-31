//! Small SigV4 S3 client using only the component's HTTP import. Multipart writes
//! bound memory to one part; conditional completion implements create-new.
use super::*;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

#[derive(Clone)]
pub(super) struct Client {
    pub endpoint: String,
    pub bucket: String,
    region: String,
    access: String,
    secret: String,
}
fn text(fields: &p::Object, name: &str) -> Result<String> {
    let value = fields
        .get(name)
        .ok_or_else(|| invalid(name, "missing_field"))?;
    let text = value
        .as_str()
        .ok_or_else(|| invalid(name, "invalid_type"))?;
    if text.is_empty() {
        return Err(invalid(name, "invalid_value"));
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
fn hmac(key: &[u8], bytes: &[u8]) -> Result<Vec<u8>> {
    let mut h = Hmac::<Sha256>::new_from_slice(key).map_err(|_| failure("backend"))?;
    h.update(bytes);
    Ok(h.finalize().into_bytes().to_vec())
}
fn xml(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| failure("invalid_data"))
}
fn header(response: &http::Response, key: &str) -> Option<String> {
    response
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(key))
        .map(|h| h.value.clone())
}
impl Client {
    pub fn parse(fields: &p::Object) -> Result<Self> {
        if fields.keys().any(|k| {
            !["endpoint", "bucket", "region", "access_key", "secret_key"].contains(&k.as_str())
        }) {
            return Err(invalid("", "unknown_field"));
        }
        let endpoint = text(fields, "endpoint")?;
        let url = url::Url::parse(&endpoint).map_err(|_| invalid("endpoint", "invalid_value"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid("endpoint", "invalid_value"));
        }
        let bucket = text(fields, "bucket")?;
        if !(3..=63).contains(&bucket.len())
            || !bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
        {
            return Err(invalid("bucket", "invalid_value"));
        }
        let region = text(fields, "region")?;
        if region.len() > 64
            || !region
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(invalid("region", "invalid_value"));
        }
        Ok(Self {
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
    ) -> Result<http::Response> {
        let request = self.sign(method, key, query, extra, body, clock::unix_time())?;
        let mutation = !matches!(method, "GET" | "HEAD");
        let response = http::request_http(&request).map_err(|code| {
            failure(
                if mutation && matches!(code.as_str(), "timed_out" | "unavailable") {
                    "outcome_unknown"
                } else {
                    &code
                },
            )
        })?;
        match response.status {
            200..=299 => Ok(response),
            403 => Err(failure("permission_denied")),
            404 => Err(failure("not_found")),
            409 => Err(failure("conflict")),
            412 => Err(failure("already_exists")),
            429 => Err(failure("rate_limited")),
            500..=599 => Err(failure(if mutation {
                "outcome_unknown"
            } else {
                "unavailable"
            })),
            _ => Err(failure("backend")),
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
    ) -> Result<http::Request> {
        let time = OffsetDateTime::from_unix_timestamp(now).map_err(|_| failure("invalid_data"))?;
        let format = time::format_description::parse_borrowed::<2>(
            "[year][month][day]T[hour][minute][second]Z",
        )
        .map_err(|_| failure("backend"))?;
        let timestamp = time.format(&format).map_err(|_| failure("backend"))?;
        let date = &timestamp[..8];
        let url = url::Url::parse(&self.endpoint).map_err(|_| failure("invalid_data"))?;
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
        let k = hmac(format!("AWS4{}", self.secret).as_bytes(), date.as_bytes())?;
        let k = hmac(&k, self.region.as_bytes())?;
        let k = hmac(&k, b"s3")?;
        let k = hmac(&k, b"aws4_request")?;
        let signature = hex::encode(hmac(&k, to_sign.as_bytes())?);
        headers.remove("host");
        headers.insert("authorization".into(),format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}",self.access));
        Ok(http::Request {
            method: method.into(),
            url: format!(
                "{}{uri}{}",
                self.endpoint,
                if query.is_empty() {
                    String::new()
                } else {
                    format!("?{query}")
                }
            ),
            headers: headers
                .into_iter()
                .map(|(name, value)| http::Header { name, value })
                .collect(),
            body,
        })
    }
    pub fn check_bucket(&self) -> Result<()> {
        self.request("HEAD", "", vec![], vec![], vec![])?;
        Ok(())
    }
    pub fn head(&self, name: &str) -> Result<Option<p::Entry>> {
        path(name)?;
        let response = match self.request("HEAD", name, vec![], vec![], vec![]) {
            Ok(r) => r,
            Err(e) if e.code == "not_found" => return Ok(None),
            Err(e) => return Err(e),
        };
        let size = header(&response, "content-length")
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| failure("invalid_data"))?;
        let modified = header(&response, "last-modified")
            .and_then(|s| {
                OffsetDateTime::parse(&s, &time::format_description::well_known::Rfc2822).ok()
            })
            .map(|t| p::Timestamp {
                seconds: t.unix_timestamp(),
                nanos: t.nanosecond(),
            });
        Ok(Some(p::Entry {
            path: name.into(),
            kind: p::EntryKind::File,
            size: Some(size),
            modified,
        }))
    }
    fn listing(
        &self,
        prefix: &str,
        after: Option<&str>,
        limit: u32,
        token: Option<&str>,
    ) -> Result<Listing> {
        let mut query = vec![
            ("list-type".into(), "2".into()),
            ("delimiter".into(), "/".into()),
            ("prefix".into(), prefix.into()),
            ("max-keys".into(), limit.to_string()),
        ];
        if let Some(token) = token {
            query.push(("continuation-token".into(), token.into()));
        } else if let Some(after) = after {
            query.push(("start-after".into(), format!("{prefix}{after}")));
        }
        let r = self.request("GET", "", query, vec![], vec![])?;
        quick_xml::de::from_str(xml(&r.body)?).map_err(|_| failure("invalid_data"))
    }
    pub fn stat(&self, name: &str) -> Result<p::Entry> {
        path(name)?;
        if name.is_empty() {
            return Ok(p::Entry {
                path: String::new(),
                kind: p::EntryKind::Dir,
                size: None,
                modified: None,
            });
        }
        let file = self.head(name)?;
        let prefix = format!("{name}/");
        let list = self.listing(&prefix, None, 1, None)?;
        let directory = !list.contents.is_empty() || !list.common_prefixes.is_empty();
        match (file, directory) {
            (Some(_), true) => Err(failure("invalid_data")),
            (Some(file), false) => Ok(file),
            (None, true) => Ok(p::Entry {
                path: name.into(),
                kind: p::EntryKind::Dir,
                size: None,
                modified: None,
            }),
            (None, false) => Err(failure("not_found")),
        }
    }
    pub fn list(
        &self,
        name: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<p::Entry>, Option<String>)> {
        if limit == 0 || limit > 1000 {
            return Err(failure("invalid_request"));
        }
        path(name)?;
        if let Some(after) = after {
            if after.is_empty() || after.contains('/') {
                return Err(failure("invalid_path"));
            }
            path(after)?;
        }
        if self.stat(name)?.kind != p::EntryKind::Dir {
            return Err(failure("not_dir"));
        }
        let prefix = if name.is_empty() {
            String::new()
        } else {
            format!("{name}/")
        };
        let mut entries = BTreeMap::new();
        let mut token = None;
        loop {
            let list = self.listing(&prefix, after, (limit + 2).min(1000), token.as_deref())?;
            for content in list.contents {
                if content.key == prefix {
                    continue;
                }
                let child = content
                    .key
                    .strip_prefix(&prefix)
                    .ok_or_else(|| failure("invalid_data"))?;
                if child.is_empty() || child.contains('/') {
                    return Err(failure("invalid_data"));
                }
                path(&content.key)?;
                if after.is_some_and(|a| child <= a) {
                    continue;
                }
                let entry = p::Entry {
                    path: content.key.clone(),
                    kind: p::EntryKind::File,
                    size: Some(content.size),
                    modified: None,
                };
                if entries.insert(child.to_owned(), entry).is_some() {
                    return Err(failure("invalid_data"));
                }
            }
            for common in list.common_prefixes {
                let key = common
                    .prefix
                    .strip_suffix('/')
                    .ok_or_else(|| failure("invalid_data"))?;
                let child = key
                    .strip_prefix(&prefix)
                    .ok_or_else(|| failure("invalid_data"))?;
                if child.is_empty() || child.contains('/') {
                    return Err(failure("invalid_data"));
                }
                path(key)?;
                if after.is_some_and(|a| child <= a) {
                    continue;
                }
                let entry = p::Entry {
                    path: key.into(),
                    kind: p::EntryKind::Dir,
                    size: None,
                    modified: None,
                };
                if entries.insert(child.into(), entry).is_some() {
                    return Err(failure("invalid_data"));
                }
            }
            if entries.len() > limit as usize || !list.is_truncated {
                break;
            }
            let next = list
                .next_continuation_token
                .ok_or_else(|| failure("invalid_data"))?;
            if token.as_ref() == Some(&next) {
                return Err(failure("invalid_data"));
            }
            token = Some(next);
        }
        let more = entries.len() > limit as usize;
        let entries: Vec<_> = entries.into_values().take(limit as usize).collect();
        let next = if more {
            entries
                .last()
                .and_then(|e| e.path.rsplit('/').next())
                .map(str::to_owned)
        } else {
            None
        };
        Ok((entries, next))
    }
    pub fn start_upload(&self, name: &str) -> Result<String> {
        let r = self.request(
            "POST",
            name,
            vec![("uploads".into(), String::new())],
            vec![],
            vec![],
        )?;
        let upload: Initiated =
            quick_xml::de::from_str(xml(&r.body)?).map_err(|_| failure("invalid_data"))?;
        if upload.upload_id.is_empty() {
            return Err(failure("invalid_data"));
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
        header(&r, "etag").ok_or_else(|| failure("invalid_data"))
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
            Err(e) if e.code == "not_found" => Ok(()),
            Err(e) => Err(e),
        }
    }
    pub fn complete_upload(
        &self,
        name: &str,
        upload: &str,
        parts: &[String],
        replace: bool,
    ) -> Result<()> {
        let mut body = String::from("<CompleteMultipartUpload>");
        for (index, etag) in parts.iter().enumerate() {
            body.push_str(&format!(
                "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
                index + 1,
                quick_xml::escape::escape(etag)
            ));
        }
        body.push_str("</CompleteMultipartUpload>");
        let mut headers = vec![("content-type".into(), "application/xml".into())];
        if !replace {
            headers.push(("if-none-match".into(), "*".into()));
        }
        let r = self.request(
            "POST",
            name,
            vec![("uploadId".into(), upload.into())],
            headers,
            body.into_bytes(),
        )?;
        if xml(&r.body)?.contains("<Error>") {
            return Err(failure("backend"));
        }
        Ok(())
    }
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
    contents: Vec<Content>,
    #[serde(default)]
    common_prefixes: Vec<CommonPrefix>,
    #[serde(default)]
    is_truncated: bool,
    next_continuation_token: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Content {
    key: String,
    size: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CommonPrefix {
    prefix: String,
}
