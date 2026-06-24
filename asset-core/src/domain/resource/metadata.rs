use super::normalize_required_text;
use crate::error::ResourceError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 元数据键允许的最大字符数。
const MAX_METADATA_KEY_LEN: usize = 255;

// ==================================================
// 资源metadata
// ==================================================

/// 用于承载资源的动态元数据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceMetadata(Value);

impl ResourceMetadata {
    /// 使用任意 JSON 值创建元数据。
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// 获取内部 `Value` 的只读引用。
    ///
    /// 适用于只需要读取元数据内容，而不转移所有权的场景。
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// 消费 `ResourceMetadata` 并返回内部的 `Value`。
    ///
    /// 该方法会转移所有权（Ownership）。
    pub fn into_value(self) -> Value {
        self.0
    }

    /// 按 key 读取对象型元数据中的字段。
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.as_object().and_then(|object| object.get(key))
    }

    /// 向对象型元数据写入字段。
    ///
    /// 如果当前元数据不是 JSON object，会返回格式错误。
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: Value,
    ) -> Result<Option<Value>, ResourceError> {
        let key = normalize_required_text("metadata.key", &key.into(), MAX_METADATA_KEY_LEN)?;
        let object = self.0.as_object_mut().ok_or(ResourceError::InvalidFormat {
            field: "metadata",
            reason: "metadata must be a JSON object",
        })?;

        Ok(object.insert(key, value))
    }

    /// 检查元数据是否为空。
    pub fn is_empty(&self) -> bool {
        match &self.0 {
            Value::Object(map) => map.is_empty(),
            Value::Null => true,
            _ => false,
        }
    }
}

impl Default for ResourceMetadata {
    /// 提供默认实现，初始为一个空的 JSON 对象（`{}`）。
    fn default() -> Self {
        Self(Value::Object(Map::new()))
    }
}

impl From<Value> for ResourceMetadata {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}
