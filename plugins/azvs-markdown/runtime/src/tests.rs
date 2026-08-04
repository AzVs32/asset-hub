use super::*;

#[test]
fn initial_frame_contains_only_small_routing_payload() {
    let output =
        render_markdown_payload(request_json("azvs.markdown.render", json!({}), None)).unwrap();
    let output: Value = serde_json::from_str(&output).unwrap();
    let payload = decode_frame_payload(&output);

    assert_eq!(
        payload["plugin_api"],
        asset_plugin_api::protocol::PLUGIN_API_VERSION
    );
    assert_eq!(payload["resource_id"], resource_json()["id"]);
    assert_eq!(payload["mode"], "read");
    assert_eq!(payload["action"], "azvs.markdown.render");
    assert!(payload.get("markdown").is_none());
    assert!(output["url"].as_str().unwrap().len() < 300);
}

#[test]
fn edit_frame_does_not_require_content() {
    let output =
        update_markdown_payload(request_json("azvs.markdown.update", json!({}), None)).unwrap();
    let output: Value = serde_json::from_str(&output).unwrap();
    let payload = decode_frame_payload(&output);
    assert_eq!(payload["mode"], "edit");
    assert!(payload.get("markdown").is_none());
}

#[test]
fn small_markdown_is_returned_directly() {
    let output = render_markdown_payload(request_json(
        "azvs.markdown.render",
        json!({"operation": "load"}),
        Some(b"\xef\xbb\xbf# Title\n\nBody"),
    ))
    .unwrap();
    let output: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(output["view"], "json");
    assert_eq!(output["data"]["transfer"], "complete");
    assert_eq!(output["data"]["markdown"], "# Title\n\nBody");
}

#[test]
fn large_markdown_uses_bounded_chunks() {
    let markdown = vec![b'a'; CONTENT_CHUNK_BYTES as usize + 17];
    let load = render_markdown_payload(request_json(
        "azvs.markdown.render",
        json!({"operation": "load"}),
        Some(&markdown),
    ))
    .unwrap();
    let load: Value = serde_json::from_str(&load).unwrap();
    assert_eq!(load["data"]["transfer"], "chunked");
    assert_eq!(load["data"]["chunk_size"], CONTENT_CHUNK_BYTES);

    let chunk = render_markdown_payload(request_json(
        "azvs.markdown.render",
        json!({"operation": "chunk", "offset": CONTENT_CHUNK_BYTES}),
        Some(&markdown),
    ))
    .unwrap();
    let chunk: Value = serde_json::from_str(&chunk).unwrap();
    assert_eq!(chunk["data"]["offset"], CONTENT_CHUNK_BYTES);
    assert_eq!(chunk["data"]["done"], true);
    assert_eq!(
        STANDARD
            .decode(chunk["data"]["data"].as_str().unwrap())
            .unwrap(),
        vec![b'a'; 17]
    );
}

#[test]
fn update_markdown_returns_replace_content_effect() {
    let output = update_markdown_payload(request_json(
        "azvs.markdown.update",
        json!({"markdown": "# Updated"}),
        None,
    ))
    .unwrap();
    let output: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(output["view"], "text");
    assert_eq!(output["effects"][0]["type"], "replace_content");
    assert_eq!(
        STANDARD
            .decode(output["effects"][0]["data"].as_str().unwrap())
            .unwrap(),
        b"# Updated"
    );
}

fn decode_frame_payload(output: &Value) -> Value {
    let encoded = output["url"]
        .as_str()
        .unwrap()
        .split_once("#payload=")
        .unwrap()
        .1;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap()
}

fn request_json(action: &str, input: Value, content: Option<&[u8]>) -> String {
    let mut request = json!({
        "action": action,
        "access": if action == "azvs.markdown.update" { "read_write" } else { "read_only" },
        "input": input,
        "resource": resource_json(),
    });
    if let Some(content) = content {
        request["content"] = json!({
            "encoding": "base64",
            "data": STANDARD.encode(content),
        });
    }
    request.to_string()
}

fn resource_json() -> Value {
    json!({
        "id": "01900000-0000-7000-8000-000000000000",
        "directory": "documents",
        "name": "demo.md",
        "kind": "azvs:markdown",
        "tags": [],
        "content": {
            "size": 4,
            "mime_type": "text/markdown",
            "verification_status": "verified",
            "checksum": {
                "kind": "sha256",
                "value": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
            }
        },
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    })
}
