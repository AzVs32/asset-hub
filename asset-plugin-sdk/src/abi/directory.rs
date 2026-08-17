//! Plugin API 中用于查询直属子目录和当前目录子树资源的 Wasm Host functions。
//!
//! Host API 只接受单次 Action 调用期间有效的不透明目录引用，并返回协议层定义的分页
//! DTO；子树查询仍被绑定在当前调用的 Directory 范围内。

pub const DIRECTORY_LIST_CHILDREN_FN: &str = "asset_hub_directory_list_children";
pub const DIRECTORY_LIST_RESOURCES_FN: &str = "asset_hub_directory_list_resources";

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
        let request = serde_json::json!({"reference": reference, "cursor": cursor, "limit": limit})
            .to_string();
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
