use super::*;
use serde_json::json;

#[test]
fn resource_exposes_description_and_tags_directly() {
    let resource: PluginResource = serde_json::from_value(json!({
        "id": "0198a123-4567-7000-8000-000000000001",
        "directory": "documents",
        "name": "notes.md",
        "kind": "core:document",
        "status": "active",
        "description": "Document",
        "tags": ["docs"],
        "created_at": "2026-07-16T10:00:00Z",
        "updated_at": "2026-07-16T10:05:00Z"
    }))
    .unwrap();

    assert_eq!(resource.description.as_deref(), Some("Document"));
    assert_eq!(resource.tags, ["docs"]);
}
