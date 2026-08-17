//! Plugin API 中用于查询当前目录子树内直属子目录和资源的 Wasm Host functions。
//!
//! Host API 只接受单次 Action 调用期间有效的不透明目录引用，并返回协议层定义的分页
//! DTO；子树查询仍被绑定在当前调用的 Directory 范围内。

pub const DIRECTORY_LIST_CHILDREN_FN: &str = "asset_hub_directory_list_children";
pub const DIRECTORY_LIST_RESOURCES_FN: &str = "asset_hub_directory_list_resources";

#[cfg(any(test, all(feature = "extism-guest", target_arch = "wasm32")))]
fn child_page_request(
    reference: &str,
    directory_id: Option<&str>,
    cursor: Option<&str>,
    limit: u32,
) -> String {
    let mut request = serde_json::json!({
        "reference": reference,
        "cursor": cursor,
        "limit": limit
    });
    if let Some(directory_id) = directory_id {
        request
            .as_object_mut()
            .expect("directory page request is an object")
            .insert(
                "directory_id".to_string(),
                serde_json::Value::String(directory_id.to_string()),
            );
    }
    request.to_string()
}

#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
pub mod guest {
    //! Extism guest 对 directory host functions 的类型安全调用封装。

    use crate::protocol::directory::{PluginDirectoryPage, PluginDirectoryResourcePage};
    use extism_pdk::{FnResult, host_fn};

    #[host_fn]
    extern "ExtismHost" {
        fn asset_hub_directory_list_children(request: String) -> String;
        fn asset_hub_directory_list_resources(request: String) -> String;
    }

    pub fn list_children(
        reference: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> FnResult<PluginDirectoryPage> {
        list_children_in(reference, None, cursor, limit)
    }

    /// Lists direct children of the current Directory or one of its descendants.
    pub fn list_children_in(
        reference: &str,
        directory_id: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> FnResult<PluginDirectoryPage> {
        let request = super::child_page_request(reference, directory_id, cursor, limit);
        let response = unsafe { asset_hub_directory_list_children(request) }?;
        Ok(serde_json::from_str(&response)?)
    }

    pub fn list_resources(
        reference: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> FnResult<PluginDirectoryResourcePage> {
        list_resources_in(reference, None, cursor, limit)
    }

    /// Lists resources in the current Directory or one of its descendants.
    pub fn list_resources_in(
        reference: &str,
        directory_id: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> FnResult<PluginDirectoryResourcePage> {
        let request = serde_json::json!({
            "reference": reference,
            "directory_id": directory_id,
            "cursor": cursor,
            "limit": limit
        })
        .to_string();
        let response = unsafe { asset_hub_directory_list_resources(request) }?;
        Ok(serde_json::from_str(&response)?)
    }
}

#[cfg(test)]
mod tests {
    use super::child_page_request;

    #[test]
    fn descendant_child_request_preserves_the_root_query_shape() {
        let root: serde_json::Value =
            serde_json::from_str(&child_page_request("opaque", None, Some("20"), 10)).unwrap();
        assert_eq!(
            root,
            serde_json::json!({"reference": "opaque", "cursor": "20", "limit": 10})
        );

        let descendant: serde_json::Value = serde_json::from_str(&child_page_request(
            "opaque",
            Some("0198a1b2-c3d4-7e5f-8012-3456789abcde"),
            None,
            10,
        ))
        .unwrap();
        assert_eq!(
            descendant["directory_id"],
            "0198a1b2-c3d4-7e5f-8012-3456789abcde"
        );
    }
}
