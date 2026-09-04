//! Resource Action 的 JSON 输入协议。
//!
//! 这些 DTO 是 Host 调用插件 Resource Action handler 时传递的线协议，不承担
//! Action 可用性判断、权限决策或持久化等领域职责。

use crate::protocol::{PluginActionAccess, PluginDiagnostic, PluginView};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Action request passed from host to a plugin handler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginResourceActionRequest {
    pub action: String,
    pub access: PluginActionAccess,
    #[serde(default)]
    pub input: Value,
    pub resource: PluginResource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginContentBytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_ref: Option<PluginContentReference>,
}

/// Resource snapshot exposed to plugin handlers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginResource {
    pub id: String,
    pub directory: String,
    pub name: String,
    pub kind: String,
    pub revision: u64,
    pub state: PluginResourceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<PluginResourceContent>,
    pub created_at: String,
    pub updated_at: String,
}

/// Authoritative Resource lifecycle, content, and effective state exposed to plugins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginResourceState {
    pub lifecycle: PluginResourceLifecycleState,
    pub content: PluginResourceContentState,
    pub effective: PluginResourceEffectiveState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginResourceLifecycleState {
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginResourceContentState {
    Absent,
    Pending,
    Verified,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginResourceEffectiveState {
    NoContent,
    Verifying,
    Ready,
    VerificationFailed,
}

/// Resource content reference exposed to plugins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginResourceContent {
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<PluginChecksum>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginChecksum {
    pub kind: String,
    pub value: String,
}

/// Inline object content supplied to a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginContentBytes {
    pub encoding: PluginInlineContentEncoding,
    pub data: String,
}

/// Encoding accepted for content embedded directly in an action request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginInlineContentEncoding {
    Base64,
}

/// Non-inline object content supplied to a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginContentReference {
    pub encoding: PluginContentReferenceEncoding,
    pub reference: String,
}

/// Encoding used by an opaque, call-scoped host content reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginContentReferenceEncoding {
    Handle,
}

/// Complete Resource Action handler result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PluginResourceActionResult {
    Success(PluginResourceActionOutput),
    Failure(crate::protocol::PluginActionFailure),
}

/// Standard Resource Action output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginResourceActionOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<PluginView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<PluginResourceActionEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl PluginResourceActionOutput {
    pub fn new(view: PluginView) -> Self {
        Self {
            view: Some(view),
            effects: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn without_view() -> Self {
        Self {
            view: None,
            effects: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

/// Side effects requested by a Resource Action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginResourceActionEffect {
    ReplaceContent(ReplaceContentEffect),
    Delete,
}

impl PluginResourceActionEffect {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ReplaceContent(_) => "replace_content",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceContentEffect {
    pub encoding: PluginReplacementEncoding,
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Encoding accepted for bytes returned by a content replacement effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginReplacementEncoding {
    Base64,
}

#[cfg(test)]
mod tests {
    use super::{PluginResourceActionEffect, PluginResourceActionOutput};

    #[test]
    fn effect_only_output_omits_view_fields() {
        let mut output = PluginResourceActionOutput::without_view();
        output.effects.push(PluginResourceActionEffect::Delete);

        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({"effects": [{"type": "delete"}]})
        );
    }
}
