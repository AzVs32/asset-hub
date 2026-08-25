//! Plugin API 中通过调用期引用读取 Resource 内容的 Wasm Host functions。
//!
//! 本模块定义 Host function 名称和范围值对象。引用既可由 Resource Action 请求提供，也可由
//! Directory Action 的资源分页结果提供；相应 JSON 内容引用定义在协议层。两层统一由
//! [`crate::protocol::PLUGIN_API_VERSION`] 版本化。

use super::contract::{CONTENT_CLOSE, CONTENT_OPEN, CONTENT_READ, CONTENT_SIZE};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

pub const CONTENT_OPEN_FN: &str = CONTENT_OPEN.name;
pub const CONTENT_SIZE_FN: &str = CONTENT_SIZE.name;
pub const CONTENT_READ_FN: &str = CONTENT_READ.name;
pub const CONTENT_CLOSE_FN: &str = CONTENT_CLOSE.name;

/// A validated half-open byte range `[offset, offset + length)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContentRange {
    offset: u64,
    length: u64,
}

impl ContentRange {
    pub fn new(offset: u64, length: u64) -> Result<Self, ContentRangeError> {
        offset
            .checked_add(length)
            .ok_or(ContentRangeError::EndOverflow)?;
        Ok(Self { offset, length })
    }

    pub fn end(self) -> u64 {
        self.offset
            .checked_add(self.length)
            .expect("ContentRange construction validates its end")
    }

    pub fn offset(self) -> u64 {
        self.offset
    }

    pub fn length(self) -> u64 {
        self.length
    }

    /// Restricts the requested length to the content boundary and the Host read limit.
    pub fn constrain_to(
        self,
        content_size: u64,
        max_read_length: u64,
    ) -> Result<Self, ContentRangeError> {
        if self.offset > content_size {
            return Err(ContentRangeError::StartOutOfBounds);
        }
        Self::new(
            self.offset,
            self.length
                .min(max_read_length)
                .min(content_size - self.offset),
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentRangeDocument {
    offset: u64,
    length: u64,
}

impl<'de> Deserialize<'de> for ContentRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = ContentRangeDocument::deserialize(deserializer)?;
        Self::new(document.offset, document.length).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRangeError {
    EndOverflow,
    StartOutOfBounds,
}

impl std::fmt::Display for ContentRangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EndOverflow => "content range end overflows u64",
            Self::StartOutOfBounds => "content range starts beyond the content size",
        })
    }
}

impl std::error::Error for ContentRangeError {}

#[cfg(test)]
mod tests;
