//! WASI HTTP egress hooks. No guest request bypasses origin, DNS or size checks.
use super::grants::{HttpGrants, public_ip};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use std::{future::Future, net::IpAddr, time::Duration};
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, WasiHttpHooks};
type Completion = Box<dyn Future<Output = Result<(), Error>> + Send>;
type ResponseFuture =
    Box<dyn Future<Output = Result<(http::Response<WasiBody>, Completion), Error>> + Send>;
#[derive(Clone)]
pub(crate) struct HttpPolicy {
    pub grants: HttpGrants,
}
impl WasiHttpHooks for HttpPolicy {
    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        _options: Option<RequestOptions>,
        _completion: Completion,
    ) -> ResponseFuture {
        let policy = self.clone();
        Box::new(async move {
            tokio::time::timeout(
                Duration::from_millis(policy.grants.timeout_ms),
                policy.send(request),
            )
            .await
            .map_err(|_| Error::ConnectionTimeout)?
        })
    }
}
impl HttpPolicy {
    async fn send(
        &self,
        request: http::Request<WasiBody>,
    ) -> Result<(http::Response<WasiBody>, Completion), Error> {
        let (parts, body) = request.into_parts();
        if parts.headers.len() > self.grants.max_headers {
            return Err(Error::HttpRequestHeaderSectionSize(None));
        }
        let mut url =
            url::Url::parse(&parts.uri.to_string()).map_err(|_| Error::HttpRequestUriInvalid)?;
        let mut body = Limited::new(body, self.grants.max_request_bytes)
            .collect()
            .await
            .map_err(|_| Error::HttpRequestBodySize(None))?
            .to_bytes();
        let mut method = parts.method;
        let mut headers = parts.headers;
        for attempt in 0..=5 {
            if !url.username().is_empty()
                || url.password().is_some()
                || !self
                    .grants
                    .origins
                    .contains(&url.origin().ascii_serialization())
            {
                return Err(Error::HttpRequestDenied);
            }
            let host = url
                .host_str()
                .ok_or(Error::HttpRequestUriInvalid)?
                .trim_matches(['[', ']']);
            let port = url
                .port_or_known_default()
                .ok_or(Error::HttpRequestUriInvalid)?;
            let literal = host.parse::<IpAddr>().ok();
            let addresses = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| Error::DestinationNotFound)?
                .collect::<Vec<_>>();
            if addresses.is_empty()
                || addresses.iter().any(|addr| {
                    literal.is_none()
                        && !public_ip(addr.ip())
                        && !self
                            .grants
                            .private_addresses
                            .iter()
                            .any(|ip| ip.to_canonical() == addr.ip().to_canonical())
                })
            {
                return Err(Error::DestinationIpProhibited);
            }
            // Pin this request's actual connector to the addresses just checked.
            let client = reqwest::Client::builder()
                .no_proxy()
                .resolve_to_addrs(host, &addresses)
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .pool_max_idle_per_host(0)
                .build()
                .map_err(|_| Error::ConfigurationError)?;
            let mut outgoing = headers.clone();
            outgoing.remove(http::header::HOST);
            outgoing.remove(http::header::CONTENT_LENGTH);
            let mut response = client
                .request(method.clone(), url.as_str())
                .headers(outgoing)
                .body(body.clone())
                .send()
                .await
                .map_err(|error| {
                    if error.is_timeout() {
                        Error::ConnectionTimeout
                    } else {
                        Error::DestinationUnavailable
                    }
                })?;
            if response.headers().len() > self.grants.max_headers {
                return Err(Error::HttpResponseHeaderSectionSize(None));
            }
            if response.status().is_redirection()
                && response.headers().contains_key(http::header::LOCATION)
            {
                if !self.grants.allow_redirects {
                    return Err(Error::HttpRequestDenied);
                }
                if attempt == 5 {
                    return Err(Error::LoopDetected);
                }
                let next = url
                    .join(
                        response
                            .headers()
                            .get(http::header::LOCATION)
                            .and_then(|value| value.to_str().ok())
                            .ok_or(Error::HttpProtocolError)?,
                    )
                    .map_err(|_| Error::HttpProtocolError)?;
                if next.origin() != url.origin() {
                    headers.remove(http::header::AUTHORIZATION);
                    headers.remove(http::header::COOKIE);
                }
                if response.status() == http::StatusCode::SEE_OTHER {
                    method = http::Method::GET;
                    body = Bytes::new();
                    headers.remove(http::header::CONTENT_TYPE);
                }
                url = next;
                continue;
            }
            if method != http::Method::HEAD
                && response
                    .content_length()
                    .is_some_and(|length| length > self.grants.max_response_bytes as u64)
            {
                return Err(Error::HttpResponseBodySize(None));
            }
            let mut result = http::Response::builder().status(response.status());
            *result.headers_mut().ok_or(Error::HttpProtocolError)? = response.headers().clone();
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::HttpResponseIncomplete)?
            {
                if chunk.len() > self.grants.max_response_bytes.saturating_sub(bytes.len()) {
                    return Err(Error::HttpResponseBodySize(None));
                }
                bytes.extend_from_slice(&chunk);
            }
            let body = Full::new(Bytes::from(bytes))
                .map_err(Error::from)
                .boxed_unsync();
            return Ok((
                result.body(body).map_err(|_| Error::HttpProtocolError)?,
                Box::new(async { Ok(()) }),
            ));
        }
        Err(Error::LoopDetected)
    }
}
