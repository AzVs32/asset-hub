use asset_plugin_api::{
    MediaView, PluginActionOutput, PluginActionRequest, PluginContentEncoding,
    PluginResource, PluginResourceContent, PluginView,
};
use extism_pdk::{plugin_fn, FnResult};
use serde_json::{Value, json};

#[plugin_fn]
pub fn play_mp4(input: String) -> FnResult<String> {
    play_mp4_payload(input)
}

fn play_mp4_payload(input: String) -> FnResult<String> {
    let input: PluginActionRequest = serde_json::from_str(&input)?;
    let title = video_title(&input.resource);
    let mime_type = video_mime_type(input.resource.content.as_ref());

    Ok(serde_json::to_string(&PluginActionOutput::new(
        PluginView::Media(MediaView {
            mime_type,
            title: Some(title),
            encoding: PluginContentEncoding::Url,
            data: format!("/resources/{}/content", input.resource.id),
        }),
    ))?)
}

fn video_title(resource: &PluginResource) -> String {
    resource
        .metadata
        .get("kind")
        .and_then(|kind| kind.get("data"))
        .and_then(|data| data.get("title"))
        .and_then(Value::as_str)
        .or(resource
            .content
            .as_ref()
            .and_then(|content| content.original_filename.as_deref()))
        .unwrap_or(&resource.name)
        .trim()
        .to_string()
}

fn video_mime_type(content: Option<&PluginResourceContent>) -> String {
    content
        .and_then(|content| content.mime_type.as_deref())
        .filter(|mime_type| mime_type.starts_with("video/"))
        .unwrap_or("video/mp4")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_prefers_kind_metadata() {
        let resource = PluginResource {
            name: "fallback.mp4".to_string(),
            metadata: json!({
                "kind": {
                    "data": {
                        "title": "Demo Video"
                    }
                }
            }),
            content: Some(PluginResourceContent {
                key: "videos/file.mp4".to_string(),
                size: 4,
                mime_type: Some("video/mp4".to_string()),
                original_filename: Some("file.mp4".to_string()),
                checksum: Vec::new(),
            }),
            id: "01900000-0000-7000-8000-000000000000".to_string(),
            kind: "core:video".to_string(),
            status: "active".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            deleted_at: None,
        };

        assert_eq!(video_title(&resource), "Demo Video");
    }

    #[test]
    fn play_mp4_returns_url_media_view() {
        let output = play_mp4_payload(
            json!({
                "action": "azvs:play_mp4",
                "access": "read_only",
                "input": {},
                "resource": {
                    "id": "01900000-0000-7000-8000-000000000000",
                    "name": "demo.mp4",
                    "kind": "core:video",
                    "status": "active",
                    "metadata": {
                        "kind": {
                            "data": {
                                "title": "Demo MP4"
                            }
                        }
                    },
                    "content": {
                        "key": "videos/demo.mp4",
                        "size": 4,
                        "mime_type": "video/mp4",
                        "original_filename": "demo.mp4",
                        "checksum": []
                    },
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                },
                "content_ref": {
                    "encoding": "url",
                    "url": "asset://content/videos/demo.mp4"
                }
            })
            .to_string(),
        )
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["view"], "media");
        assert_eq!(output["mime_type"], "video/mp4");
        assert_eq!(output["title"], "Demo MP4");
        assert_eq!(output["encoding"], "url");
        assert_eq!(
            output["data"],
            "/resources/01900000-0000-7000-8000-000000000000/content"
        );
    }
}
