use crate::action::builtin;
use crate::official_plugins;
use asset_core::CoreError;
use asset_plugin_api::{
    PLUGIN_LOCK_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME, PLUGIN_WASM_FILE_NAME,
    PLUGIN_WEB_ENTRY_FILE_NAME, PluginManifest, PluginManifestLock, PluginRuntime,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Maximum accepted serialized Manifest size.
pub const MAX_PLUGIN_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Maximum accepted serialized lock size.
pub const MAX_PLUGIN_LOCK_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum accepted Wasm artifact size.
pub const MAX_PLUGIN_WASM_BYTES: usize = 64 * 1024 * 1024;
/// Maximum accepted aggregate Web asset size.
pub const MAX_PLUGIN_WEB_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub(crate) manifest: PluginManifest,
    pub(crate) manifest_path: Option<PathBuf>,
    pub(crate) wasm: Option<Arc<[u8]>>,
    pub(crate) web_assets: HashMap<PathBuf, Arc<[u8]>>,
}

#[derive(Debug, Clone)]
pub struct PluginCatalog {
    plugins: Vec<LoadedPlugin>,
}

impl PluginCatalog {
    /// Load the built-in catalog and verify every external package.
    ///
    /// This operation is read-only. External packages must already contain a valid
    /// `manifest.lock.json`; use [`generate_plugin_manifest_lock`] explicitly when sealing a
    /// package.
    pub fn load(packages_root: &Path) -> Result<Self, CoreError> {
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

        for package_root in discover_plugin_packages(packages_root)? {
            plugins.push(load_verified_plugin_package(
                &package_root.join(PLUGIN_MANIFEST_FILE_NAME),
            )?);
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

    pub fn plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }

    pub fn external_plugin_count(&self) -> usize {
        self.plugins
            .iter()
            .filter(|plugin| plugin.manifest_path.is_some())
            .count()
    }
}

impl LoadedPlugin {
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn is_external(&self) -> bool {
        self.manifest_path.is_some()
    }

    pub fn wasm(&self) -> Option<&Arc<[u8]>> {
        self.wasm.as_ref()
    }

    pub fn web_assets(&self) -> &HashMap<PathBuf, Arc<[u8]>> {
        &self.web_assets
    }
}

/// Verify and snapshot one external plugin package without changing it.
pub fn load_verified_plugin_package(path: &Path) -> Result<LoadedPlugin, CoreError> {
    let manifest = load_plugin_manifest_file(path)?;
    validate_package_location(&manifest, path)?;
    let package_root = path.parent().unwrap_or_else(|| Path::new("."));
    let artifacts = validate_loaded_manifest(&manifest, Some(package_root))?;
    Ok(LoadedPlugin {
        manifest,
        manifest_path: Some(path.to_path_buf()),
        wasm: artifacts.wasm,
        web_assets: artifacts.web_assets,
    })
}

/// Generate and atomically install the lock for one unsealed plugin package.
///
/// Generation and verification deliberately remain separate: this function refuses to replace an
/// existing lock, while [`load_verified_plugin_package`] never creates or updates one.
pub fn generate_plugin_manifest_lock(path: &Path) -> Result<PluginManifest, CoreError> {
    let manifest = load_plugin_manifest_file(path)?;
    validate_package_location(&manifest, path)?;
    let package_root = path.parent().unwrap_or_else(|| Path::new("."));
    let lock_path = package_root.join(PLUGIN_LOCK_FILE_NAME);
    match std::fs::symlink_metadata(&lock_path) {
        Ok(_) => {
            return Err(CoreError::configuration(format!(
                "plugin manifest lock `{}` already exists",
                lock_path.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CoreError::configuration(format!(
                "inspect plugin manifest lock `{}`: {error}",
                lock_path.display()
            )));
        }
    }
    let lock = generate_plugin_manifest_lock_value(package_root, &manifest)?;
    lock.validate_for(&manifest).map_err(|error| {
        CoreError::configuration(format!(
            "generated invalid plugin manifest lock `{}`: {error}",
            lock_path.display()
        ))
    })?;
    if !write_json_atomically(&lock_path, &lock)? {
        return Err(CoreError::configuration(format!(
            "plugin manifest lock `{}` was created concurrently",
            lock_path.display()
        )));
    }
    Ok(manifest)
}

fn validate_package_location(manifest: &PluginManifest, path: &Path) -> Result<(), CoreError> {
    if path.file_name().and_then(|name| name.to_str()) != Some(PLUGIN_MANIFEST_FILE_NAME) {
        return Err(CoreError::configuration(format!(
            "plugin manifest must be named `{PLUGIN_MANIFEST_FILE_NAME}`"
        )));
    }
    let package_root = path.parent().unwrap_or_else(|| Path::new("."));
    let package_metadata = std::fs::symlink_metadata(package_root).map_err(|error| {
        CoreError::configuration(format!(
            "inspect plugin package `{}`: {error}",
            package_root.display()
        ))
    })?;
    if package_metadata.file_type().is_symlink() || !package_metadata.is_dir() {
        return Err(CoreError::configuration(format!(
            "plugin package `{}` must be a directory and not a symbolic link",
            package_root.display()
        )));
    }
    if package_root.file_name().and_then(|name| name.to_str()) != Some(manifest.plugin_id()) {
        return Err(CoreError::configuration(format!(
            "plugin package directory `{}` must match plugin.id `{}`",
            package_root.display(),
            manifest.plugin_id()
        )));
    }
    Ok(())
}

fn discover_plugin_packages(root: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(CoreError::configuration(format!(
                "inspect plugin packages root `{}`: {error}",
                root.display()
            )));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CoreError::configuration(format!(
            "plugin packages root `{}` must be a directory",
            root.display()
        )));
    }

    let mut packages = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|error| {
        CoreError::configuration(format!(
            "read plugin packages root `{}`: {error}",
            root.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            CoreError::configuration(format!("read plugin package entry: {error}"))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            CoreError::configuration(format!(
                "inspect plugin package `{}`: {error}",
                entry.path().display()
            ))
        })?;
        if file_type.is_symlink() || !file_type.is_dir() {
            return Err(CoreError::configuration(format!(
                "plugin package `{}` must be a directory",
                entry.path().display()
            )));
        }
        packages.push(entry.path());
    }
    packages.sort();
    Ok(packages)
}

pub(crate) fn load_plugin_manifest_file(path: &Path) -> Result<PluginManifest, CoreError> {
    let metadata = regular_file_metadata(path, "plugin manifest")?;
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

fn load_plugin_manifest_lock_file(
    path: &Path,
    manifest: &PluginManifest,
) -> Result<PluginManifestLock, CoreError> {
    let metadata = regular_file_metadata(path, "plugin manifest lock")?;
    if metadata.len() > MAX_PLUGIN_LOCK_BYTES {
        return Err(CoreError::configuration(format!(
            "plugin manifest lock `{}` exceeds the {MAX_PLUGIN_LOCK_BYTES} byte limit",
            path.display()
        )));
    }
    let content = std::fs::read_to_string(path).map_err(|error| {
        CoreError::configuration(format!(
            "read plugin manifest lock `{}`: {error}",
            path.display()
        ))
    })?;
    let lock: PluginManifestLock = serde_json::from_str(&content).map_err(|error| {
        CoreError::configuration(format!(
            "parse plugin manifest lock `{}`: {error}",
            path.display()
        ))
    })?;
    lock.validate_for(manifest).map_err(|error| {
        CoreError::configuration(format!(
            "invalid plugin manifest lock `{}`: {error}",
            path.display()
        ))
    })?;
    Ok(lock)
}

fn generate_plugin_manifest_lock_value(
    package_root: &Path,
    manifest: &PluginManifest,
) -> Result<PluginManifestLock, CoreError> {
    let artifacts = inspect_package_artifacts(package_root, manifest)?;
    Ok(PluginManifestLock {
        manifest_version: manifest.manifest_version,
        plugin_id: manifest.plugin_id().to_string(),
        integrity: artifacts.integrity,
    })
}

/// Installs a complete JSON file without replacing a file created concurrently.
/// Returns whether this call installed the file.
fn write_json_atomically(path: &Path, value: &impl Serialize) -> Result<bool, CoreError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        CoreError::configuration(format!("encode `{}`: {error}", path.display()))
    })?;
    bytes.push(b'\n');
    if bytes.len() > MAX_PLUGIN_LOCK_BYTES as usize {
        return Err(CoreError::configuration(format!(
            "generated plugin manifest lock `{}` exceeds the {MAX_PLUGIN_LOCK_BYTES} byte limit",
            path.display()
        )));
    }
    let temporary = path.with_file_name(format!(
        ".{PLUGIN_LOCK_FILE_NAME}.{}.tmp",
        uuid::Uuid::now_v7()
    ));
    let write_result = (|| -> Result<bool, CoreError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                CoreError::configuration(format!(
                    "create plugin manifest lock `{}`: {error}",
                    temporary.display()
                ))
            })?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| {
                CoreError::configuration(format!(
                    "write plugin manifest lock `{}`: {error}",
                    temporary.display()
                ))
            })?;
        let installed = match std::fs::hard_link(&temporary, path) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => Err(CoreError::configuration(format!(
                "install plugin manifest lock `{}`: {error}",
                path.display()
            )))?,
        };
        std::fs::remove_file(&temporary).map_err(|error| {
            CoreError::configuration(format!(
                "remove temporary plugin manifest lock `{}`: {error}",
                temporary.display()
            ))
        })?;
        Ok(installed)
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result
}

fn regular_file_metadata(path: &Path, label: &str) -> Result<std::fs::Metadata, CoreError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        CoreError::configuration(format!("inspect {label} `{}`: {error}", path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CoreError::configuration(format!(
            "{label} `{}` must be a regular file",
            path.display()
        )));
    }
    Ok(metadata)
}

#[derive(Debug, Default)]
struct LoadedArtifacts {
    wasm: Option<Arc<[u8]>>,
    web_assets: HashMap<PathBuf, Arc<[u8]>>,
    integrity: BTreeMap<PathBuf, String>,
}

fn validate_loaded_manifest(
    manifest: &PluginManifest,
    package_root: Option<&Path>,
) -> Result<LoadedArtifacts, CoreError> {
    manifest.validate().map_err(|error| {
        let source = package_root
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| format!("official plugin `{}`", manifest.plugin_id()));
        CoreError::configuration(format!("invalid plugin manifest `{source}`: {error}"))
    })?;

    if matches!(manifest.runtime, PluginRuntime::Builtin) {
        for action in &manifest.capabilities.resource_actions {
            let handler = action.handler();
            if !builtin::is_builtin_handler(Some(handler)) {
                return Err(CoreError::configuration(format!(
                    "plugin `{}` declares unknown builtin handler `{handler}`",
                    manifest.plugin_id()
                )));
            }
        }
    }

    let Some(package_root) = package_root else {
        return Ok(LoadedArtifacts::default());
    };
    let lock = load_plugin_manifest_lock_file(&package_root.join(PLUGIN_LOCK_FILE_NAME), manifest)?;
    let artifacts = inspect_package_artifacts(package_root, manifest)?;
    if lock.integrity != artifacts.integrity {
        let expected_paths = lock.integrity.keys().cloned().collect::<BTreeSet<_>>();
        let actual_paths = artifacts.integrity.keys().cloned().collect::<BTreeSet<_>>();
        let missing = expected_paths.difference(&actual_paths).collect::<Vec<_>>();
        let undeclared = actual_paths.difference(&expected_paths).collect::<Vec<_>>();
        let changed = expected_paths
            .intersection(&actual_paths)
            .filter(|path| lock.integrity.get(*path) != artifacts.integrity.get(*path))
            .collect::<Vec<_>>();
        let wasm_detail = if changed
            .iter()
            .any(|path| path.as_path() == Path::new(PLUGIN_WASM_FILE_NAME))
        {
            "; Wasm digest mismatch"
        } else {
            ""
        };
        return Err(CoreError::configuration(format!(
            "plugin `{}` package integrity mismatch: missing={missing:?} undeclared={undeclared:?} changed={changed:?}{wasm_detail}",
            manifest.plugin_id(),
        )));
    }
    Ok(artifacts)
}

fn inspect_package_artifacts(
    package_root: &Path,
    manifest: &PluginManifest,
) -> Result<LoadedArtifacts, CoreError> {
    let mut artifact_paths = HashSet::new();
    collect_package_artifact_files(package_root, package_root, &mut artifact_paths)?;
    validate_artifact_layout(manifest, &artifact_paths)?;

    let wasm_path = Path::new(PLUGIN_WASM_FILE_NAME);
    let mut artifacts = LoadedArtifacts::default();
    let mut total_web_bytes = 0usize;
    let mut paths = artifact_paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    for relative_path in paths {
        let path = package_root.join(&relative_path);
        let metadata = regular_file_metadata(&path, "plugin artifact")?;
        if relative_path == wasm_path {
            if metadata.len() > MAX_PLUGIN_WASM_BYTES as u64 {
                return Err(CoreError::configuration(format!(
                    "plugin `{}` Wasm exceeds the {MAX_PLUGIN_WASM_BYTES} byte limit",
                    manifest.plugin_id()
                )));
            }
        } else {
            total_web_bytes = total_web_bytes
                .checked_add(metadata.len() as usize)
                .ok_or_else(|| {
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
        }
        let bytes = std::fs::read(&path).map_err(|error| {
            CoreError::configuration(format!(
                "read plugin `{}` artifact `{}`: {error}",
                manifest.plugin_id(),
                path.display()
            ))
        })?;
        artifacts.integrity.insert(
            relative_path.clone(),
            format!("{:x}", Sha256::digest(&bytes)),
        );
        if relative_path == wasm_path {
            artifacts.wasm = Some(Arc::from(bytes));
        } else {
            artifacts.web_assets.insert(relative_path, Arc::from(bytes));
        }
    }
    Ok(artifacts)
}

fn validate_artifact_layout(
    manifest: &PluginManifest,
    artifact_paths: &HashSet<PathBuf>,
) -> Result<(), CoreError> {
    let wasm_path = Path::new(PLUGIN_WASM_FILE_NAME);
    let has_wasm = artifact_paths.contains(wasm_path);
    match manifest.runtime {
        PluginRuntime::Builtin if has_wasm => {
            return Err(CoreError::configuration(format!(
                "builtin plugin `{}` must not contain `{PLUGIN_WASM_FILE_NAME}`",
                manifest.plugin_id()
            )));
        }
        PluginRuntime::Extism { .. } if !has_wasm => {
            return Err(CoreError::configuration(format!(
                "extism plugin `{}` must contain `{PLUGIN_WASM_FILE_NAME}`",
                manifest.plugin_id()
            )));
        }
        _ => {}
    }
    let has_web_assets = artifact_paths.iter().any(|path| path != wasm_path);
    let has_entry = artifact_paths.contains(Path::new(PLUGIN_WEB_ENTRY_FILE_NAME));
    if has_web_assets && !has_entry {
        return Err(CoreError::configuration(format!(
            "plugin `{}` contains Web assets but `{PLUGIN_WEB_ENTRY_FILE_NAME}` is missing",
            manifest.plugin_id()
        )));
    }
    if manifest_uses_plugin_frame(manifest) && !has_entry {
        return Err(CoreError::configuration(format!(
            "plugin `{}` declares plugin_frame but `{PLUGIN_WEB_ENTRY_FILE_NAME}` is missing",
            manifest.plugin_id()
        )));
    }
    Ok(())
}

fn collect_package_artifact_files(
    root: &Path,
    directory: &Path,
    files: &mut HashSet<PathBuf>,
) -> Result<(), CoreError> {
    for entry in std::fs::read_dir(directory).map_err(|error| {
        CoreError::configuration(format!(
            "read plugin package `{}`: {error}",
            directory.display()
        ))
    })? {
        let entry = entry
            .map_err(|error| CoreError::configuration(format!("read plugin package: {error}")))?;
        let file_type = entry.file_type().map_err(|error| {
            CoreError::configuration(format!(
                "inspect plugin package entry `{}`: {error}",
                entry.path().display()
            ))
        })?;
        if file_type.is_symlink() {
            return Err(CoreError::configuration(format!(
                "plugin package entry `{}` must not be a symbolic link",
                entry.path().display()
            )));
        }
        if file_type.is_dir() {
            collect_package_artifact_files(root, &entry.path(), files)?;
        } else if file_type.is_file() {
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|error| {
                CoreError::configuration(format!("resolve plugin artifact path: {error}"))
            })?;
            if !is_plugin_metadata_file(relative) {
                files.insert(relative.to_path_buf());
            }
        } else {
            return Err(CoreError::configuration(format!(
                "plugin package entry `{}` must be a regular file or directory",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

fn is_plugin_metadata_file(path: &Path) -> bool {
    [PLUGIN_MANIFEST_FILE_NAME, PLUGIN_LOCK_FILE_NAME]
        .iter()
        .any(|name| path == Path::new(name))
        || is_temporary_lock_file(path)
}

fn is_temporary_lock_file(path: &Path) -> bool {
    path.components().count() == 1
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with(&format!(".{PLUGIN_LOCK_FILE_NAME}.")) && name.ends_with(".tmp")
            })
}

fn manifest_uses_plugin_frame(manifest: &PluginManifest) -> bool {
    manifest
        .capabilities
        .resource_actions
        .iter()
        .any(|action| action.views.iter().any(|view| view == "plugin_frame"))
        || manifest
            .capabilities
            .directory_actions
            .iter()
            .any(|action| action.views.iter().any(|view| view == "plugin_frame"))
}

#[cfg(test)]
mod tests;
