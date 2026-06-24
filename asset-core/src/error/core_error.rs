use super::ResourceError;
use thiserror::Error;

/// 核心层对外暴露的统一错误类型。
///
/// 领域错误会原样透传；基础设施适配器应将底层错误转换为对应的存储或仓储错误，
/// 避免 OpenDAL、sqlx 等具体实现泄漏到核心端口签名中。
#[derive(Error, Debug)]
pub enum CoreError {
    /// 资源领域内的业务校验或状态流转错误。
    #[error(transparent)]
    Resource(#[from] ResourceError),

    /// 对象存储操作失败。
    #[error("storage operation `{operation}` failed: {source}")]
    Storage {
        /// 失败的存储操作名称，例如 `put`、`get`、`delete`。
        operation: &'static str,
        /// 底层存储适配器返回的原始错误。
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// 数据仓储操作失败。
    #[error("repository operation `{operation}` failed: {source}")]
    Repository {
        /// 失败的仓储操作名称，例如 `save`、`find_by_id`、`remove`。
        operation: &'static str,
        /// 底层数据存储适配器返回的原始错误。
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// 调用方期望对象存在，但实际未找到。
    #[error("{entity} not found: {id}")]
    NotFound {
        /// 未找到的对象类型。
        entity: &'static str,
        /// 未找到对象的标识。
        id: String,
    },

    /// 保存或更新时发生状态冲突。
    #[error("conflict: {message}")]
    Conflict {
        /// 冲突原因。
        message: String,
    },

    /// 基础设施配置不合法。
    #[error("invalid configuration: {message}")]
    Configuration {
        /// 配置错误说明。
        message: String,
    },
}

impl CoreError {
    /// 包装对象存储适配器返回的底层错误。
    pub fn storage(
        operation: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Storage {
            operation,
            source: Box::new(source),
        }
    }

    /// 包装数据仓储适配器返回的底层错误。
    pub fn repository(
        operation: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Repository {
            operation,
            source: Box::new(source),
        }
    }

    /// 创建未找到错误。
    pub fn not_found(entity: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            entity,
            id: id.into(),
        }
    }

    /// 创建状态冲突错误。
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            message: message.into(),
        }
    }

    /// 创建基础设施配置错误。
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }
}
