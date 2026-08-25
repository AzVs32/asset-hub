use crate::runtime::{Error, Result};
use asset_plugin_api::abi::ContentRange;
use asset_plugin_api::protocol::{PluginDirectoryPage, PluginDirectoryResourcePage};

#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
pub(crate) fn read_content_range(
    reference: &str,
    range: ContentRange,
    max_size: u64,
    chunk_size: u64,
) -> Result<Vec<u8>> {
    crate::extism::content::read_range(reference, range, max_size, chunk_size).map_err(extism_error)
}

#[cfg(not(all(feature = "extism-guest", target_arch = "wasm32")))]
pub(crate) fn read_content_range(
    _reference: &str,
    _range: ContentRange,
    _max_size: u64,
    _chunk_size: u64,
) -> Result<Vec<u8>> {
    Err(adapter_unavailable())
}

#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
pub(crate) fn read_all_content(reference: &str, max_size: u64, chunk_size: u64) -> Result<Vec<u8>> {
    crate::extism::content::read_all(reference, max_size, chunk_size).map_err(extism_error)
}

#[cfg(not(all(feature = "extism-guest", target_arch = "wasm32")))]
pub(crate) fn read_all_content(
    _reference: &str,
    _max_size: u64,
    _chunk_size: u64,
) -> Result<Vec<u8>> {
    Err(adapter_unavailable())
}

#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
pub(crate) fn list_directory_children(
    reference: &str,
    directory_id: Option<&str>,
    cursor: Option<&str>,
    limit: u32,
) -> Result<PluginDirectoryPage> {
    crate::extism::directory::list_children_in(reference, directory_id, cursor, limit)
        .map_err(extism_error)
}

#[cfg(not(all(feature = "extism-guest", target_arch = "wasm32")))]
pub(crate) fn list_directory_children(
    _reference: &str,
    _directory_id: Option<&str>,
    _cursor: Option<&str>,
    _limit: u32,
) -> Result<PluginDirectoryPage> {
    Err(adapter_unavailable())
}

#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
pub(crate) fn list_directory_resources(
    reference: &str,
    directory_id: Option<&str>,
    cursor: Option<&str>,
    limit: u32,
) -> Result<PluginDirectoryResourcePage> {
    crate::extism::directory::list_resources_in(reference, directory_id, cursor, limit)
        .map_err(extism_error)
}

#[cfg(not(all(feature = "extism-guest", target_arch = "wasm32")))]
pub(crate) fn list_directory_resources(
    _reference: &str,
    _directory_id: Option<&str>,
    _cursor: Option<&str>,
    _limit: u32,
) -> Result<PluginDirectoryResourcePage> {
    Err(adapter_unavailable())
}

#[cfg(all(feature = "extism-guest", target_arch = "wasm32"))]
fn extism_error(error: extism_pdk::WithReturnCode<extism_pdk::Error>) -> Error {
    Error::from_display(error.0)
}

#[cfg(not(all(feature = "extism-guest", target_arch = "wasm32")))]
fn adapter_unavailable() -> Error {
    Error::msg("Wasm Host access requires an enabled runtime adapter")
}
