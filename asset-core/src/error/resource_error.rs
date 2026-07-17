use thiserror::Error;

/// 资源领域内的业务校验和状态流转错误。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResourceError {
    /// 必填文本字段为空或只有空白字符。
    #[error("{field} cannot be blank")]
    Blank {
        /// 为空的字段名。
        field: &'static str,
    },

    /// 文本字段超过领域允许的最大字符数。
    #[error("{field} must not exceed {max} characters")]
    TooLong {
        /// 超长的字段名。
        field: &'static str,
        /// 允许的最大字符数。
        max: usize,
    },

    /// 字段格式不符合领域约束。
    #[error("{field} has invalid format: {reason}")]
    InvalidFormat {
        /// 发生格式错误的字段名。
        field: &'static str,
        /// 具体格式错误原因。
        reason: &'static str,
    },

    /// Kind 专属元数据没有满足所属 Kind、版本或 schema 约束。
    #[error("metadata.kind_metadata layer `{kind}` is invalid: {reason}")]
    InvalidKindMetadata {
        /// 声明该层数据所有权的资源类型。
        kind: String,
        /// 可以安全返回给调用方的具体失败原因。
        reason: String,
    },

    /// 已软删除的资源不允许继续修改。
    #[error("deleted resource cannot be modified")]
    DeletedResource,
}
