//! Plugin API 中的 Resource 内容读取 Wasm Host functions。
//!
//! 本模块定义 Host function 名称和范围值对象。Resource Action 的 JSON 内容引用定义在协议层；二者统一由
//! [`crate::protocol::PLUGIN_API_VERSION`] 版本化。

use serde::{Deserialize, Deserializer, Serialize};

pub const CONTENT_OPEN_FN: &str = "asset_hub_content_open";
pub const CONTENT_SIZE_FN: &str = "asset_hub_content_size";
pub const CONTENT_READ_RANGE_FN: &str = "asset_hub_content_read";
pub const CONTENT_CLOSE_FN: &str = "asset_hub_content_close";

/// A validated half-open byte range `[offset, offset + length)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginContentRange {
    offset: u64,
    length: u64,
}

impl PluginContentRange {
    pub fn new(offset: u64, length: u64) -> Result<Self, ContentRangeError> {
        offset
            .checked_add(length)
            .ok_or(ContentRangeError::Overflow)?;
        Ok(Self { offset, length })
    }

    pub fn end(self) -> u64 {
        self.offset
            .checked_add(self.length)
            .expect("PluginContentRange construction validates its end")
    }

    pub fn offset(self) -> u64 {
        self.offset
    }

    pub fn length(self) -> u64 {
        self.length
    }

    pub fn bounded(self, size: u64, max_length: u64) -> Result<Self, ContentRangeError> {
        if self.offset > size {
            return Err(ContentRangeError::OutOfBounds);
        }
        Self::new(
            self.offset,
            self.length.min(max_length).min(size - self.offset),
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginContentRangeDocument {
    offset: u64,
    length: u64,
}

impl<'de> Deserialize<'de> for PluginContentRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = PluginContentRangeDocument::deserialize(deserializer)?;
        Self::new(document.offset, document.length).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRangeError {
    Overflow,
    OutOfBounds,
}

impl std::fmt::Display for ContentRangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Overflow => "content range overflows u64",
            Self::OutOfBounds => "content range starts beyond the content size",
        })
    }
}

impl std::error::Error for ContentRangeError {}

#[cfg(test)]
mod tests;
