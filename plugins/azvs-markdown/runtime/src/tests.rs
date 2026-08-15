use super::*;

#[test]
fn large_markdown_uses_bounded_chunks() {
    let markdown = vec![b'a'; CONTENT_CHUNK_BYTES as usize + 17];
    let load = render_markdown_payload(request_json(
        "azvs.markdown.read",
        json!({"operation": "load"}),
        Some(&markdown),
    ))
    .unwrap();
    let load: Value = serde_json::from_str(&load).unwrap();
    assert_eq!(load["data"]["transfer"], "chunked");
    assert_eq!(load["data"]["chunk_size"], CONTENT_CHUNK_BYTES);

    let chunk = render_markdown_payload(request_json(
        "azvs.markdown.read",
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
fn update_markdown_rejects_legacy_inline_writeback() {
    let error = update_markdown_payload(request_json(
        "azvs.markdown.edit",
        json!({"markdown": "# Updated"}),
        None,
    ))
    .unwrap_err();
    assert!(
        error
            .0
            .to_string()
            .contains("unsupported Markdown edit operation")
    );
}

fn request_json(action: &str, input: Value, content: Option<&[u8]>) -> String {
    let mut request = json!({
        "action": action,
        "access": if action == "azvs.markdown.edit" { "write" } else { "read" },
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
        "revision": 1,
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
