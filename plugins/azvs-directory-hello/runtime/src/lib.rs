use asset_plugin_api::protocol::{
    PLUGIN_API_VERSION, PluginDirectoryActionOutput, PluginDirectoryActionRequest, PluginFrameView,
    PluginView,
};
use extism_pdk::{FnResult, plugin_fn};

#[plugin_fn]
pub fn render_workspace(input: String) -> FnResult<String> {
    render_workspace_payload(input)
}

fn render_workspace_payload(input: String) -> FnResult<String> {
    let _request: PluginDirectoryActionRequest = serde_json::from_str(&input)?;
    let output = PluginDirectoryActionOutput::new(PluginView::PluginFrame(PluginFrameView {
        plugin_api: PLUGIN_API_VERSION.to_string(),
        title: Some("Hello Directory".to_string()),
        url: "index.html".to_string(),
    }));

    Ok(serde_json::to_string(&output)?)
}

#[cfg(test)]
mod tests {
    use super::render_workspace_payload;
    use asset_plugin_api::protocol::{PluginDirectoryActionOutput, PluginView};

    #[test]
    fn workspace_action_returns_the_static_plugin_frame() {
        let input = serde_json::json!({
            "action": "azvs.directory.hello.workspace",
            "access": "read",
            "input": {},
            "directory": {
                "id": "0198a1b2-c3d4-7e5f-8012-3456789abcde",
                "parent_id": null,
                "path": "/hello",
                "name": "Hello",
                "kind": "azvs:directory.hello",
                "revision": 1,
                "created_at": "2026-08-13T00:00:00Z",
                "updated_at": "2026-08-13T00:00:00Z"
            },
            "directory_ref": "opaque-directory-ref"
        });

        let output: PluginDirectoryActionOutput = serde_json::from_str(
            &render_workspace_payload(input.to_string()).expect("render workspace"),
        )
        .expect("deserialize output");

        let Some(PluginView::PluginFrame(frame)) = output.view else {
            panic!("expected plugin_frame output");
        };
        assert_eq!(frame.plugin_api, "asset-hub.plugin-api@3");
        assert_eq!(frame.title.as_deref(), Some("Hello Directory"));
        assert_eq!(frame.url, "index.html");
        assert!(output.effects.is_empty());
    }
}
