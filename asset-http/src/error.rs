use crate::dto::{ErrorResponse, PluginDiagnosticResponse};
use asset_core::CoreError;
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// HTTP 层统一错误类型。
///
/// handler 内部统一返回该类型，由 `IntoResponse` 转换成 JSON 错误响应。
#[derive(Debug)]
pub(crate) struct HttpError {
    status: StatusCode,
    message: String,
    diagnostic: Option<Box<HttpDiagnostic>>,
    diagnostics: Vec<asset_plugin_api::protocol::PluginDiagnostic>,
}

#[derive(Debug)]
struct HttpDiagnostic {
    code: String,
    retryable: bool,
    details: Option<serde_json::Value>,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for HttpError {}

impl HttpError {
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
            diagnostic: None,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
            diagnostic: None,
            diagnostics: Vec::new(),
        }
    }
    /// 构造 400 Bad Request。
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            diagnostic: None,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: message.into(),
            diagnostic: None,
            diagnostics: Vec::new(),
        }
    }

    /// 构造 404 Not Found。
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            diagnostic: None,
            diagnostics: Vec::new(),
        }
    }

    /// 构造 403 Forbidden。
    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
            diagnostic: None,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn too_many_requests(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: message.into(),
            diagnostic: None,
            diagnostics: Vec::new(),
        }
    }
}

impl From<CoreError> for HttpError {
    fn from(error: CoreError) -> Self {
        let status = match &error {
            CoreError::Directory(_)
            | CoreError::Resource(_)
            | CoreError::User(_)
            | CoreError::Unsupported { .. }
            | CoreError::InvalidOperation { .. } => StatusCode::BAD_REQUEST,
            CoreError::Unauthenticated => StatusCode::UNAUTHORIZED,
            CoreError::Forbidden { .. } => StatusCode::FORBIDDEN,
            CoreError::NotFound { .. } => StatusCode::NOT_FOUND,
            CoreError::Conflict { .. } => StatusCode::CONFLICT,
            CoreError::LimitExceeded { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            CoreError::Plugin { diagnostic, .. } => plugin_status(&diagnostic.code),
            CoreError::Storage { .. }
            | CoreError::Repository { .. }
            | CoreError::Configuration { .. }
            | CoreError::InvariantViolation { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let diagnostic = match &error {
            CoreError::Plugin { diagnostic, .. } => Some(Box::new(HttpDiagnostic {
                code: diagnostic.code.clone(),
                retryable: diagnostic.retryable,
                details: diagnostic.details.clone(),
            })),
            _ => None,
        };
        let diagnostics = match &error {
            CoreError::Plugin { diagnostics, .. } => diagnostics.clone(),
            _ => Vec::new(),
        };
        Self {
            status,
            message: error.to_string(),
            diagnostic,
            diagnostics,
        }
    }
}

fn plugin_status(code: &str) -> StatusCode {
    use asset_plugin_api::protocol::diagnostic::codes;
    match code {
        codes::INVALID_INPUT | codes::CONTENT_RANGE_INVALID => StatusCode::BAD_REQUEST,
        codes::PERMISSION_DENIED => StatusCode::FORBIDDEN,
        codes::CONTENT_LIMIT_EXCEEDED
        | codes::INPUT_LIMIT_EXCEEDED
        | codes::OUTPUT_LIMIT_EXCEEDED => StatusCode::PAYLOAD_TOO_LARGE,
        codes::TIMEOUT => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl From<asset_core::ResourceError> for HttpError {
    fn from(error: asset_core::ResourceError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
            diagnostic: None,
            diagnostics: Vec::new(),
        }
    }
}

impl From<asset_core::DirectoryError> for HttpError {
    fn from(error: asset_core::DirectoryError) -> Self {
        Self::bad_request(error.to_string())
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (code, retryable, details) = self.diagnostic.map_or((None, None, None), |value| {
            (Some(value.code), Some(value.retryable), value.details)
        });
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
                code,
                retryable,
                details,
                diagnostics: self
                    .diagnostics
                    .iter()
                    .map(PluginDiagnosticResponse::from)
                    .collect(),
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests;
