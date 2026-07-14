use crate::action::builtin;
use crate::config::KindRegistryConfig;
use crate::official_plugins;
use asset_core::CoreError;
use asset_plugin_api::{ActionExecutor, PluginManifest, PluginRuntime};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_PLUGIN_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_PLUGIN_WASM_BYTES: usize = 64 * 1024 * 1024;
const MAX_PLUGIN_WEB_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct LoadedPlugin {
    pub(crate) manifest: PluginManifest,
    pub(crate) manifest_path: Option<PathBuf>,
    pub(crate) wasm: Option<Arc<[u8]>>,
    pub(crate) web_assets: HashMap<PathBuf, Arc<[u8]>>,
}

#[derive(Debug, Clone)]
pub(crate) struct PluginCatalog {
    plugins: Vec<LoadedPlugin>,
}

impl PluginCatalog {
    pub(crate) fn load(config: &KindRegistryConfig) -> Result<Self, CoreError> {
        let mut plugins = Vec::new();
        for content in official_plugins::MANIFESTS {
            let manifest: PluginManifest = serde_json::from_str(content).map_err(|error| {
                CoreError::configuration(format!("parse official plugin manifest: {error}"))
            })?;
            let artifacts = validate_loaded_manifest(&manifest, None)?;
            plugins.push(LoadedPlugin {
                manifest,
                manifest_path: None,
                wasm: artifacts.wasm,
                web_assets: artifacts.web_assets,
            });
        }
        for path in &config.plugin_manifests {
            let manifest = load_plugin_manifest_file(path)?;
            let artifacts = validate_loaded_manifest(&manifest, Some(path))?;
            plugins.push(LoadedPlugin {
                manifest,
                manifest_path: Some(path.clone()),
                wasm: artifacts.wasm,
                web_assets: artifacts.web_assets,
            });
        }

        let mut ids = HashSet::new();
        for plugin in &plugins {
            if !ids.insert(plugin.manifest.plugin_id()) {
                return Err(CoreError::configuration(format!(
                    "duplicate plugin id `{}`",
                    plugin.manifest.plugin_id()
                )));
            }
        }
        Ok(Self { plugins })
    }

    pub(crate) fn plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }
}

pub(crate) fn load_plugin_manifest_file(path: &Path) -> Result<PluginManifest, CoreError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        CoreError::configuration(format!(
            "inspect plugin manifest `{}`: {error}",
            path.display()
        ))
    })?;
    if metadata.len() > MAX_PLUGIN_MANIFEST_BYTES {
        return Err(CoreError::configuration(format!(
            "plugin manifest `{}` exceeds the {MAX_PLUGIN_MANIFEST_BYTES} byte limit",
            path.display()
        )));
    }
    let content = std::fs::read_to_string(path).map_err(|error| {
        CoreError::configuration(format!(
            "read plugin manifest `{}`: {error}",
            path.display()
        ))
    })?;
    let manifest: PluginManifest = serde_json::from_str(&content).map_err(|error| {
        CoreError::configuration(format!(
            "parse plugin manifest `{}`: {error}",
            path.display()
        ))
    })?;
    manifest.validate().map_err(|error| {
        CoreError::configuration(format!(
            "invalid plugin manifest `{}`: {error}",
            path.display()
        ))
    })?;

    Ok(manifest)
}

#[derive(Debug, Default)]
struct LoadedArtifacts {
    wasm: Option<Arc<[u8]>>,
    web_assets: HashMap<PathBuf, Arc<[u8]>>,
}

fn validate_loaded_manifest(
    manifest: &PluginManifest,
    manifest_path: Option<&PathBuf>,
) -> Result<LoadedArtifacts, CoreError> {
    manifest.validate().map_err(|error| {
        let source = manifest_path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| format!("official plugin `{}`", manifest.plugin_id()));
        CoreError::configuration(format!("invalid plugin manifest `{source}`: {error}"))
    })?;

    for action in &manifest.capabilities.resource_actions {
        if let Some(ActionExecutor::Builtin { handler }) = &action.executor
            && !builtin::is_builtin_handler(Some(handler))
        {
            return Err(CoreError::configuration(format!(
                "plugin `{}` declares unknown builtin handler `{handler}`",
                manifest.plugin_id()
            )));
        }
    }

    let Some(manifest_path) = manifest_path else {
        return Ok(LoadedArtifacts::default());
    };
    let mut artifacts = LoadedArtifacts::default();
    let resolve = |configured: &Path| {
        if configured.is_absolute() {
            configured.to_path_buf()
        } else {
            manifest_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(configured)
        }
    };
    if let PluginRuntime::Extism {
        wasm, wasm_sha256, ..
    } = &manifest.runtime
    {
        let wasm_path = resolve(wasm);
        let metadata = std::fs::symlink_metadata(&wasm_path).map_err(|error| {
            CoreError::configuration(format!(
                "inspect plugin `{}` Wasm `{}`: {error}",
                manifest.plugin_id(),
                wasm_path.display()
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(CoreError::configuration(format!(
                "plugin `{}` Wasm `{}` must be a regular file",
                manifest.plugin_id(),
                wasm_path.display()
            )));
        }
        let bytes = std::fs::read(&wasm_path).map_err(|error| {
            CoreError::configuration(format!(
                "read plugin `{}` Wasm `{}`: {error}",
                manifest.plugin_id(),
                wasm_path.display()
            ))
        })?;
        if bytes.len() > MAX_PLUGIN_WASM_BYTES {
            return Err(CoreError::configuration(format!(
                "plugin `{}` Wasm exceeds the {MAX_PLUGIN_WASM_BYTES} byte limit",
                manifest.plugin_id()
            )));
        }
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if &actual != wasm_sha256 {
            return Err(CoreError::configuration(format!(
                "plugin `{}` Wasm digest mismatch: expected `{wasm_sha256}`, got `{actual}`",
                manifest.plugin_id()
            )));
        }
        artifacts.wasm = Some(Arc::from(bytes));
    }
    if let Some(web) = &manifest.web {
        let root = resolve(&web.root);
        if !root.is_dir() {
            return Err(CoreError::configuration(format!(
                "plugin `{}` Web root `{}` is not a directory",
                manifest.plugin_id(),
                root.display()
            )));
        }
        let mut actual_paths = HashSet::new();
        let mut total_web_bytes = 0usize;
        collect_web_files(&root, &root, &mut actual_paths)?;
        for (relative_path, expected) in &web.integrity {
            let path = root.join(relative_path);
            let bytes = std::fs::read(&path).map_err(|error| {
                CoreError::configuration(format!(
                    "read plugin `{}` Web asset `{}`: {error}",
                    manifest.plugin_id(),
                    path.display()
                ))
            })?;
            total_web_bytes = total_web_bytes.checked_add(bytes.len()).ok_or_else(|| {
                CoreError::configuration(format!(
                    "plugin `{}` Web assets exceed the host size limit",
                    manifest.plugin_id()
                ))
            })?;
            if total_web_bytes > MAX_PLUGIN_WEB_BYTES {
                return Err(CoreError::configuration(format!(
                    "plugin `{}` Web assets exceed the {MAX_PLUGIN_WEB_BYTES} byte limit",
                    manifest.plugin_id()
                )));
            }
            let actual = format!("{:x}", Sha256::digest(&bytes));
            if &actual != expected {
                return Err(CoreError::configuration(format!(
                    "plugin `{}` Web asset `{}` digest mismatch",
                    manifest.plugin_id(),
                    relative_path.display()
                )));
            }
            artifacts
                .web_assets
                .insert(relative_path.clone(), Arc::from(bytes));
            actual_paths.remove(relative_path);
        }
        if !actual_paths.is_empty() {
            return Err(CoreError::configuration(format!(
                "plugin `{}` Web root contains undeclared files: {actual_paths:?}",
                manifest.plugin_id()
            )));
        }
    }
    Ok(artifacts)
}

fn collect_web_files(
    root: &Path,
    directory: &Path,
    files: &mut HashSet<PathBuf>,
) -> Result<(), CoreError> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| CoreError::configuration(format!("read Web root: {error}")))?
    {
        let entry =
            entry.map_err(|error| CoreError::configuration(format!("read Web asset: {error}")))?;
        let file_type = entry
            .file_type()
            .map_err(|error| CoreError::configuration(format!("inspect Web asset: {error}")))?;
        if file_type.is_symlink() {
            return Err(CoreError::configuration(format!(
                "plugin Web asset `{}` must not be a symbolic link",
                entry.path().display()
            )));
        }
        if file_type.is_dir() {
            collect_web_files(root, &entry.path(), files)?;
        } else if file_type.is_file() {
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|error| {
                CoreError::configuration(format!("resolve Web asset path: {error}"))
            })?;
            files.insert(relative.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
                "resource": {
                  "read": true,
                  "write": false
                },
                "content": {
                  "read": true,
                  "write": false
                },
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
            definitions: Vec::new(),
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
        std::fs::write(
            &path,
            minimal_extism_manifest(
                "digest.plugin",
                "0000000000000000000000000000000000000000000000000000000000000000",
            ),
        )
        .unwrap();

        let error = PluginCatalog::load(&KindRegistryConfig {
            definitions: Vec::new(),
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
        std::fs::write(&path, minimal_extism_manifest("snapshot.plugin", &digest)).unwrap();

        let catalog = PluginCatalog::load(&KindRegistryConfig {
            definitions: Vec::new(),
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
              "manifest_version": 2,
              "plugin": {{"id": "{id}", "name": "Test", "version": "0.1.0", "publisher": "test"}},
              "runtime": {{"type": "builtin"}},
              "capabilities": {{"resource_kinds": [], "resource_actions": []}},
              "permissions": {{
                "resource": {{"read": true, "write": false}},
                "content": {{"read": false, "write": false}},
                "network": false,
                "filesystem": false
              }}
            }}"#
        )
    }

    fn minimal_extism_manifest(id: &str, digest: &str) -> String {
        format!(
            r#"{{
              "manifest_version": 2,
              "plugin": {{"id": "{id}", "name": "Test", "version": "0.1.0", "publisher": "test"}},
              "runtime": {{
                "type": "extism", "wasm": "plugin.wasm", "wasm_sha256": "{digest}",
                "wasi": false, "plugin_api": "asset-hub.plugin-api@0.1"
              }},
              "capabilities": {{"resource_kinds": [], "resource_actions": []}},
              "permissions": {{
                "resource": {{"read": true, "write": false}},
                "content": {{"read": false, "write": false}},
                "network": false,
                "filesystem": false
              }}
            }}"#
        )
    }

    fn unique_temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "asset-hub-plugin-manifest-{}-{name}",
            std::process::id()
        ));
        path
    }
}
