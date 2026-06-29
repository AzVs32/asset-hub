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

impl HttpError {
    /// 构造 400 Bad Request。
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
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
}

impl From<CoreError> for HttpError {
    fn from(error: CoreError) -> Self {
        let status = match &error {
            CoreError::Resource(_) | CoreError::Configuration { .. } => StatusCode::BAD_REQUEST,
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
