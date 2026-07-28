use super::*;

#[test]
fn rejects_manifest_with_missing_fields() {
    let path = unique_temp_path("broken-plugin.json");
    std::fs::write(
        &path,
        r#"
        {
          "plugin": {
            "id": "broken",
            "name": "Broken",
            "version": "0.1.0",
            "publisher": "test",
            "description": "Broken manifest."
          },
          "runtime": {
            "type": "builtin"
          },
          "permissions": {
            "allow": ["resource.read", "resource.content.read"],
            "network": false,
            "filesystem": false
          }
        }
        "#,
    )
    .unwrap();

    let error = load_plugin_manifest_file(&path).unwrap_err();

    assert!(format!("{error:?}").contains("missing field `manifest_version`"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn catalog_rejects_duplicate_plugin_ids() {
    let root = unique_temp_path("duplicate-root");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = minimal_builtin_manifest("duplicate.plugin");
    let first = root.join("first.json");
    let second = root.join("second.json");
    std::fs::write(&first, &manifest).unwrap();
    std::fs::write(&second, &manifest).unwrap();

    let error = PluginCatalog::load(&KindRegistryConfig {
        plugin_manifests: vec![first, second],
    })
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("duplicate plugin id `duplicate.plugin`")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn catalog_rejects_a_wasm_digest_mismatch() {
    let root = unique_temp_path("digest-root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("plugin.wasm"), b"actual").unwrap();
    let path = root.join("plugin.json");
    std::fs::write(&path, minimal_extism_manifest("digest.plugin")).unwrap();
    write_wasm_lock(
        &root,
        "digest.plugin",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );

    let error = PluginCatalog::load(&KindRegistryConfig {
        plugin_manifests: vec![path],
    })
    .unwrap_err();

    assert!(error.to_string().contains("Wasm digest mismatch"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn catalog_keeps_the_verified_wasm_snapshot() {
    let root = unique_temp_path("snapshot-root");
    std::fs::create_dir_all(&root).unwrap();
    let original = b"verified wasm bytes";
    std::fs::write(root.join("plugin.wasm"), original).unwrap();
    let digest = format!("{:x}", Sha256::digest(original));
    let path = root.join("plugin.json");
    std::fs::write(&path, minimal_extism_manifest("snapshot.plugin")).unwrap();
    write_wasm_lock(&root, "snapshot.plugin", &digest);

    let catalog = PluginCatalog::load(&KindRegistryConfig {
        plugin_manifests: vec![path],
    })
    .unwrap();
    std::fs::write(root.join("plugin.wasm"), b"changed after startup").unwrap();

    let loaded = catalog
        .plugins()
        .iter()
        .find(|plugin| plugin.manifest.plugin_id() == "snapshot.plugin")
        .unwrap();
    assert_eq!(loaded.wasm.as_deref(), Some(original.as_slice()));
    let _ = std::fs::remove_dir_all(root);
}

fn minimal_builtin_manifest(id: &str) -> String {
    format!(
        r#"{{
          "manifest_version": 1,
          "plugin": {{"id": "{id}", "name": "Test", "version": "0.1.0", "publisher": "test"}},
          "runtime": {{"type": "builtin"}},
          "capabilities": {{"kinds": [], "resource_actions": []}},
          "permissions": {{
            "allow": ["resource.read"],
            "network": false,
            "filesystem": false
          }}
        }}"#
    )
}

fn minimal_extism_manifest(id: &str) -> String {
    format!(
        r#"{{
          "manifest_version": 1,
          "plugin": {{"id": "{id}", "name": "Test", "version": "0.1.0", "publisher": "test"}},
          "runtime": {{
            "type": "extism", "wasm": "plugin.wasm",
            "wasi": false, "plugin_api": "asset-hub.plugin-api@1"
          }},
          "capabilities": {{"kinds": [], "resource_actions": []}},
          "permissions": {{
            "allow": ["resource.read"],
            "network": false,
            "filesystem": false
          }}
        }}"#
    )
}

fn write_wasm_lock(root: &Path, plugin_id: &str, digest: &str) {
    std::fs::write(
        root.join("manifest.lock.json"),
        format!(
            r#"{{
              "manifest_version": 1,
              "plugin_id": "{plugin_id}",
              "runtime": {{
                "wasm_sha256": "{digest}"
              }}
            }}"#
        ),
    )
    .unwrap();
}

fn unique_temp_path(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "asset-hub-plugin-manifest-{}-{name}",
        std::process::id()
    ));
    path
}
