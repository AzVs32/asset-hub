use super::*;
use serde_json::json;

#[test]
fn resource_metadata_has_an_explicit_schema() {
    let metadata: PluginResourceMetadata = serde_json::from_value(json!({
        "schema_version": 1,
        "summary": {
            "description": "Document",
            "tags": ["docs"]
        }
    }))
    .unwrap();

    assert_eq!(metadata.schema_version, 1);
    assert_eq!(metadata.summary.description.as_deref(), Some("Document"));
    assert_eq!(metadata.summary.tags, ["docs"]);
}

#[test]
fn resource_metadata_rejects_plugin_defined_fields() {
    let result = serde_json::from_value::<PluginResourceMetadata>(json!({
        "schema_version": 1,
        "summary": {
            "description": null,
            "tags": []
        },
        "plugin_data": {}
    }));

    assert!(result.is_err());
}
