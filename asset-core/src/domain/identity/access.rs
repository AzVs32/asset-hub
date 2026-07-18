use super::UserId;

/// 已通过外部入口认证的访问主体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessContext {
    user_id: UserId,
    administrator: bool,
}

impl AccessContext {
    pub fn member(user_id: UserId) -> Self {
        Self {
            user_id,
            administrator: false,
        }
    }
    pub fn administrator(user_id: UserId) -> Self {
        Self {
            user_id,
            administrator: true,
        }
    }
    pub fn user_id(&self) -> UserId {
        self.user_id
    }
    pub fn is_administrator(&self) -> bool {
        self.administrator
    }
}

/// 资源用例要求的访问级别，用于生成一致的拒绝信息。
///
/// 普通用户在自己的工作区子树内满足全部级别；该枚举不再表示可配置的目录授权。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryPermission {
    Read,
    Write,
    Full,
}
