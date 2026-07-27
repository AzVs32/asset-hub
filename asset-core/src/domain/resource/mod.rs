mod content;
mod kind;
#[allow(clippy::module_inception)]
mod resource;
mod tag;

use crate::error::ResourceError;

pub use content::{Checksum, ChecksumKind, ResourceContent, ResourceContentBuilder, StorageKey};
pub use kind::ResourceKind;
pub use resource::{Resource, ResourceBuilder, ResourceId, ResourceSnapshot};
pub use tag::ResourceTag;

/// 归一化并校验 Resource 领域模型中的必填文本。
fn normalize_required_text(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, ResourceError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ResourceError::Blank { field });
    }
    if value.chars().count() > max {
        return Err(ResourceError::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(ResourceError::InvalidFormat {
            field,
            reason: "control characters are not allowed",
        });
    }
    Ok(value.to_owned())
}

/// 校验需要原样保存的 Resource 必填文本，不执行首尾裁剪。
fn validate_required_text_exact(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<String, ResourceError> {
    if value.trim().is_empty() {
        return Err(ResourceError::Blank { field });
    }
    if value.chars().count() > max {
        return Err(ResourceError::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(ResourceError::InvalidFormat {
            field,
            reason: "control characters are not allowed",
        });
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests;
