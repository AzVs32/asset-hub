//! Host 与插件共享的诊断和失败响应协议。
//!
//! 诊断码用于跨运行时边界稳定表达错误类别；其中不包含 Host 内部错误类型或日志实现。

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// Machine-readable diagnostic produced by a plugin or by a host execution phase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDiagnostic {
    pub code: String,
    pub message: String,
    #[serde(default = "error_severity")]
    pub severity: PluginDiagnosticSeverity,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl std::fmt::Display for PluginDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "[{}] {}", self.code, self.message)
    }
}

fn error_severity() -> PluginDiagnosticSeverity {
    PluginDiagnosticSeverity::Error
}

/// Error response that API 0.4 plugins may return instead of an action view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginActionFailure {
    pub error: PluginDiagnostic,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl PluginDiagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            severity: PluginDiagnosticSeverity::Error,
            retryable: false,
            details: None,
        }
    }
}

impl PluginActionFailure {
    pub fn new(error: PluginDiagnostic) -> Self {
        Self {
            error,
            diagnostics: Vec::new(),
        }
    }
}

pub mod codes {
    pub const INVALID_INPUT: &str = "plugin.invalid_input";
    pub const PERMISSION_DENIED: &str = "plugin.permission_denied";
    pub const CONTENT_RANGE_INVALID: &str = "plugin.content_range_invalid";
    pub const CONTENT_LIMIT_EXCEEDED: &str = "plugin.content_limit_exceeded";
    pub const INPUT_LIMIT_EXCEEDED: &str = "plugin.input_limit_exceeded";
    pub const OUTPUT_LIMIT_EXCEEDED: &str = "plugin.output_limit_exceeded";
    pub const TIMEOUT: &str = "plugin.timeout";
    pub const RUNTIME_FAILURE: &str = "plugin.runtime_failure";
    pub const INVALID_OUTPUT: &str = "plugin.invalid_output";
    pub const ACTION_FAILED: &str = "plugin.action_failed";
}
