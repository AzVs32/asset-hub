use asset_plugin_api::protocol::{
    MediaView, PLUGIN_API_VERSION, PluginDirectoryActionOutput, PluginDirectoryActionRequest,
    PluginFrameView, PluginMediaEncoding, PluginView,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use extism_pdk::{FnResult, plugin_fn};

const THUMBNAIL_SVG: &str = include_str!("thumbnail.svg");

#[plugin_fn]
pub fn render_workspace(input: String) -> FnResult<String> {
    render_workspace_payload(input)
}

#[plugin_fn]
pub fn render_thumbnail(input: String) -> FnResult<String> {
    render_thumbnail_payload(input)
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

fn render_thumbnail_payload(input: String) -> FnResult<String> {
    let request: PluginDirectoryActionRequest = serde_json::from_str(&input)?;
    let output = PluginDirectoryActionOutput::new(PluginView::Media(MediaView {
        mime_type: "image/svg+xml".to_string(),
        title: Some(request.directory.name),
        encoding: PluginMediaEncoding::Base64,
        data: BASE64_STANDARD.encode(THUMBNAIL_SVG.as_bytes()),
    }));

    Ok(serde_json::to_string(&output)?)
}

#[cfg(test)]
mod tests {
    use super::{THUMBNAIL_SVG, render_thumbnail_payload, render_workspace_payload};
    use asset_plugin_api::protocol::{
        PluginDirectoryActionOutput, PluginMediaEncoding, PluginView,
    };
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

    fn action_request(action: &str) -> String {
        serde_json::json!({
            "action": action,
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
        })
        .to_string()
    }

    #[test]
    fn workspace_action_returns_the_static_plugin_frame() {
        let output: PluginDirectoryActionOutput = serde_json::from_str(
            &render_workspace_payload(action_request("azvs.directory.hello.workspace"))
                .expect("render workspace"),
        )
        .expect("deserialize output");

        let Some(PluginView::PluginFrame(frame)) = output.view else {
            panic!("expected plugin_frame output");
        };
        assert_eq!(frame.plugin_api, "asset-hub.plugin-api@4");
        assert_eq!(frame.title.as_deref(), Some("Hello Directory"));
        assert_eq!(frame.url, "index.html");
        assert!(output.effects.is_empty());
    }

    #[test]
    fn thumbnail_action_returns_the_hello_svg() {
        let output: PluginDirectoryActionOutput = serde_json::from_str(
            &render_thumbnail_payload(action_request("azvs.directory.hello.thumbnail"))
                .expect("render thumbnail"),
        )
        .expect("deserialize output");

        let Some(PluginView::Media(media)) = output.view else {
            panic!("expected media output");
        };
        assert_eq!(media.mime_type, "image/svg+xml");
        assert_eq!(media.title.as_deref(), Some("Hello"));
        assert_eq!(media.encoding, PluginMediaEncoding::Base64);
        assert_eq!(
            BASE64_STANDARD
                .decode(media.data)
                .expect("decode thumbnail"),
            THUMBNAIL_SVG.as_bytes()
        );
        assert!(output.effects.is_empty());
    }
}
