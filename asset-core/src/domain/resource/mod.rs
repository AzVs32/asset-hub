mod content;
mod directory;
mod kind;
mod metadata;
#[allow(clippy::module_inception)]
mod resource;
mod status;

pub use content::{Checksum, ChecksumKind, ResourceContent, ResourceContentBuilder, StorageKey};
pub use directory::ResourceDirectory;
pub use kind::ResourceKind;
pub use metadata::{ResourceMetadata, ResourceMetadataBuilder, ResourceSummaryMetadata};
pub use resource::{Resource, ResourceBuilder, ResourceId, ResourceSnapshot};
pub use status::ResourceStatus;

use crate::error::ResourceError;

/// 归一化并校验资源领域中的必填文本字段。
///
/// 校验规则：
/// - 去除首尾空白后不能为空，否则返回 `ResourceError::Blank`。
/// - 文本长度按 Unicode 字符数计算，超过 `max` 时返回 `ResourceError::TooLong`。
/// - 不允许包含控制字符，例如换行、制表符或不可见控制码，否则返回
///   `ResourceError::InvalidFormat`。
///
/// `field` 用于标识具体出错的领域字段，最终会原样出现在错误对象中，方便调用方定位
/// 是哪个属性没有通过校验。
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

    Ok(value.to_string())
}

#[cfg(test)]
mod tests;
