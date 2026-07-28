//! 插件 Action 的 JSON 输出与视图协议。
//!
//! 插件通过这里的 DTO 返回可渲染视图、受约束的副作用声明和诊断信息；实际副作用
//! 是否允许以及如何落库仍由 Host 校验和执行。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PluginDiagnostic;

/// Standard action output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginActionOutput {
    #[serde(flatten)]
    pub view: PluginView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<PluginActionEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl PluginActionOutput {
    pub fn new(view: PluginView) -> Self {
        Self {
            view,
            effects: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl PluginView {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Markdown(_) => "markdown",
            Self::Html(_) => "html",
            Self::PluginFrame(_) => "plugin_frame",
            Self::Json(_) => "json",
            Self::Media(_) => "media",
            Self::Download(_) => "download",
        }
    }
}

/// Side effects requested by a plugin action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginActionEffect {
    ReplaceContent(ReplaceContentEffect),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceContentEffect {
    pub encoding: PluginReplacementEncoding,
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Encoding accepted for bytes returned by a content replacement effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginReplacementEncoding {
    Base64,
}

/// Shared view protocol returned by plugin actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum PluginView {
    Text(TextView),
    Markdown(MarkdownView),
    Html(HtmlView),
    PluginFrame(PluginFrameView),
    Json(JsonView),
    Media(MediaView),
    Download(DownloadView),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextView {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownView {
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub html: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginFrameView {
    pub plugin_api: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonView {
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaView {
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub encoding: PluginMediaEncoding,
    pub data: String,
}

/// Encodings renderable by a media view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginMediaEncoding {
    Base64,
    Url,
}

/// Host-owned URL exposed as a downloadable file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadView {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{DownloadView, PluginView};

    #[test]
    fn download_view_has_an_explicit_wire_discriminator() {
        let view = PluginView::Download(DownloadView {
            url: "/resources/resource-1/download".to_string(),
            mime_type: Some("application/octet-stream".to_string()),
            filename: Some("asset.bin".to_string()),
        });

        assert_eq!(view.kind(), "download");
        assert_eq!(
            serde_json::to_value(view).unwrap(),
            serde_json::json!({
                "view": "download",
                "url": "/resources/resource-1/download",
                "mime_type": "application/octet-stream",
                "filename": "asset.bin"
            })
        );
    }
}
