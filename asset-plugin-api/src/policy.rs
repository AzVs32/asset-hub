//! Host 执行插件时采用的资源限制策略。
//!
//! 该策略不是插件 handler 的线协议，而是 Host 创建执行环境时使用的受校验配置值对象。

use serde::{Deserialize, Serialize};
use std::fmt;

/// Limits applied consistently by the action service and the Wasm host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginExecutionPolicy {
    max_content_bytes: u64,
    max_inline_content_bytes: u64,
    max_content_read_bytes: u64,
    max_input_bytes: usize,
    max_output_bytes: usize,
    max_concurrent_calls: usize,
    memory_max_pages: u32,
    timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidPluginExecutionPolicy(&'static str);

impl fmt::Display for InvalidPluginExecutionPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for InvalidPluginExecutionPolicy {}

impl PluginExecutionPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        max_content_bytes: u64,
        max_inline_content_bytes: u64,
        max_content_read_bytes: u64,
        max_input_bytes: usize,
        max_output_bytes: usize,
        max_concurrent_calls: usize,
        memory_max_pages: u32,
        timeout_seconds: u64,
    ) -> Result<Self, InvalidPluginExecutionPolicy> {
        let policy = Self {
            max_content_bytes,
            max_inline_content_bytes: max_inline_content_bytes.min(max_content_bytes),
            max_content_read_bytes: max_content_read_bytes.min(max_content_bytes),
            max_input_bytes,
            max_output_bytes,
            max_concurrent_calls,
            memory_max_pages,
            timeout_seconds,
        };
        if policy.max_content_bytes == 0
            || policy.max_inline_content_bytes == 0
            || policy.max_content_read_bytes == 0
            || policy.max_input_bytes == 0
            || policy.max_output_bytes == 0
            || policy.max_concurrent_calls == 0
            || policy.memory_max_pages == 0
            || policy.timeout_seconds == 0
        {
            return Err(InvalidPluginExecutionPolicy(
                "plugin execution limits must all be greater than zero",
            ));
        }
        Ok(policy)
    }

    pub fn max_content_bytes(&self) -> u64 {
        self.max_content_bytes
    }
    pub fn max_inline_content_bytes(&self) -> u64 {
        self.max_inline_content_bytes
    }
    pub fn max_content_read_bytes(&self) -> u64 {
        self.max_content_read_bytes
    }
    pub fn max_input_bytes(&self) -> usize {
        self.max_input_bytes
    }
    pub fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }
    pub fn max_concurrent_calls(&self) -> usize {
        self.max_concurrent_calls
    }
    pub fn memory_max_pages(&self) -> u32 {
        self.memory_max_pages
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
}

#[cfg(test)]
mod tests;
