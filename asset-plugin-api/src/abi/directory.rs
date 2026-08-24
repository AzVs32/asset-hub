//! Plugin API 中用于查询当前目录子树内直属子目录和资源的 Wasm Host functions。
//!
//! Host API 只接受单次 Action 调用期间有效的不透明目录引用，并返回协议层定义的分页
//! DTO；子树查询仍被绑定在当前调用的 Directory 范围内。

use serde::{Deserialize, Serialize};

pub const DIRECTORY_LIST_CHILDREN_FN: &str = "asset_hub_directory_list_children";
pub const DIRECTORY_LIST_RESOURCES_FN: &str = "asset_hub_directory_list_resources";
pub const DIRECTORY_PAGE_MAX_LIMIT: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDirectoryPageRequest {
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory_id: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    pub limit: u32,
}

#[cfg(test)]
mod tests {
    use super::PluginDirectoryPageRequest;

    #[test]
    fn descendant_child_request_preserves_the_root_query_shape() {
        let root = PluginDirectoryPageRequest {
            reference: "opaque".to_string(),
            directory_id: None,
            cursor: Some("20".to_string()),
            limit: 10,
        };
        let root = serde_json::to_value(root).unwrap();
        assert_eq!(
            root,
            serde_json::json!({"reference": "opaque", "cursor": "20", "limit": 10})
        );

        let descendant = serde_json::to_value(PluginDirectoryPageRequest {
            reference: "opaque".to_string(),
            directory_id: Some("0198a1b2-c3d4-7e5f-8012-3456789abcde".to_string()),
            cursor: None,
            limit: 10,
        })
        .unwrap();
        assert_eq!(
            descendant["directory_id"],
            "0198a1b2-c3d4-7e5f-8012-3456789abcde"
        );
    }
}
