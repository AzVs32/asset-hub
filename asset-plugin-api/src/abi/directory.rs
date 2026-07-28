//! Directory Action 查询直属子目录和直属资源的 Wasm Host ABI。
//!
//! Host API 只接受单次 Action 调用期间有效的不透明目录引用，并返回协议层定义的分页
//! DTO；guest helper 不具备跨目录选择或整棵树遍历能力。

pub const DIRECTORY_HOST_API_VERSION: u32 = 1;
pub const DIRECTORY_LIST_CHILDREN_FN: &str = "asset_hub_directory_list_children";
pub const DIRECTORY_LIST_RESOURCES_FN: &str = "asset_hub_directory_list_resources";

#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
pub mod guest {
    //! Extism guest 对 Directory Host API 的类型安全调用封装。

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
        let request = serde_json::json!({"reference": reference, "cursor": cursor, "limit": limit})
            .to_string();
        let response = unsafe { asset_hub_directory_list_resources(request) }?;
        Ok(serde_json::from_str(&response)?)
    }
}
