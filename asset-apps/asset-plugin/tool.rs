use asset_plugin_api::{
    MANIFEST_TEMPLATE, PluginCapabilities, PluginManifest, PluginMetadata, PluginPermissions,
    PluginRuntime, PluginWeb,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub struct ToolError(String);

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ToolError {}

type Result<T> = std::result::Result<T, ToolError>;

pub fn generate_manifest(path: &Path) -> Result<PluginManifest> {
    let document: serde_json::Value = serde_json::from_str(MANIFEST_TEMPLATE)
        .map_err(|error| ToolError(format!("parse embedded manifest template: {error}")))?;
    let draft: DraftManifest = serde_json::from_value(document)
        .map_err(|error| ToolError(format!("parse embedded manifest template: {error}")))?;
    let manifest = manifest_for_draft_validation(draft);
    validate_contract(&manifest)?;
    write_new_file(path, MANIFEST_TEMPLATE.as_bytes())?;
    Ok(manifest)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftManifest {
    manifest_version: u32,
    plugin: PluginMetadata,
    runtime: DraftRuntime,
    #[serde(default)]
    web: Option<DraftWeb>,
    #[serde(default)]
    capabilities: PluginCapabilities,
    permissions: PluginPermissions,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum DraftRuntime {
    Builtin,
    Extism {
        wasm: PathBuf,
        #[serde(default, rename = "wasm_sha256")]
        _wasm_sha256: Option<String>,
        #[serde(default)]
        wasi: bool,
        plugin_api: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftWeb {
    root: PathBuf,
    #[serde(default, rename = "integrity")]
    _integrity: BTreeMap<PathBuf, String>,
}

fn manifest_for_draft_validation(draft: DraftManifest) -> PluginManifest {
    let runtime = match draft.runtime {
        DraftRuntime::Builtin => PluginRuntime::Builtin,
        DraftRuntime::Extism {
            wasm,
            wasi,
            plugin_api,
            ..
        } => PluginRuntime::Extism {
            wasm,
            wasm_sha256: "0".repeat(64),
            wasi,
            plugin_api,
        },
    };
    let web = draft.web.map(|web| PluginWeb {
        root: web.root,
        integrity: BTreeMap::from([(PathBuf::from("index.html"), "0".repeat(64))]),
    });
    PluginManifest {
        manifest_version: draft.manifest_version,
        plugin: draft.plugin,
        runtime,
        web,
        capabilities: draft.capabilities,
        permissions: draft.permissions,
    }
}

pub fn seal_manifest(path: &Path) -> Result<PluginManifest> {
    let mut document: serde_json::Value = read_json(path)?;
    let draft: DraftManifest = serde_json::from_value(document.clone())
        .map_err(|error| ToolError(format!("parse `{}`: {error}", path.display())))?;
    let base = manifest_base(path);
    let runtime = match draft.runtime {
        DraftRuntime::Builtin => PluginRuntime::Builtin,
        DraftRuntime::Extism {
            wasm,
            wasi,
            plugin_api,
            ..
        } => {
            let wasm_sha256 = digest_file(&resolve_artifact(&base, "runtime.wasm", &wasm)?)?;
            PluginRuntime::Extism {
                wasm,
                wasm_sha256,
                wasi,
                plugin_api,
            }
        }
    };
    let web = draft
        .web
        .map(|web| {
            let root = resolve_artifact(&base, "web.root", &web.root)?;
            let integrity = web_integrity(&root)?;
            Ok(PluginWeb {
                root: web.root,
                integrity,
            })
        })
        .transpose()?;
    let manifest = PluginManifest {
        manifest_version: draft.manifest_version,
        plugin: draft.plugin,
        runtime,
        web,
        capabilities: draft.capabilities,
        permissions: draft.permissions,
    };
    validate_contract(&manifest)?;
    if let PluginRuntime::Extism { wasm_sha256, .. } = &manifest.runtime {
        document["runtime"]["wasm_sha256"] = serde_json::Value::String(wasm_sha256.clone());
    }
    if let Some(web) = &manifest.web {
        document["web"]["integrity"] = serde_json::to_value(&web.integrity)
            .map_err(|error| ToolError(format!("encode Web integrity: {error}")))?;
    }
    write_json_atomically(path, &document)?;
    Ok(manifest)
}

pub fn verify_manifest(path: &Path) -> Result<PluginManifest> {
    let manifest: PluginManifest = read_json(path)?;
    validate_contract(&manifest)?;
    verify_wasm(&manifest, path)?;
    verify_web(&manifest, path)?;
    Ok(manifest)
}

pub fn verify_wasm_manifest(path: &Path) -> Result<PluginManifest> {
    let manifest: PluginManifest = read_json(path)?;
    validate_contract(&manifest)?;
    verify_wasm(&manifest, path)?;
    Ok(manifest)
}

pub fn verify_web_manifest(path: &Path) -> Result<PluginManifest> {
    let manifest: PluginManifest = read_json(path)?;
    validate_contract(&manifest)?;
    verify_web(&manifest, path)?;
    Ok(manifest)
}

fn verify_wasm(manifest: &PluginManifest, path: &Path) -> Result<()> {
    let base = manifest_base(path);
    if let PluginRuntime::Extism {
        wasm, wasm_sha256, ..
    } = &manifest.runtime
    {
        let actual = digest_file(&resolve_artifact(&base, "runtime.wasm", wasm)?)?;
        if &actual != wasm_sha256 {
            return Err(ToolError(format!(
                "Wasm digest mismatch: manifest={wasm_sha256} actual={actual}"
            )));
        }
    }
    Ok(())
}

fn verify_web(manifest: &PluginManifest, path: &Path) -> Result<()> {
    let base = manifest_base(path);
    if let Some(web) = &manifest.web {
        let actual = web_integrity(&resolve_artifact(&base, "web.root", &web.root)?)?;
        if actual != web.integrity {
            return Err(ToolError(web_integrity_difference(&web.integrity, &actual)));
        }
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| ToolError(format!("read `{}`: {error}", path.display())))?;
    serde_json::from_str(&content)
        .map_err(|error| ToolError(format!("parse `{}`: {error}", path.display())))
}

fn validate_contract(manifest: &PluginManifest) -> Result<()> {
    manifest
        .validate()
        .map_err(|error| ToolError(format!("invalid Manifest V2: {error}")))
}

fn manifest_base(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn resolve_artifact(base: &Path, field: &str, path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ToolError(format!("{field} must be a safe relative path")));
    }
    Ok(base.join(path))
}

fn digest_file(path: &Path) -> Result<String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| ToolError(format!("inspect artifact `{}`: {error}", path.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ToolError(format!(
            "artifact `{}` must be a regular file",
            path.display()
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| ToolError(format!("read artifact `{}`: {error}", path.display())))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn web_integrity(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    if !root.is_dir() {
        return Err(ToolError(format!(
            "Web root `{}` is not a directory",
            root.display()
        )));
    }
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    let mut integrity = BTreeMap::new();
    for (relative, absolute) in files {
        integrity.insert(relative, digest_file(&absolute)?);
    }
    Ok(integrity)
}

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        ToolError(format!(
            "read Web directory `{}`: {error}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| ToolError(format!("read Web entry: {error}")))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            ToolError(format!("inspect Web asset `{}`: {error}", path.display()))
        })?;
        if file_type.is_symlink() {
            return Err(ToolError(format!(
                "symbolic links are not allowed in Web roots: `{}`",
                path.display()
            )));
        }
        if file_type.is_dir() {
            collect_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| ToolError(format!("resolve Web asset path: {error}")))?
                .to_path_buf();
            files.push((relative, path));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(())
}

fn web_integrity_difference(
    expected: &BTreeMap<PathBuf, String>,
    actual: &BTreeMap<PathBuf, String>,
) -> String {
    let expected_files = expected.keys().cloned().collect::<BTreeSet<_>>();
    let actual_files = actual.keys().cloned().collect::<BTreeSet<_>>();
    let missing = expected_files.difference(&actual_files).collect::<Vec<_>>();
    let undeclared = actual_files.difference(&expected_files).collect::<Vec<_>>();
    let changed = expected_files
        .intersection(&actual_files)
        .filter(|path| expected.get(*path) != actual.get(*path))
        .collect::<Vec<_>>();
    format!(
        "Web integrity mismatch: missing={missing:?} undeclared={undeclared:?} changed={changed:?}"
    )
}

fn write_json_atomically(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| ToolError(format!("encode `{}`: {error}", path.display())))?;
    bytes.push(b'\n');
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ToolError(format!("invalid manifest path `{}`", path.display())))?;
    let temporary = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let permissions = std::fs::metadata(path)
        .map_err(|error| ToolError(format!("inspect `{}`: {error}", path.display())))?
        .permissions();
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| ToolError(format!("create `{}`: {error}", temporary.display())))?;
        file.set_permissions(permissions).map_err(|error| {
            ToolError(format!(
                "set permissions on `{}`: {error}",
                temporary.display()
            ))
        })?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| ToolError(format!("write `{}`: {error}", temporary.display())))?;
        std::fs::rename(&temporary, path).map_err(|error| {
            ToolError(format!(
                "replace `{}` with `{}`: {error}",
                path.display(),
                temporary.display()
            ))
        })?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| ToolError(format!("create `{}`: {error}", path.display())))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(ToolError(format!("write `{}`: {error}", path.display())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn seal_generates_wasm_and_web_integrity() {
        let root = test_root("seal");
        std::fs::create_dir_all(root.join("web/nested")).unwrap();
        std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
        std::fs::write(root.join("web/index.html"), b"html").unwrap();
        std::fs::write(root.join("web/nested/viewer.js"), b"js").unwrap();
        let manifest_path = root.join("plugin.json");
        std::fs::write(&manifest_path, draft_manifest()).unwrap();

        let sealed = seal_manifest(&manifest_path).unwrap();
        verify_manifest(&manifest_path).unwrap();
        seal_manifest(&manifest_path).unwrap();
        verify_manifest(&manifest_path).unwrap();

        let PluginRuntime::Extism { wasm_sha256, .. } = sealed.runtime else {
            panic!("expected Extism runtime");
        };
        assert_eq!(wasm_sha256, digest_file(&root.join("plugin.wasm")).unwrap());
        assert_eq!(sealed.web.unwrap().integrity.len(), 2);
        let document: serde_json::Value = read_json(&manifest_path).unwrap();
        assert!(document["runtime"].get("wasm_sha256").is_some());
        assert!(document["runtime"].get("wasi").is_none());
        assert!(document["plugin"].get("description").is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn generate_writes_a_seal_ready_manifest_without_generated_hashes() {
        let root = test_root("generate");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("manifest.json");

        let generated = generate_manifest(&path).unwrap();
        let document: serde_json::Value = read_json(&path).unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), MANIFEST_TEMPLATE);
        assert_eq!(generated.plugin_id(), "example.plugin");
        assert_eq!(
            document["manifest_version"],
            asset_plugin_api::MANIFEST_VERSION
        );
        assert_eq!(document["runtime"]["wasm"], "dist/plugin.wasm");
        assert!(document["runtime"].get("wasm_sha256").is_none());
        assert_eq!(
            document["capabilities"]["resource_actions"][0]["executor"]["handler"],
            "run"
        );
        assert!(generate_manifest(&path).is_err());
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist/plugin.wasm"), b"wasm").unwrap();
        seal_manifest(&path).unwrap();
        verify_manifest(&path).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_detects_artifact_changes_without_modifying_manifest() {
        let root = test_root("verify");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("plugin.wasm"), b"wasm").unwrap();
        let manifest_path = root.join("plugin.json");
        std::fs::write(&manifest_path, draft_manifest_without_web()).unwrap();
        seal_manifest(&manifest_path).unwrap();
        let sealed = std::fs::read(&manifest_path).unwrap();

        std::fs::write(root.join("plugin.wasm"), b"changed").unwrap();
        assert!(verify_manifest(&manifest_path).is_err());
        assert_eq!(std::fs::read(&manifest_path).unwrap(), sealed);
        let _ = std::fs::remove_dir_all(root);
    }

    fn draft_manifest() -> String {
        draft_manifest_with_web(r#", "web": {"root": "web"}"#)
    }

    fn draft_manifest_without_web() -> String {
        draft_manifest_with_web("")
    }

    fn draft_manifest_with_web(web: &str) -> String {
        format!(
            r#"{{
              "manifest_version": 2,
              "plugin": {{"id": "test.plugin", "name": "Test", "version": "0.1.0", "publisher": "test"}},
              "runtime": {{"type": "extism", "wasm": "plugin.wasm", "plugin_api": "asset-hub.plugin-api@0.1"}},
              "capabilities": {{"resource_kinds": [], "resource_actions": []}},
              "permissions": {{
                "resource": {{"read": true, "write": false}},
                "content": {{"read": false, "write": false}},
                "network": false,
                "filesystem": false
              }}{web}
            }}"#
        )
    }

    fn test_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "asset-plugin-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
