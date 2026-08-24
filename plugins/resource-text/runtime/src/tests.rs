use super::*;
use asset_rust_sdk::decode_base64;

#[test]
fn large_text_uses_bounded_chunks() {
    let text = vec![b'a'; CONTENT_CHUNK_BYTES as usize + 17];
    let load = asset_rust_sdk::__private::run_resource_action(
        request_json(
            "resource.text.read",
            json!({"operation": "load"}),
            Some(&text),
        ),
        handle_read_text,
    )
    .unwrap();
    let load: Value = serde_json::from_str(&load).unwrap();
    assert_eq!(load["data"]["transfer"], "chunked");
    assert_eq!(load["data"]["chunk_size"], CONTENT_CHUNK_BYTES);

    let chunk = asset_rust_sdk::__private::run_resource_action(
        request_json(
            "resource.text.read",
            json!({"operation": "chunk", "offset": CONTENT_CHUNK_BYTES}),
            Some(&text),
        ),
        handle_read_text,
    )
    .unwrap();
    let chunk: Value = serde_json::from_str(&chunk).unwrap();
    assert_eq!(chunk["data"]["offset"], CONTENT_CHUNK_BYTES);
    assert_eq!(chunk["data"]["done"], true);
    assert_eq!(
        decode_base64(chunk["data"]["data"].as_str().unwrap()).unwrap(),
        vec![b'a'; 17]
    );
}

#[test]
fn edit_text_rejects_inline_writeback() {
    let error = asset_rust_sdk::__private::run_resource_action(
        request_json("resource.text.edit", json!({"text": "updated"}), None),
        handle_edit_text,
    )
    .unwrap();
    let error: Value = serde_json::from_str(&error).unwrap();
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unsupported text edit operation")
    );
}

#[test]
fn text_format_selects_format_specific_renderers_before_plain_text() {
    assert_eq!(
        detect_text_format("resource:markdown", "README"),
        TextFormat::Markdown
    );
    assert_eq!(
        detect_text_format("core:resource", "README.MD"),
        TextFormat::Markdown
    );
    assert_eq!(
        detect_text_format("resource:mermaid", "diagram"),
        TextFormat::Mermaid
    );
    assert_eq!(
        detect_text_format("core:resource", "architecture.MMD"),
        TextFormat::Mermaid
    );
    assert_eq!(
        detect_text_format("core:resource", "main.cpp"),
        TextFormat::Plain
    );
    assert_eq!(
        detect_text_format("core:resource", "notes.txt"),
        TextFormat::Plain
    );
}

#[test]
fn thumbnail_returns_the_format_specific_static_svg() {
    for (kind, name, marker) in [
        (
            "resource:markdown",
            "README.md",
            "data-thumbnail=\"markdown\"",
        ),
        (
            "resource:mermaid",
            "architecture.mmd",
            "data-thumbnail=\"mermaid\"",
        ),
    ] {
        let output = asset_rust_sdk::__private::run_resource_action(
            thumbnail_request_json(kind, name),
            handle_thumbnail,
        )
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["view"], "media");
        assert_eq!(output["mime_type"], "image/svg+xml");
        assert_eq!(output["encoding"], "base64");
        assert_eq!(output["title"], name);
        let svg = decode_base64(output["data"].as_str().unwrap()).unwrap();
        assert!(String::from_utf8(svg).unwrap().contains(marker));
    }
}

#[test]
fn manifest_keeps_format_kinds_and_shared_text_actions_aligned() {
    let manifest: Value = serde_json::from_str(include_str!("../../manifest.json")).unwrap();
    let resource_kinds = manifest["capabilities"]["resource_kinds"]
        .as_array()
        .unwrap();
    let resource_actions = manifest["capabilities"]["resource_actions"]
        .as_array()
        .unwrap();

    for kind_id in ["resource:markdown", "resource:mermaid"] {
        let kind = resource_kinds
            .iter()
            .find(|kind| kind["kind"] == kind_id)
            .unwrap();
        assert_eq!(kind["parent"], "core:resource");
    }

    for action_id in ["resource.text.read", "resource.text.edit"] {
        let action = resource_actions
            .iter()
            .find(|action| action["id"] == action_id)
            .unwrap();
        for extension in [".md", ".mmd", ".mermaid"] {
            assert!(
                action["applies_to"]["extensions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item == extension)
            );
        }
    }

    let thumbnail = resource_actions
        .iter()
        .find(|action| action["id"] == "resource.text.thumbnail")
        .unwrap();
    assert_eq!(thumbnail["provides"], "thumbnail");
    assert_eq!(thumbnail["output"]["views"][0], "media");
    assert!(
        thumbnail["ui"]["locations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|location| location == "resource_thumbnail")
    );
    for extension in [".md", ".markdown", ".mdown", ".mkd", ".mmd", ".mermaid"] {
        assert!(
            thumbnail["applies_to"]["extensions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == extension)
        );
    }
    assert!(
        !thumbnail["applies_to"]["extensions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == ".txt")
    );
}

fn thumbnail_request_json(kind: &str, name: &str) -> String {
    let mut resource = resource_json();
    resource["kind"] = json!(kind);
    resource["name"] = json!(name);
    json!({
        "action": "resource.text.thumbnail",
        "access": "read",
        "input": {},
        "resource": resource,
    })
    .to_string()
}

fn request_json(action: &str, input: Value, content: Option<&[u8]>) -> String {
    let mut request = json!({
        "action": action,
        "access": if action == "resource.text.edit" { "write" } else { "read" },
        "input": input,
        "resource": resource_json(),
    });
    if let Some(content) = content {
        request["content"] = json!({
            "encoding": "base64",
            "data": encode_base64(content),
        });
    }
    request.to_string()
}

fn resource_json() -> Value {
    json!({
        "id": "01900000-0000-7000-8000-000000000000",
        "directory": "documents",
        "name": "demo.md",
        "kind": "resource:markdown",
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
