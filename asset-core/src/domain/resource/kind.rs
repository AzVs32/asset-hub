use super::normalize_required_text;
use crate::error::ResourceError;
use serde::{Deserialize, Serialize};

/// 资源类型允许的最大字符数。
const MAX_RESOURCE_KIND_LEN: usize = 128;

// ==================================================
// 资源类型
// ==================================================

/// 资源类型值对象。
///
/// 建议使用 `namespace:typename` 形式避免不同业务模块之间的类型冲突。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceKind(String);

impl ResourceKind {
    /// 资源类型未知
    pub const UNKNOWN: &'static str = "core:unknown";

    /// 返回核心内置资源类型。
    pub const fn builtin_values() -> &'static [&'static str] {
        &[Self::UNKNOWN]
    }

    /// 创建一个资源类型实例。支持传入 `String` 或 `&str`。
    ///
    /// 建议采用 `namespace:typename` 的命名规范防止冲突。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_string())
    }

    /// 创建并校验资源类型。
    pub fn try_new(value: impl Into<String>) -> Result<Self, ResourceError> {
        let kind = Self::new(value);
        kind.validate()?;
        Ok(kind)
    }

    /// 获取内部原始字符串的只读借用（`&str`）。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// 判断当前资源类型是否等于指定类型值。
    ///
    /// 例如
    /// `kind.is(ResourceKind::UNKNOWN)` 或 `kind.is("asset:image")`。
    pub fn is(&self, kind: impl AsRef<str>) -> bool {
        self.0 == kind.as_ref()
    }

    /// 校验资源类型是否满足领域规则。
    pub fn validate(&self) -> Result<(), ResourceError> {
        normalize_required_text("resource.kind", &self.0, MAX_RESOURCE_KIND_LEN)?;

        if self.0.chars().any(char::is_whitespace) {
            return Err(ResourceError::InvalidFormat {
                field: "resource.kind",
                reason: "whitespace is not allowed",
            });
        }

        Ok(())
    }
}

impl Default for ResourceKind {
    /// 默认值： "UNKNOWN"。
    fn default() -> Self {
        Self(Self::UNKNOWN.to_string())
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

/// 将能转换成 `String` 的类型允许通过 `.into()` 转换为 `ResourceKind` 类型。
impl<T: Into<String>> From<T> for ResourceKind {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

/// 允许使用 `s.parse::<ResourceKind>()` 语法。
impl std::str::FromStr for ResourceKind {
    type Err = ResourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_new(s)
    }
}
