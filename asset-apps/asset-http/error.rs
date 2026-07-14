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
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
    /// 构造 400 Bad Request。
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub(crate) fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: message.into(),
        }
    }

    /// 构造 404 Not Found。
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    /// 构造 403 Forbidden。
    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    pub(crate) fn too_many_requests(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: message.into(),
        }
    }
}

impl From<CoreError> for HttpError {
    fn from(error: CoreError) -> Self {
        let status = match &error {
            CoreError::Resource(_) | CoreError::User(_) | CoreError::Configuration { .. } => {
                StatusCode::BAD_REQUEST
            }
            CoreError::Unauthenticated => StatusCode::UNAUTHORIZED,
            CoreError::Forbidden { .. } => StatusCode::FORBIDDEN,
            CoreError::NotFound { .. } => StatusCode::NOT_FOUND,
            CoreError::Conflict { .. } => StatusCode::CONFLICT,
            CoreError::Storage { .. } | CoreError::Repository { .. } | CoreError::Plugin { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };

        Self {
            status,
            message: error.to_string(),
        }
    }
}

impl From<asset_core::ResourceError> for HttpError {
    fn from(error: asset_core::ResourceError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}
