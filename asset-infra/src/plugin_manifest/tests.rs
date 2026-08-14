use super::*;
use asset_plugin_api::manifest::MANIFEST_VERSION;

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
            "publisher": "test"
          },
          "runtime": {
            "type": "extism",
            "plugin_api": "asset-hub.plugin-api@5"
          },
          "permissions": {"allow": ["resource.read"]}
        }
        "#,
    )
    .unwrap();

    let error = load_plugin_manifest_file(&path).unwrap_err();

    assert!(format!("{error:?}").contains("missing field `manifest_version`"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn external_package_rejects_host_owned_builtin_runtime() {
    let root = unique_temp_path("builtin-runtime");
    let package = root.join("invalid.builtin");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join(PLUGIN_MANIFEST_FILE_NAME),
        r#"
        {
          "manifest_version": 3,
          "plugin": {
            "id": "invalid.builtin",
            "name": "Invalid Builtin",
            "version": "0.1.0",
            "publisher": "test"
          },
          "runtime": {"type": "builtin"},
          "capabilities": {},
          "permissions": {"allow": ["resource.read"]}
        }
        "#,
    )
    .unwrap();

    let error = PluginCatalog::load(&root).unwrap_err();

    assert!(error.to_string().contains("unknown variant `builtin`"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn catalog_discovers_packages_and_rejects_directory_id_mismatch() {
    let root = unique_temp_path("directory-id");
    let package = root.join("wrong-name");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join(PLUGIN_MANIFEST_FILE_NAME),
        minimal_extism_manifest("actual.name"),
    )
    .unwrap();
    std::fs::write(package.join(PLUGIN_WASM_FILE_NAME), []).unwrap();
    write_lock(
        &package,
        "actual.name",
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        None,
    );

    let error = PluginCatalog::load(&root).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("must match plugin.id `actual.name`")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn catalog_rejects_a_wasm_digest_mismatch() {
    let root = unique_temp_path("digest-root");
    let package = create_package(
        &root,
        "digest.plugin",
        minimal_extism_manifest("digest.plugin"),
    );
    std::fs::write(package.join(PLUGIN_WASM_FILE_NAME), b"actual").unwrap();
    write_lock(
        &package,
        "digest.plugin",
        Some("0000000000000000000000000000000000000000000000000000000000000000"),
        None,
    );

    let error = PluginCatalog::load(&root).unwrap_err();

    assert!(error.to_string().contains("Wasm digest mismatch"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn generation_and_loading_share_the_wasm_size_limit() {
    let root = unique_temp_path("wasm-limit");
    let package = create_package(
        &root,
        "large.plugin",
        minimal_extism_manifest("large.plugin"),
    );
    let wasm_path = package.join(PLUGIN_WASM_FILE_NAME);
    std::fs::File::create(&wasm_path)
        .unwrap()
        .set_len(MAX_PLUGIN_WASM_BYTES as u64 + 1)
        .unwrap();
    let generate_error = generate_plugin_manifest_lock(&package).unwrap_err();
    write_lock(&package, "large.plugin", Some(&"0".repeat(64)), None);
    let load_error = load_verified_plugin_package(&package).unwrap_err();

    let limit = format!("{MAX_PLUGIN_WASM_BYTES} byte limit");
    assert!(generate_error.to_string().contains(&limit));
    assert!(load_error.to_string().contains(&limit));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn catalog_requires_an_explicitly_generated_lock_and_then_only_verifies_it() {
    let root = unique_temp_path("generated-lock");
    let package = create_package(
        &root,
        "generated.lock",
        minimal_extism_manifest("generated.lock"),
    );
    std::fs::write(package.join(PLUGIN_WASM_FILE_NAME), b"original wasm").unwrap();
    std::fs::write(
        package.join(PLUGIN_WEB_ENTRY_FILE_NAME),
        b"<!doctype html><title>Generated</title>",
    )
    .unwrap();
    let temporary_lock_name = format!(".{PLUGIN_LOCK_FILE_NAME}.concurrent.tmp");
    std::fs::write(package.join(&temporary_lock_name), b"in progress").unwrap();
    let lock_path = package.join(PLUGIN_LOCK_FILE_NAME);

    let error = PluginCatalog::load(&root).unwrap_err();
    assert!(error.to_string().contains("manifest.lock.json"));
    assert!(!lock_path.exists());

    generate_plugin_manifest_lock(&package).unwrap();
    load_verified_plugin_package(&package).unwrap();
    let manifest_path_error =
        load_verified_plugin_package(&package.join(PLUGIN_MANIFEST_FILE_NAME)).unwrap_err();
    assert!(
        manifest_path_error
            .to_string()
            .contains("must be a directory")
    );
    PluginCatalog::load(&root).unwrap();

    let generated = std::fs::read(&lock_path).unwrap();
    let lock: PluginManifestLock = serde_json::from_slice(&generated).unwrap();
    assert_eq!(lock.manifest_version, MANIFEST_VERSION);
    assert_eq!(lock.plugin_id, "generated.lock");
    assert!(
        lock.integrity
            .contains_key(Path::new(PLUGIN_WASM_FILE_NAME))
    );
    assert!(
        lock.integrity
            .contains_key(Path::new(PLUGIN_WEB_ENTRY_FILE_NAME))
    );
    assert!(!lock.integrity.contains_key(Path::new(&temporary_lock_name)));

    std::fs::write(package.join(PLUGIN_WASM_FILE_NAME), b"changed wasm").unwrap();
    let error = PluginCatalog::load(&root).unwrap_err();
    assert!(error.to_string().contains("Wasm digest mismatch"));
    assert_eq!(std::fs::read(lock_path).unwrap(), generated);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn atomic_lock_install_does_not_replace_an_existing_file() {
    let root = unique_temp_path("atomic-no-replace");
    std::fs::create_dir_all(&root).unwrap();
    let lock_path = root.join(PLUGIN_LOCK_FILE_NAME);
    let existing = b"existing lock\n";
    std::fs::write(&lock_path, existing).unwrap();
    let lock = PluginManifestLock {
        manifest_version: 1,
        plugin_id: "concurrent.plugin".to_string(),
        integrity: BTreeMap::new(),
    };

    assert!(!write_json_atomically(&lock_path, &lock).unwrap());
    assert_eq!(std::fs::read(&lock_path).unwrap(), existing);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn catalog_keeps_verified_wasm_and_web_snapshots() {
    let root = unique_temp_path("snapshot-root");
    let package = create_package(
        &root,
        "snapshot.plugin",
        minimal_extism_manifest("snapshot.plugin"),
    );
    let original_wasm = b"verified wasm bytes";
    let original_html = b"<!doctype html><title>Plugin</title>";
    std::fs::write(package.join(PLUGIN_WASM_FILE_NAME), original_wasm).unwrap();
    std::fs::write(package.join(PLUGIN_WEB_ENTRY_FILE_NAME), original_html).unwrap();
    std::fs::create_dir(package.join("static-files")).unwrap();
    std::fs::write(package.join("static-files/app.js"), b"export {};").unwrap();
    let wasm_digest = sha256_hex(original_wasm);
    let web = HashMap::from([
        (PLUGIN_WEB_ENTRY_FILE_NAME, sha256_hex(original_html)),
        ("static-files/app.js", sha256_hex(b"export {};")),
    ]);
    write_lock(&package, "snapshot.plugin", Some(&wasm_digest), Some(&web));

    let catalog = PluginCatalog::load(&root).unwrap();
    std::fs::write(package.join(PLUGIN_WASM_FILE_NAME), b"changed").unwrap();
    std::fs::write(package.join(PLUGIN_WEB_ENTRY_FILE_NAME), b"changed").unwrap();

    let loaded = catalog
        .plugins()
        .iter()
        .find(|plugin| plugin.manifest.plugin_id() == "snapshot.plugin")
        .unwrap();
    assert_eq!(loaded.wasm.as_ref(), original_wasm);
    assert_eq!(
        loaded.web_assets[Path::new(PLUGIN_WEB_ENTRY_FILE_NAME)].as_ref(),
        original_html
    );
    assert!(
        loaded
            .web_assets
            .contains_key(Path::new("static-files/app.js"))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn catalog_rejects_web_assets_without_root_index() {
    let root = unique_temp_path("missing-index");
    let package = create_package(&root, "assets.only", minimal_extism_manifest("assets.only"));
    std::fs::write(package.join(PLUGIN_WASM_FILE_NAME), []).unwrap();
    std::fs::write(package.join("viewer.html"), b"viewer").unwrap();
    write_lock(
        &package,
        "assets.only",
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        None,
    );

    let error = PluginCatalog::load(&root).unwrap_err();

    assert!(error.to_string().contains("index.html"));
    let _ = std::fs::remove_dir_all(root);
}

fn create_package(root: &Path, id: &str, manifest: String) -> PathBuf {
    let package = root.join(id);
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join(PLUGIN_MANIFEST_FILE_NAME), manifest).unwrap();
    package
}

fn minimal_extism_manifest(id: &str) -> String {
    format!(
        r#"{{
          "manifest_version": 3,
          "plugin": {{"id": "{id}", "name": "Test", "version": "0.1.0", "publisher": "test"}},
          "runtime": {{
            "type": "extism", "wasi": false, "plugin_api": "asset-hub.plugin-api@5"
          }},
          "capabilities": {{"resource_kinds": [], "resource_actions": []}},
          "permissions": {{"allow": ["resource.read"]}}
        }}"#
    )
}

fn write_lock(
    root: &Path,
    plugin_id: &str,
    wasm_digest: Option<&str>,
    web: Option<&HashMap<&str, String>>,
) {
    let mut integrity = BTreeMap::new();
    if let Some(digest) = wasm_digest {
        integrity.insert(PLUGIN_WASM_FILE_NAME, digest.to_string());
    }
    if let Some(web) = web {
        integrity.extend(web.iter().map(|(path, digest)| (*path, digest.to_string())));
    }
    std::fs::write(
        root.join(PLUGIN_LOCK_FILE_NAME),
        serde_json::to_vec(&serde_json::json!({
            "manifest_version": 3,
            "plugin_id": plugin_id,
            "integrity": integrity,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "asset-hub-plugin-manifest-{name}-{}",
        uuid::Uuid::now_v7()
    ))
}
