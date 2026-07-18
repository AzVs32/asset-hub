use super::normalize_required_text;
use crate::error::ResourceError;
use serde::{Deserialize, Deserializer, Serialize};

/// 单个资源标签允许的最大字符数。
const MAX_RESOURCE_TAG_LEN: usize = 64;

/// 已完成归一化和校验的资源标签。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ResourceTag(String);

impl ResourceTag {
    pub fn try_new(value: impl Into<String>) -> Result<Self, ResourceError> {
        normalize_required_text("resource.tag", &value.into(), MAX_RESOURCE_TAG_LEN).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ResourceTag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::try_new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for ResourceTag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}
