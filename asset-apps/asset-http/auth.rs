use crate::audit::SecurityAuditEventResponse;
use crate::error::HttpError;
use asset_core::domain::{
    AccessContext, NewSecurityAuditEvent, ResourceDirectory, SecurityAuditActor,
    SecurityAuditEventType, SecurityAuditOutcome, SecurityAuditSource, User, UserId, UserRole,
    UserStatus,
};
use asset_core::port::SecurityAuditRepository;
use asset_core::service::UserService;
use axum::{
    Json,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_login::{AuthSession, AuthUser, AuthnBackend};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) mod backend;
pub(crate) mod dto;
pub(crate) mod rate_limit;
pub(crate) mod routes;

pub(crate) use backend::{AuthBackend, AuthenticatedUser, Credentials, Session};
pub(crate) use dto::{
    CreateUserRequest, ManagedUserResponse, MeResponse, SecurityAuditQuery, UpdateUserStatusRequest,
};
pub(crate) use rate_limit::MAX_LOGIN_REQUEST_BYTES;
pub(crate) use routes::{
    audit_request, authorize_request, create_user, list_security_audit_events, list_users, login,
    logout, me, update_user_status,
};

use rate_limit::*;

#[cfg(test)]
mod tests;
