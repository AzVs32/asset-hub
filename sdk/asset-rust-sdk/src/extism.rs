use crate::runtime::{
    DirectoryContext, DirectoryResponse, ResourceContext, ResourceResponse, Result,
};
use asset_plugin_api::protocol::{PluginActionFailure, PluginDiagnostic};
use extism_pdk::FnResult;

pub fn run_resource_action(
    input: String,
    handler: impl FnOnce(ResourceContext) -> Result<ResourceResponse>,
) -> FnResult<String> {
    structured_action_result((|| {
        let request = serde_json::from_str(&input)?;
        let output = handler(ResourceContext { request })?;
        Ok(serde_json::to_string(&output.inner)?)
    })())
}

pub fn run_directory_action(
    input: String,
    handler: impl FnOnce(DirectoryContext) -> Result<DirectoryResponse>,
) -> FnResult<String> {
    structured_action_result((|| {
        let request = serde_json::from_str(&input)?;
        let output = handler(DirectoryContext { request })?;
        Ok(serde_json::to_string(&output.inner)?)
    })())
}

fn structured_action_result(result: Result<String>) -> FnResult<String> {
    match result {
        Ok(output) => Ok(output),
        Err(error) => Ok(serde_json::to_string(&PluginActionFailure::new(
            PluginDiagnostic::error(
                asset_plugin_api::protocol::diagnostic::codes::ACTION_FAILED,
                error.to_string(),
            ),
        ))?),
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) mod content {
    use asset_plugin_api::abi::PluginContentRange;
    use extism_pdk::{Error, FnResult, host_fn};

    #[host_fn]
    extern "ExtismHost" {
        fn asset_hub_content_open(reference: String) -> String;
        fn asset_hub_content_size(handle: String) -> u64;
        fn asset_hub_content_read(handle: String, offset: u64, length: u64) -> Vec<u8>;
        fn asset_hub_content_close(handle: String);
    }

    pub(crate) fn read_all(reference: &str, max_size: u64, chunk_size: u64) -> FnResult<Vec<u8>> {
        if chunk_size == 0 {
            return Err(Error::msg("content host chunk size must be greater than zero").into());
        }
        with_content(reference, |handle, size| {
            if size > max_size {
                return Err(Error::msg(format!(
                    "content is {size} bytes, plugin limit is {max_size}"
                ))
                .into());
            }
            read_open_range(handle, PluginContentRange::new(0, size)?, chunk_size)
        })
    }

    pub(crate) fn read_range(
        reference: &str,
        range: PluginContentRange,
        max_size: u64,
        chunk_size: u64,
    ) -> FnResult<Vec<u8>> {
        if chunk_size == 0 {
            return Err(Error::msg("content host chunk size must be greater than zero").into());
        }
        with_content(reference, |handle, size| {
            if size > max_size {
                return Err(Error::msg(format!(
                    "content is {size} bytes, plugin limit is {max_size}"
                ))
                .into());
            }
            if range.end() > size {
                return Err(Error::msg("content host range is out of bounds").into());
            }
            read_open_range(handle, range, chunk_size)
        })
    }

    fn with_content<T>(
        reference: &str,
        operation: impl FnOnce(&str, u64) -> FnResult<T>,
    ) -> FnResult<T> {
        let handle = unsafe { asset_hub_content_open(reference.to_string()) }?;
        let result = (|| {
            let size = unsafe { asset_hub_content_size(handle.clone()) }?;
            operation(&handle, size)
        })();
        let close = unsafe { asset_hub_content_close(handle) };
        match (result, close) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    fn read_open_range(
        handle: &str,
        range: PluginContentRange,
        chunk_size: u64,
    ) -> FnResult<Vec<u8>> {
        let capacity = usize::try_from(range.length())
            .map_err(|_| Error::msg("content host range does not fit guest memory"))?;
        let mut bytes = Vec::with_capacity(capacity);
        let mut offset = range.offset();
        while offset < range.end() {
            let requested = (range.end() - offset).min(chunk_size);
            let chunk = unsafe { asset_hub_content_read(handle.to_string(), offset, requested) }?;
            if chunk.is_empty() || chunk.len() as u64 > requested {
                return Err(Error::msg("content host returned an invalid chunk").into());
            }
            offset += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::run_resource_action;
    use crate::{Diagnostic, Error, Frame, ResourceResponse};

    fn request(action: &str) -> String {
        serde_json::json!({
            "action": action,
            "access": "read",
            "resource": {
                "id": "01900000-0000-7000-8000-000000000000",
                "directory": "docs",
                "name": "demo.txt",
                "kind": "core:resource",
                "revision": 1,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            }
        })
        .to_string()
    }

    #[test]
    fn authoring_facade_owns_frame_version_and_structured_failures() {
        let input = request("example.open");
        let output = run_resource_action(input.clone(), |_| {
            Ok(ResourceResponse::frame(Frame::new("index.html")))
        })
        .unwrap();
        let output: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            output["plugin_api"],
            asset_plugin_api::protocol::PLUGIN_API_VERSION
        );

        let failure = run_resource_action(input, |_| Err(Error::msg("broken action"))).unwrap();
        let failure: serde_json::Value = serde_json::from_str(&failure).unwrap();
        assert_eq!(failure["error"]["code"], "plugin.action_failed");
    }

    #[test]
    fn response_diagnostics_do_not_require_protocol_types() {
        let output = run_resource_action(request("example.inspect"), |_| {
            Ok(ResourceResponse::without_view().diagnostic(
                Diagnostic::warning("example.partial", "some metadata was skipped")
                    .details(serde_json::json!({ "field": "author" }))?,
            ))
        })
        .unwrap();
        let output: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output["diagnostics"][0]["code"], "example.partial");
        assert_eq!(output["diagnostics"][0]["severity"], "warning");
        assert_eq!(output["diagnostics"][0]["details"]["field"], "author");
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) mod directory {
    use asset_plugin_api::abi::PluginDirectoryPageRequest;
    use asset_plugin_api::protocol::directory::{PluginDirectoryPage, PluginDirectoryResourcePage};
    use extism_pdk::{FnResult, host_fn};

    #[host_fn]
    extern "ExtismHost" {
        fn asset_hub_directory_list_children(request: String) -> String;
        fn asset_hub_directory_list_resources(request: String) -> String;
    }

    pub(crate) fn list_children_in(
        reference: &str,
        directory_id: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> FnResult<PluginDirectoryPage> {
        let request = PluginDirectoryPageRequest {
            reference: reference.to_string(),
            directory_id: directory_id.map(str::to_string),
            cursor: cursor.map(str::to_string),
            limit,
        };
        let response =
            unsafe { asset_hub_directory_list_children(serde_json::to_string(&request)?) }?;
        Ok(serde_json::from_str(&response)?)
    }

    pub(crate) fn list_resources_in(
        reference: &str,
        directory_id: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> FnResult<PluginDirectoryResourcePage> {
        let request = PluginDirectoryPageRequest {
            reference: reference.to_string(),
            directory_id: directory_id.map(str::to_string),
            cursor: cursor.map(str::to_string),
            limit,
        };
        let response =
            unsafe { asset_hub_directory_list_resources(serde_json::to_string(&request)?) }?;
        Ok(serde_json::from_str(&response)?)
    }
}
