use asset_plugin_api::{
    PLUGIN_LOCK_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME, PLUGIN_WASM_FILE_NAME,
    PLUGIN_WEB_ENTRY_FILE_NAME, PluginManifest, PluginManifestLock,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ToolError(String);

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ToolError {}

type Result<T> = std::result::Result<T, ToolError>;

pub fn verify_manifest(path: &Path) -> Result<PluginManifest> {
    let manifest: PluginManifest = read_json(path)?;
    validate_contract(&manifest)?;
    validate_package_location(&manifest, path)?;
    let lock = read_required_lock(&manifest, path)?;
    verify_integrity(&manifest, &lock, path)?;
    Ok(manifest)
}

fn verify_integrity(
    manifest: &PluginManifest,
    lock: &PluginManifestLock,
    path: &Path,
) -> Result<()> {
    let base = manifest_base(path);
    let actual = package_integrity(&base)?;
    if manifest_uses_plugin_frame(manifest)
        && !actual.contains_key(Path::new(PLUGIN_WEB_ENTRY_FILE_NAME))
    {
        return Err(ToolError(format!(
            "plugin_frame actions require package-root `{PLUGIN_WEB_ENTRY_FILE_NAME}`"
        )));
    }
    if lock.integrity != actual {
        return Err(ToolError(integrity_difference(&lock.integrity, &actual)));
    }
    Ok(())
}

fn read_required_lock(manifest: &PluginManifest, path: &Path) -> Result<PluginManifestLock> {
    let lock_path = manifest_lock_path(path);
    let lock: PluginManifestLock = read_json(&lock_path)?;
    validate_lock(manifest, &lock)?;
    Ok(lock)
}

fn validate_lock(manifest: &PluginManifest, lock: &PluginManifestLock) -> Result<()> {
    lock.validate_for(manifest)
        .map_err(|error| ToolError(format!("invalid manifest.lock.json: {error}")))
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
        .map_err(|error| ToolError(format!("invalid plugin manifest: {error}")))
}

fn manifest_base(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn manifest_lock_path(path: &Path) -> PathBuf {
    manifest_base(path).join(PLUGIN_LOCK_FILE_NAME)
}

fn validate_package_location(manifest: &PluginManifest, path: &Path) -> Result<()> {
    if path.file_name().and_then(|name| name.to_str()) != Some(PLUGIN_MANIFEST_FILE_NAME) {
        return Err(ToolError(format!(
            "plugin manifest must be named `{PLUGIN_MANIFEST_FILE_NAME}`"
        )));
    }
    let directory_name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    if directory_name != Some(manifest.plugin_id()) {
        return Err(ToolError(format!(
            "plugin package directory must be named `{}`",
            manifest.plugin_id()
        )));
    }
    Ok(())
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

fn package_integrity(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    if !root.is_dir() {
        return Err(ToolError(format!(
            "plugin package `{}` is not a directory",
            root.display()
        )));
    }
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.retain(|(relative, _)| !is_plugin_metadata_file(relative));
    let has_web_assets = files
        .iter()
        .any(|(relative, _)| relative != Path::new(PLUGIN_WASM_FILE_NAME));
    let has_entry = files
        .iter()
        .any(|(relative, _)| relative == Path::new(PLUGIN_WEB_ENTRY_FILE_NAME));
    if has_web_assets && !has_entry {
        return Err(ToolError(format!(
            "plugin package contains Web assets but `{PLUGIN_WEB_ENTRY_FILE_NAME}` is missing"
        )));
    }
    let mut integrity = BTreeMap::new();
    for (relative, absolute) in files {
        integrity.insert(relative, digest_file(&absolute)?);
    }
    Ok(integrity)
}

fn is_plugin_metadata_file(path: &Path) -> bool {
    [PLUGIN_MANIFEST_FILE_NAME, PLUGIN_LOCK_FILE_NAME]
        .iter()
        .any(|name| path == Path::new(name))
        || path.components().count() == 1
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!(".{PLUGIN_LOCK_FILE_NAME}."))
                        && name.ends_with(".tmp")
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

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        ToolError(format!(
            "read plugin package directory `{}`: {error}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| ToolError(format!("read plugin package: {error}")))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            ToolError(format!(
                "inspect plugin package entry `{}`: {error}",
                path.display()
            ))
        })?;
        if file_type.is_symlink() {
            return Err(ToolError(format!(
                "symbolic links are not allowed in plugin packages: `{}`",
                path.display()
            )));
        }
        if file_type.is_dir() {
            collect_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| ToolError(format!("resolve plugin artifact path: {error}")))?
                .to_path_buf();
            files.push((relative, path));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(())
}

fn integrity_difference(
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
        "Plugin integrity mismatch: missing={missing:?} undeclared={undeclared:?} changed={changed:?}"
    )
}

#[cfg(test)]
mod tests {
    include!("tests.rs");
}
