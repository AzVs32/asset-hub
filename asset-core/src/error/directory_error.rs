use thiserror::Error;

/// 目录领域内的业务校验和树结构约束错误。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DirectoryError {
    #[error("{field} cannot be blank")]
    Blank { field: &'static str },

    #[error("{field} must not exceed {max} characters")]
    TooLong { field: &'static str, max: usize },

    #[error("{field} has invalid format: {reason}")]
    InvalidFormat {
        field: &'static str,
        reason: &'static str,
    },
}
