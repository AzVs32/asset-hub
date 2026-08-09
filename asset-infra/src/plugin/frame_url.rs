use asset_core::CoreError;
use asset_plugin_api::protocol::{PLUGIN_API_VERSION, PluginResourceActionOutput, PluginView};

pub(super) fn resolve_plugin_output_urls(
    output: &mut PluginResourceActionOutput,
    plugin_id: &str,
) -> Result<(), CoreError> {
    if let PluginView::PluginFrame(frame) = &mut output.view {
        if frame.plugin_api != PLUGIN_API_VERSION {
            return Err(CoreError::plugin(
                plugin_id,
                "plugin_frame",
                format!(
                    "plugin_frame requires plugin API `{}`, got `{}`",
                    PLUGIN_API_VERSION, frame.plugin_api
                ),
            ));
        }
        frame.url = plugin_web_asset_url(plugin_id, &frame.url)?;
    }
    Ok(())
}

pub(super) fn plugin_web_asset_url(plugin_id: &str, url: &str) -> Result<String, CoreError> {
    let url = url.trim();
    if url.starts_with('/') || url.contains("://") || url.starts_with("//") {
        return Err(CoreError::plugin(
            plugin_id,
            "plugin_frame",
            "plugin_frame URL must be relative to the plugin Web root",
        ));
    }

    let relative = url.trim_start_matches("./");
    let relative = if relative.is_empty() {
        "index.html".to_string()
    } else if relative.starts_with('#') || relative.starts_with('?') {
        format!("index.html{relative}")
    } else {
        relative.to_string()
    };

    if relative.split('/').any(|part| part == "..") {
        return Err(CoreError::plugin(
            plugin_id,
            "plugin_frame",
            "plugin_frame URL contains a parent path segment",
        ));
    }
    Ok(format!("/plugins/{plugin_id}/{relative}"))
}
