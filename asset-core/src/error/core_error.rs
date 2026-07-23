use super::{DirectoryError, ResourceError, UserError};
use asset_plugin_api::{PluginActionFailure, PluginDiagnostic};
use thiserror::Error;

/// 核心层对外暴露的统一错误类型。
///
/// 领域错误会原样透传；基础设施适配器应将底层错误转换为对应的存储或仓储错误，
/// 避免 OpenDAL、sqlx 等具体实现泄漏到核心端口签名中。
#[derive(Error, Debug)]
pub enum CoreError {
    /// 目录领域内的业务校验或树结构约束错误。
    #[error(transparent)]
    Directory(#[from] DirectoryError),

    /// 资源领域内的业务校验或状态流转错误。
    #[error(transparent)]
    Resource(#[from] ResourceError),

    #[error(transparent)]
    User(#[from] UserError),

    #[error("authentication failed")]
    Unauthenticated,

    #[error("access denied for `{action}` on directory `{directory}`")]
    Forbidden {
        action: &'static str,
        directory: String,
    },

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

    /// 插件动作执行失败。
    #[error("plugin `{plugin}` action `{action}` failed: {diagnostic}")]
    Plugin {
        /// 插件标识。
        plugin: String,
        /// 动作标识。
        action: String,
        /// Stable code, message, retry hint and optional machine-readable details.
        diagnostic: Box<PluginDiagnostic>,
        /// Additional diagnostics emitted while producing the primary failure.
        diagnostics: Vec<PluginDiagnostic>,
    },
}

impl CoreError {
    pub fn forbidden(action: &'static str, directory: impl Into<String>) -> Self {
        Self::Forbidden {
            action,
            directory: directory.into(),
        }
    }
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

    /// 创建插件执行错误。
    pub fn plugin(
        plugin: impl Into<String>,
        action: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::plugin_diagnostic(
            plugin,
            action,
            PluginDiagnostic {
                code: asset_plugin_api::diagnostic::codes::RUNTIME_FAILURE.to_string(),
                message: message.into(),
                severity: asset_plugin_api::PluginDiagnosticSeverity::Error,
                retryable: false,
                details: None,
            },
        )
    }

    pub fn plugin_diagnostic(
        plugin: impl Into<String>,
        action: impl Into<String>,
        diagnostic: PluginDiagnostic,
    ) -> Self {
        Self::Plugin {
            plugin: plugin.into(),
            action: action.into(),
            diagnostic: Box::new(diagnostic),
            diagnostics: Vec::new(),
        }
    }

    pub fn plugin_failure(
        plugin: impl Into<String>,
        action: impl Into<String>,
        failure: PluginActionFailure,
    ) -> Self {
        Self::Plugin {
            plugin: plugin.into(),
            action: action.into(),
            diagnostic: Box::new(failure.error),
            diagnostics: failure.diagnostics,
        }
    }
}
