mod auth;
mod dto;
mod error;
mod handlers;
mod openapi;
mod router;
mod settings;
mod state;

pub use router::{build_router, with_authentication};
pub use settings::{CorsPolicy, HttpSettings, RouterOptions, SessionOptions};

/// HTTP action request body limit, exported for host-level integration tests.
pub const MAX_ACTION_REQUEST_BYTES: usize = handlers::MAX_ACTION_REQUEST_BYTES;
/// HTTP login request body limit, exported for host-level integration tests.
pub const MAX_LOGIN_REQUEST_BYTES: usize = auth::MAX_LOGIN_REQUEST_BYTES;
