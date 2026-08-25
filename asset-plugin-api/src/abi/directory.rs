//! Plugin API 中用于查询当前目录子树内直属子目录和资源的 Wasm Host functions。
//!
//! Host API 只接受单次 Action 调用期间有效的不透明目录引用，并返回协议层定义的分页
//! DTO；子树查询仍被绑定在当前调用的 Directory 范围内。

use super::contract::{DIRECTORY_LIST_CHILDREN, DIRECTORY_LIST_RESOURCES};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

pub const DIRECTORY_LIST_CHILDREN_FN: &str = DIRECTORY_LIST_CHILDREN.name;
pub const DIRECTORY_LIST_RESOURCES_FN: &str = DIRECTORY_LIST_RESOURCES.name;
pub const DIRECTORY_PAGE_MAX_LIMIT: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DirectoryPageRequest {
    reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    directory_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
    limit: u32,
}

impl DirectoryPageRequest {
    pub fn new(
        reference: impl Into<String>,
        limit: u32,
    ) -> Result<Self, DirectoryPageRequestError> {
        let reference = non_empty(reference.into(), DirectoryPageRequestError::EmptyReference)?;
        if !(1..=DIRECTORY_PAGE_MAX_LIMIT).contains(&limit) {
            return Err(DirectoryPageRequestError::InvalidLimit);
        }
        Ok(Self {
            reference,
            directory_id: None,
            cursor: None,
            limit,
        })
    }

    pub fn in_directory(
        mut self,
        directory_id: impl Into<String>,
    ) -> Result<Self, DirectoryPageRequestError> {
        self.directory_id = Some(non_empty(
            directory_id.into(),
            DirectoryPageRequestError::EmptyDirectoryId,
        )?);
        Ok(self)
    }

    pub fn after(mut self, cursor: impl Into<String>) -> Result<Self, DirectoryPageRequestError> {
        self.cursor = Some(non_empty(
            cursor.into(),
            DirectoryPageRequestError::EmptyCursor,
        )?);
        Ok(self)
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    pub fn directory_id(&self) -> Option<&str> {
        self.directory_id.as_deref()
    }

    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryPageRequestDocument {
    reference: String,
    #[serde(default)]
    directory_id: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    limit: u32,
}

impl<'de> Deserialize<'de> for DirectoryPageRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = DirectoryPageRequestDocument::deserialize(deserializer)?;
        let mut request =
            Self::new(document.reference, document.limit).map_err(serde::de::Error::custom)?;
        if let Some(directory_id) = document.directory_id {
            request = request
                .in_directory(directory_id)
                .map_err(serde::de::Error::custom)?;
        }
        if let Some(cursor) = document.cursor {
            request = request.after(cursor).map_err(serde::de::Error::custom)?;
        }
        Ok(request)
    }
}

fn non_empty(
    value: String,
    error: DirectoryPageRequestError,
) -> Result<String, DirectoryPageRequestError> {
    if value.trim().is_empty() {
        Err(error)
    } else {
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryPageRequestError {
    EmptyReference,
    EmptyDirectoryId,
    EmptyCursor,
    InvalidLimit,
}

impl std::fmt::Display for DirectoryPageRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyReference => "directory reference must not be empty",
            Self::EmptyDirectoryId => "directory id must not be empty",
            Self::EmptyCursor => "directory page cursor must not be empty",
            Self::InvalidLimit => "directory page limit must be between 1 and 100",
        })
    }
}

impl std::error::Error for DirectoryPageRequestError {}

#[cfg(test)]
mod tests {
    use super::DirectoryPageRequest;

    #[test]
    fn descendant_child_request_preserves_the_root_query_shape() {
        let root = DirectoryPageRequest::new("opaque", 10)
            .unwrap()
            .after("20")
            .unwrap();
        let root = serde_json::to_value(root).unwrap();
        assert_eq!(
            root,
            serde_json::json!({"reference": "opaque", "cursor": "20", "limit": 10})
        );

        let descendant = serde_json::to_value(
            DirectoryPageRequest::new("opaque", 10)
                .unwrap()
                .in_directory("0198a1b2-c3d4-7e5f-8012-3456789abcde")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            descendant["directory_id"],
            "0198a1b2-c3d4-7e5f-8012-3456789abcde"
        );
    }
}
