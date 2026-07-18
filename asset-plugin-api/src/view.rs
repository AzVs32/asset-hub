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
            Self::BinaryUrl(_) => "binary_url",
            Self::Table(_) => "table",
            Self::Form(_) => "form",
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
    BinaryUrl(BinaryUrlView),
    Table(TableView),
    Form(FormView),
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
pub struct PluginFrameView {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryUrlView {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableView {
    #[serde(default)]
    pub columns: Vec<TableColumn>,
    #[serde(default)]
    pub rows: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumn {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormView {
    pub schema: Value,
    #[serde(default)]
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit_action: Option<String>,
}
