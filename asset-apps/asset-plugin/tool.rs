use asset_plugin_api::{
    MANIFEST_SCHEMA, MANIFEST_TEMPLATE, PluginCapabilities, PluginManifest, PluginManifestLock,
    PluginMetadata, PluginPermissions, PluginRuntime, PluginRuntimeLock, PluginWeb, PluginWebLock,
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

pub fn generate_schema(path: &Path) -> Result<()> {
    write_new_file(path, MANIFEST_SCHEMA.as_bytes())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftManifest {
    #[serde(default, rename = "$schema")]
    schema: Option<String>,
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
        #[serde(default)]
        wasi: bool,
        #[serde(default = "default_plugin_api_version")]
        plugin_api: String,
    },
}

fn default_plugin_api_version() -> String {
    asset_plugin_api::PLUGIN_API_VERSION.to_string()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftWeb {
    root: PathBuf,
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
            wasi,
            plugin_api,
        },
    };
    let web = draft.web.map(|web| PluginWeb { root: web.root });
    PluginManifest {
        schema: draft.schema,
        manifest_version: draft.manifest_version,
        plugin: draft.plugin,
        runtime,
        web,
        capabilities: draft.capabilities,
        permissions: draft.permissions,
    }
}

pub fn seal_manifest(path: &Path) -> Result<PluginManifest> {
    let manifest: PluginManifest = read_json(path)?;
    validate_contract(&manifest)?;
    let lock = lock_for_manifest(&manifest, path)?;
    validate_lock(&manifest, &lock)?;
    write_json_atomically(&manifest_lock_path(path), &lock)?;
    Ok(manifest)
}

pub fn verify_manifest(path: &Path) -> Result<PluginManifest> {
    let manifest: PluginManifest = read_json(path)?;
    validate_contract(&manifest)?;
    let lock = read_required_lock(&manifest, path)?;
    verify_wasm(&manifest, lock.as_ref(), path)?;
    verify_web(&manifest, lock.as_ref(), path)?;
    Ok(manifest)
}

pub fn verify_wasm_manifest(path: &Path) -> Result<PluginManifest> {
    let manifest: PluginManifest = read_json(path)?;
    validate_contract(&manifest)?;
    let lock = read_required_lock(&manifest, path)?;
    verify_wasm(&manifest, lock.as_ref(), path)?;
    Ok(manifest)
}

pub fn verify_web_manifest(path: &Path) -> Result<PluginManifest> {
    let manifest: PluginManifest = read_json(path)?;
    validate_contract(&manifest)?;
    let lock = read_required_lock(&manifest, path)?;
    verify_web(&manifest, lock.as_ref(), path)?;
    Ok(manifest)
}

fn lock_for_manifest(manifest: &PluginManifest, path: &Path) -> Result<PluginManifestLock> {
    let base = manifest_base(path);
    let runtime = match &manifest.runtime {
        PluginRuntime::Builtin => None,
        PluginRuntime::Extism { wasm, .. } => Some(PluginRuntimeLock {
            wasm_sha256: digest_file(&resolve_artifact(&base, "runtime.wasm", wasm)?)?,
        }),
    };
    let web = manifest
        .web
        .as_ref()
        .map(|web| {
            let root = resolve_artifact(&base, "web.root", &web.root)?;
            Ok(PluginWebLock {
                integrity: web_integrity(&root)?,
            })
        })
        .transpose()?;
    Ok(PluginManifestLock {
        manifest_version: manifest.manifest_version,
        plugin_id: manifest.plugin_id().to_string(),
        runtime,
        web,
    })
}

fn verify_wasm(
    manifest: &PluginManifest,
    lock: Option<&PluginManifestLock>,
    path: &Path,
) -> Result<()> {
    let base = manifest_base(path);
    if let PluginRuntime::Extism { wasm, .. } = &manifest.runtime {
        let expected = lock
            .and_then(|lock| lock.runtime.as_ref())
            .ok_or_else(|| ToolError("manifest.lock.json runtime.wasm_sha256 is required".into()))?
            .wasm_sha256
            .as_str();
        let actual = digest_file(&resolve_artifact(&base, "runtime.wasm", wasm)?)?;
        if actual != expected {
            return Err(ToolError(format!(
                "Wasm digest mismatch: manifest.lock.json={expected} actual={actual}"
            )));
        }
    }
    Ok(())
}

fn verify_web(
    manifest: &PluginManifest,
    lock: Option<&PluginManifestLock>,
    path: &Path,
) -> Result<()> {
    let base = manifest_base(path);
    if let Some(web) = &manifest.web {
        let expected = &lock
            .and_then(|lock| lock.web.as_ref())
            .ok_or_else(|| ToolError("manifest.lock.json web.integrity is required".into()))?
            .integrity;
        let actual = web_integrity(&resolve_artifact(&base, "web.root", &web.root)?)?;
        if &actual != expected {
            return Err(ToolError(web_integrity_difference(expected, &actual)));
        }
    }
    Ok(())
}

fn read_required_lock(
    manifest: &PluginManifest,
    path: &Path,
) -> Result<Option<PluginManifestLock>> {
    if !manifest_requires_lock(manifest) {
        return Ok(None);
    }
    let lock_path = manifest_lock_path(path);
    let lock: PluginManifestLock = read_json(&lock_path)?;
    validate_lock(manifest, &lock)?;
    Ok(Some(lock))
}

fn validate_lock(manifest: &PluginManifest, lock: &PluginManifestLock) -> Result<()> {
    lock.validate_for(manifest)
        .map_err(|error| ToolError(format!("invalid manifest.lock.json: {error}")))
}

fn manifest_requires_lock(manifest: &PluginManifest) -> bool {
    matches!(manifest.runtime, PluginRuntime::Extism { .. }) || manifest.web.is_some()
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
    manifest_base(path).join("manifest.lock.json")
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
        .map(|metadata| metadata.permissions())
        .ok();
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| ToolError(format!("create `{}`: {error}", temporary.display())))?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions).map_err(|error| {
                ToolError(format!(
                    "set permissions on `{}`: {error}",
                    temporary.display()
                ))
            })?;
        }
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
mod tests;
