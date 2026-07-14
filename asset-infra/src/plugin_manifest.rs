use crate::action::builtin;
use crate::config::KindRegistryConfig;
use crate::official_plugins;
use asset_core::CoreError;
use asset_plugin_api::{ActionExecutor, PluginManifest, PluginRuntime};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct LoadedPlugin {
    pub(crate) manifest: PluginManifest,
    pub(crate) manifest_path: Option<PathBuf>,
}

impl LoadedPlugin {
    pub(crate) fn resolve_path(&self, configured_path: &Path) -> Option<PathBuf> {
        let manifest_path = self.manifest_path.as_ref()?;
        Some(if configured_path.is_absolute() {
            configured_path.to_path_buf()
        } else {
            manifest_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(configured_path)
        })
    }
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
            validate_loaded_manifest(&manifest, None)?;
            plugins.push(LoadedPlugin {
                manifest,
                manifest_path: None,
            });
        }
        for path in &config.plugin_manifests {
            let manifest = load_plugin_manifest_file(path)?;
            validate_loaded_manifest(&manifest, Some(path))?;
            plugins.push(LoadedPlugin {
                manifest,
                manifest_path: Some(path.clone()),
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

fn validate_loaded_manifest(
    manifest: &PluginManifest,
    manifest_path: Option<&PathBuf>,
) -> Result<(), CoreError> {
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
        return Ok(());
    };
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
        let actual = format!("{:x}", Sha256::digest(bytes));
        if &actual != wasm_sha256 {
            return Err(CoreError::configuration(format!(
                "plugin `{}` Wasm digest mismatch: expected `{wasm_sha256}`, got `{actual}`",
                manifest.plugin_id()
            )));
        }
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
            let actual = format!("{:x}", Sha256::digest(bytes));
            if &actual != expected {
                return Err(CoreError::configuration(format!(
                    "plugin `{}` Web asset `{}` digest mismatch",
                    manifest.plugin_id(),
                    relative_path.display()
                )));
            }
            actual_paths.remove(relative_path);
        }
        if !actual_paths.is_empty() {
            return Err(CoreError::configuration(format!(
                "plugin `{}` Web root contains undeclared files: {actual_paths:?}",
                manifest.plugin_id()
            )));
        }
    }
    Ok(())
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
