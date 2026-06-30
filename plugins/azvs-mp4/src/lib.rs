use asset_plugin_api::{
    HtmlView, PluginActionOutput, PluginActionRequest, PluginContentEncoding,
    PluginResource, PluginResourceContent, PluginView,
};
use extism_pdk::*;
use serde_json::{Value, json};

#[plugin_fn]
pub fn play_mp4(input: String) -> FnResult<String> {
    play_mp4_payload(input)
}

fn play_mp4_payload(input: String) -> FnResult<String> {
    let input: PluginActionRequest = serde_json::from_str(&input)?;
    let content = input
        .content
        .ok_or_else(|| Error::msg("missing MP4 content payload"))?;

    if content.encoding != PluginContentEncoding::Base64 {
        return Err(Error::msg("unsupported content encoding").into());
    }

    let title = video_title(&input.resource);
    let mime_type = video_mime_type(input.resource.content.as_ref());
    let html = video_html(&title, &mime_type, &content.data);

    Ok(serde_json::to_string(&PluginActionOutput::new(PluginView::Html(
        HtmlView {
            title: Some(title),
            html,
        },
    )))?)
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

fn video_html(title: &str, mime_type: &str, data_base64: &str) -> String {
    let title = escape_html(title);
    let mime_type = escape_attr(mime_type);

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root {{
      color-scheme: dark;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: #111;
      color: #f7f7f7;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      min-height: 100vh;
      display: grid;
      grid-template-rows: auto 1fr;
      background: #111;
    }}
    header {{
      padding: 16px 20px 12px;
      border-bottom: 1px solid rgba(255, 255, 255, 0.12);
    }}
    h1 {{
      margin: 0;
      font-size: 16px;
      font-weight: 650;
      line-height: 1.35;
    }}
    main {{
      display: grid;
      align-items: center;
      padding: 20px;
    }}
    video {{
      width: 100%;
      max-height: calc(100vh - 94px);
      background: #000;
      outline: none;
    }}
  </style>
</head>
<body>
  <header><h1>{title}</h1></header>
  <main>
    <video controls preload="metadata" src="data:{mime_type};base64,{data_base64}"></video>
  </main>
</body>
</html>"#
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn escape_attr(value: &str) -> String {
    escape_html(value)
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
    fn html_escapes_title() {
        let html = video_html("<Video>", "video/mp4", "AAAA");

        assert!(html.contains("&lt;Video&gt;"));
        assert!(!html.contains("<Video>"));
    }

    #[test]
    fn play_mp4_returns_html_player() {
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
                "content": {
                    "encoding": "base64",
                    "data": "AAAA"
                }
            })
            .to_string(),
        )
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["view"], "html");
        assert_eq!(output["title"], "Demo MP4");
        assert!(output["html"].as_str().unwrap().contains("<video controls"));
        assert!(
            output["html"]
                .as_str()
                .unwrap()
                .contains("data:video/mp4;base64,AAAA")
        );
    }
}
