//! Core 执行资源 Action 时采用的内容预算。

use std::fmt;

/// Content limits enforced before dispatching any resource action executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceActionPolicy {
    max_content_bytes: u64,
    max_inline_content_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidResourceActionPolicy(&'static str);

impl fmt::Display for InvalidResourceActionPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for InvalidResourceActionPolicy {}

impl ResourceActionPolicy {
    pub fn new(
        max_content_bytes: u64,
        max_inline_content_bytes: u64,
    ) -> Result<Self, InvalidResourceActionPolicy> {
        if max_content_bytes == 0 || max_inline_content_bytes == 0 {
            return Err(InvalidResourceActionPolicy(
                "resource action content limits must be greater than zero",
            ));
        }
        Ok(Self {
            max_content_bytes,
            max_inline_content_bytes: max_inline_content_bytes.min(max_content_bytes),
        })
    }

    pub fn max_content_bytes(&self) -> u64 {
        self.max_content_bytes
    }

    pub fn max_inline_content_bytes(&self) -> u64 {
        self.max_inline_content_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_inline_limit_and_rejects_zero_values() {
        let policy = ResourceActionPolicy::new(8, 9).unwrap();
        assert_eq!(policy.max_inline_content_bytes(), 8);
        assert!(ResourceActionPolicy::new(0, 1).is_err());
    }
}
