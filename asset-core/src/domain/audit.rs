use super::UserId;
use chrono::{DateTime, Utc};

/// 安全审计事件的业务类型。
///
/// 事件类型只描述发生了什么，与 HTTP、CLI 等调用来源无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityAuditEventType {
    AuthLogin,
    AuthLogout,
    AuthUserCreate,
    AuthUserPassword,
    AuthUserStatus,
    ResourcePurge,
    ResourceSoftDelete,
    ResourceAction,
    ResourceUpload,
    ResourceCreate,
    ResourceUpdate,
    /// 显式完整扫描对象存储并协调资源记录。
    ResourceScan,
    DirectoryCreate,
}

impl SecurityAuditEventType {
    /// 返回用于持久化和跨应用交换的稳定事件名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthLogin => "auth.login",
            Self::AuthLogout => "auth.logout",
            Self::AuthUserCreate => "auth.user.create",
            Self::AuthUserPassword => "auth.user.password",
            Self::AuthUserStatus => "auth.user.status",
            Self::ResourcePurge => "resource.purge",
            Self::ResourceSoftDelete => "resource.soft_delete",
            Self::ResourceAction => "resource.action",
            Self::ResourceUpload => "resource.upload",
            Self::ResourceCreate => "resource.create",
            Self::ResourceUpdate => "resource.update",
            Self::ResourceScan => "resource.scan",
            Self::DirectoryCreate => "directory.create",
        }
    }

    /// 从持久化名称恢复领域类型。
    pub fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "auth.login" => Some(Self::AuthLogin),
            "auth.logout" => Some(Self::AuthLogout),
            "auth.user.create" => Some(Self::AuthUserCreate),
            "auth.user.password" => Some(Self::AuthUserPassword),
            "auth.user.status" => Some(Self::AuthUserStatus),
            "resource.purge" => Some(Self::ResourcePurge),
            "resource.soft_delete" => Some(Self::ResourceSoftDelete),
            "resource.action" => Some(Self::ResourceAction),
            "resource.upload" => Some(Self::ResourceUpload),
            "resource.create" => Some(Self::ResourceCreate),
            "resource.update" => Some(Self::ResourceUpdate),
            "resource.scan" => Some(Self::ResourceScan),
            "directory.create" => Some(Self::DirectoryCreate),
            _ => None,
        }
    }
}

/// 触发安全审计事件的应用入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityAuditSource {
    Http,
    Cli,
}

impl SecurityAuditSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Cli => "cli",
        }
    }

    pub fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "http" => Some(Self::Http),
            "cli" => Some(Self::Cli),
            _ => None,
        }
    }
}

/// 安全审计事件的执行结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityAuditOutcome {
    Success,
    Failure,
}

impl SecurityAuditOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }

    pub fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            _ => None,
        }
    }
}

/// 已验证的 Asset Hub 操作者；未登录操作不会伪造用户身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityAuditActor {
    Authenticated(UserId),
    Unauthenticated,
}

impl SecurityAuditActor {
    pub const fn authenticated(user_id: UserId) -> Self {
        Self::Authenticated(user_id)
    }

    pub const fn unauthenticated() -> Self {
        Self::Unauthenticated
    }

    pub const fn user_id(&self) -> Option<UserId> {
        match self {
            Self::Authenticated(user_id) => Some(*user_id),
            Self::Unauthenticated => None,
        }
    }
}

/// 等待写入仓储的安全审计事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSecurityAuditEvent {
    pub actor: SecurityAuditActor,
    pub source: SecurityAuditSource,
    pub event_type: SecurityAuditEventType,
    pub outcome: SecurityAuditOutcome,
    pub target: Option<String>,
}

/// 从仓储读取的安全审计事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityAuditEvent {
    pub id: i64,
    pub occurred_at: DateTime<Utc>,
    pub actor: SecurityAuditActor,
    pub source: SecurityAuditSource,
    pub event_type: SecurityAuditEventType,
    pub outcome: SecurityAuditOutcome,
    pub target: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_types_roundtrip_through_stable_storage_names() {
        let event_types = [
            SecurityAuditEventType::AuthLogin,
            SecurityAuditEventType::AuthLogout,
            SecurityAuditEventType::AuthUserCreate,
            SecurityAuditEventType::AuthUserPassword,
            SecurityAuditEventType::AuthUserStatus,
            SecurityAuditEventType::ResourcePurge,
            SecurityAuditEventType::ResourceSoftDelete,
            SecurityAuditEventType::ResourceAction,
            SecurityAuditEventType::ResourceUpload,
            SecurityAuditEventType::ResourceCreate,
            SecurityAuditEventType::ResourceUpdate,
            SecurityAuditEventType::ResourceScan,
            SecurityAuditEventType::DirectoryCreate,
        ];

        for event_type in event_types {
            assert_eq!(
                SecurityAuditEventType::from_stable_str(event_type.as_str()),
                Some(event_type)
            );
        }
    }

    #[test]
    fn source_and_outcome_roundtrip_through_stable_storage_names() {
        for source in [SecurityAuditSource::Http, SecurityAuditSource::Cli] {
            assert_eq!(
                SecurityAuditSource::from_stable_str(source.as_str()),
                Some(source)
            );
        }
        for outcome in [SecurityAuditOutcome::Success, SecurityAuditOutcome::Failure] {
            assert_eq!(
                SecurityAuditOutcome::from_stable_str(outcome.as_str()),
                Some(outcome)
            );
        }
    }
}
