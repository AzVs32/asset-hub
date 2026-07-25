use super::normalize_required_text;
use crate::error::ResourceError;
use serde::{Deserialize, Serialize};

/// 资源类型允许的最大字符数。
const MAX_RESOURCE_KIND_LEN: usize = 256;

/// 资源类型值对象。
///
/// 类型必须使用小写的 `namespace:name` 形式。命名空间和名称只允许包含 ASCII
/// 字母、数字、`.`、`-` 和 `_`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResourceKind(String);

impl ResourceKind {
    /// 未指定或未能识别具体类型时使用的默认资源类型。
    pub const DEFAULT: &'static str = "core:resource";

    /// 创建、规范化并校验资源类型。
    pub fn try_new(value: impl Into<String>) -> Result<Self, ResourceError> {
        normalize_resource_kind(value.into()).map(Self)
    }

    /// 获取内部原始字符串的只读借用（`&str`）。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// 判断当前资源类型是否等于指定类型值。
    ///
    /// 例如
    /// `kind.is(ResourceKind::DEFAULT)` 或 `kind.is("core:image")`。
    pub fn is(&self, kind: impl AsRef<str>) -> bool {
        self.0 == kind.as_ref()
    }
}

impl Default for ResourceKind {
    /// 默认值：`core:resource`。
    fn default() -> Self {
        Self::try_new(Self::DEFAULT).expect("core:resource must be a valid resource kind")
    }
}

/// 打印支持，便于在`format!("{kind}")`或`info!("{kind}")`中使用。
impl std::fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 支持以 `&str` 形式借用资源类型值，便于复用字符串比较和通用接口。
impl AsRef<str> for ResourceKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<String> for ResourceKind {
    type Error = ResourceError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<ResourceKind> for String {
    fn from(value: ResourceKind) -> Self {
        value.0
    }
}

/// 允许使用 `s.parse::<ResourceKind>()` 语法。
impl std::str::FromStr for ResourceKind {
    type Err = ResourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_new(s)
    }
}

fn normalize_resource_kind(value: String) -> Result<String, ResourceError> {
    let value = normalize_required_text("resource.kind", &value, MAX_RESOURCE_KIND_LEN)?
        .to_ascii_lowercase();
    let Some((namespace, name)) = value.split_once(':') else {
        return Err(ResourceError::InvalidFormat {
            field: "resource.kind",
            reason: "resource kind must use namespace:name format",
        });
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_')
                    || character == '.'
            })
    };
    if !valid(namespace) || !valid(name) {
        return Err(ResourceError::InvalidFormat {
            field: "resource.kind",
            reason: "resource kind contains invalid characters",
        });
    }
    Ok(value)
}
