//! Host 文本内容编辑的运行时无关策略。

use std::fmt;

/// Limits for interactive text replacement independent of plugin execution budgets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceContentEditPolicy {
    max_text_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidResourceContentEditPolicy(&'static str);

impl fmt::Display for InvalidResourceContentEditPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for InvalidResourceContentEditPolicy {}

impl ResourceContentEditPolicy {
    pub fn new(max_text_bytes: u64) -> Result<Self, InvalidResourceContentEditPolicy> {
        if max_text_bytes == 0 {
            return Err(InvalidResourceContentEditPolicy(
                "resource text edit limit must be greater than zero",
            ));
        }
        Ok(Self { max_text_bytes })
    }

    pub fn max_text_bytes(&self) -> u64 {
        self.max_text_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_unbounded_zero_limit() {
        assert!(ResourceContentEditPolicy::new(0).is_err());
        assert_eq!(
            ResourceContentEditPolicy::new(1024)
                .unwrap()
                .max_text_bytes(),
            1024
        );
    }
}
