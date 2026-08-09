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

/// 在目录工作区边界内执行的具体业务操作。
///
/// 当前授权策略只判断目标是否位于用户工作区子树；操作类型用于保留调用意图并生成
/// 准确的拒绝信息，不表示可配置的权限等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryOperation {
    ViewDirectory,
    DownloadDirectory,
    CreateDirectory,
    UpdateDirectory,
    DeleteDirectory,
    ReadResource,
    UpdateResource,
    ReplaceResourceContent,
    ExecuteDirectoryAction,
    ExecuteResourceAction,
    DeleteResource,
    PurgeResource,
}

impl DirectoryOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ViewDirectory => "view directory",
            Self::DownloadDirectory => "download directory",
            Self::CreateDirectory => "create directory",
            Self::UpdateDirectory => "update directory",
            Self::DeleteDirectory => "delete directory",
            Self::ReadResource => "read resource",
            Self::UpdateResource => "update resource",
            Self::ReplaceResourceContent => "replace resource content",
            Self::ExecuteDirectoryAction => "execute directory action",
            Self::ExecuteResourceAction => "execute resource action",
            Self::DeleteResource => "delete resource",
            Self::PurgeResource => "purge resource",
        }
    }
}
