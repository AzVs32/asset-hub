//! Synchronous Rust HTTP client over standard WASIp2 imports.
//! The host remains asynchronous and authoritatively enforces egress permissions.
use http::{Request, Response};
use vfs_plugin_protocol::{Failure, FailureCode};
#[cfg(not(target_os = "wasi"))]
pub fn send(_request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, Failure> {
    Err(Failure::new(FailureCode::UnsupportedOperation))
}
#[cfg(target_os = "wasi")]
pub fn send(request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, Failure> {
    use wasip2::{
        http::{outgoing_handler, types::*},
        io::streams::StreamError,
    };
    fn fail() -> Failure {
        Failure::new(FailureCode::InvalidRequest)
    }
    fn failed() -> Failure {
        Failure::new(FailureCode::Unavailable)
    }
    fn code(error: ErrorCode) -> Failure {
        Failure::new(match error {
            ErrorCode::HttpRequestDenied | ErrorCode::DestinationIpProhibited => {
                FailureCode::PermissionDenied
            }
            ErrorCode::HttpRequestBodySize(_)
            | ErrorCode::HttpResponseBodySize(_)
            | ErrorCode::HttpRequestHeaderSectionSize(_)
            | ErrorCode::HttpResponseHeaderSectionSize(_) => FailureCode::TooLarge,
            ErrorCode::ConnectionTimeout
            | ErrorCode::ConnectionReadTimeout
            | ErrorCode::ConnectionWriteTimeout
            | ErrorCode::DnsTimeout
            | ErrorCode::HttpResponseTimeout => FailureCode::TimedOut,
            _ => FailureCode::Unavailable,
        })
    }
    fn stream(error: StreamError) -> Failure {
        match error {
            StreamError::LastOperationFailed(error) => http_error_code(&error)
                .map(code)
                .unwrap_or_else(|| Failure::new(FailureCode::Unavailable)),
            StreamError::Closed => Failure::new(FailureCode::Unavailable),
        }
    }
    let (parts, body) = request.into_parts();
    let headers = Fields::from_list(
        &parts
            .headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec()))
            .collect::<Vec<_>>(),
    )
    .map_err(|_| fail())?;
    let outgoing = OutgoingRequest::new(headers);
    let method = match parts.method.as_str() {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "CONNECT" => Method::Connect,
        "OPTIONS" => Method::Options,
        "TRACE" => Method::Trace,
        "PATCH" => Method::Patch,
        method => Method::Other(method.into()),
    };
    outgoing.set_method(&method).map_err(|_| fail())?;
    outgoing
        .set_scheme(Some(&match parts.uri.scheme_str() {
            Some("http") => Scheme::Http,
            Some("https") => Scheme::Https,
            _ => return Err(fail()),
        }))
        .map_err(|_| fail())?;
    outgoing
        .set_authority(parts.uri.authority().map(|authority| authority.as_str()))
        .map_err(|_| fail())?;
    outgoing
        .set_path_with_query(Some(
            parts
                .uri
                .path_and_query()
                .map(|path| path.as_str())
                .unwrap_or("/"),
        ))
        .map_err(|_| fail())?;
    let outgoing_body = outgoing.body().map_err(|_| fail())?;
    let future = outgoing_handler::handle(outgoing, None).map_err(code)?;
    let output = outgoing_body.write().map_err(|_| failed())?;
    for chunk in body.chunks(16 * 1024) {
        output.blocking_write_and_flush(chunk).map_err(stream)?;
    }
    drop(output);
    OutgoingBody::finish(outgoing_body, None).map_err(code)?;
    future.subscribe().block();
    let incoming = future
        .get()
        .ok_or_else(failed)?
        .map_err(|_| failed())?
        .map_err(code)?;
    let mut result = Response::builder().status(incoming.status());
    for (name, value) in incoming.headers().entries() {
        result = result.header(name, value);
    }
    let body = incoming.consume().map_err(|_| failed())?;
    let input = body.stream().map_err(|_| failed())?;
    let mut bytes = Vec::new();
    loop {
        match input.blocking_read(64 * 1024) {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(StreamError::Closed) => break,
            Err(error) => return Err(stream(error)),
        }
    }
    drop(input);
    let trailers = IncomingBody::finish(body);
    trailers.subscribe().block();
    let _ = trailers
        .get()
        .ok_or_else(failed)?
        .map_err(|_| failed())?
        .map_err(code)?;
    result.body(bytes).map_err(|_| failed())
}
