use crate::dto::ErrorResponse;
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
    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
            diagnostic: Some(Box::new(HttpDiagnostic {
                code: "archive.unavailable".to_string(),
                retryable: true,
                details: None,
            })),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
            diagnostic: None,
        }
    }
    /// 构造 400 Bad Request。
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            diagnostic: None,
        }
    }

    pub(crate) fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: message.into(),
            diagnostic: None,
        }
    }

    /// 构造 404 Not Found。
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            diagnostic: None,
        }
    }
}

impl From<CoreError> for HttpError {
    fn from(error: CoreError) -> Self {
        let status = match &error {
            CoreError::Directory(_)
            | CoreError::Resource(_)
            | CoreError::Idempotency(_)
            | CoreError::StorageValue(_)
            | CoreError::Unsupported { .. }
            | CoreError::InvalidOperation { .. } => StatusCode::BAD_REQUEST,
            CoreError::NotFound { .. } => StatusCode::NOT_FOUND,
            CoreError::Conflict { .. }
            | CoreError::RevisionConflict { .. }
            | CoreError::LostIdempotencyLease { .. } => StatusCode::CONFLICT,
            CoreError::LimitExceeded { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            CoreError::Storage { .. }
            | CoreError::Repository { .. }
            | CoreError::Configuration { .. }
            | CoreError::InvariantViolation { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let diagnostic = match &error {
            CoreError::RevisionConflict { .. } => Some(Box::new(HttpDiagnostic {
                code: "concurrency.revision_conflict".to_string(),
                retryable: true,
                details: None,
            })),
            _ => None,
        };
        Self {
            status,
            message: error.to_string(),
            diagnostic,
        }
    }
}

impl From<asset_core::ResourceError> for HttpError {
    fn from(error: asset_core::ResourceError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
            diagnostic: None,
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
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests;
