//! Manifest 与锁文件的跨字段不变量校验。
//!
//! 本模块在文档完成反序列化后检查版本、标识符、相对路径、权限与 capability 的组合
//! 约束。它只验证声明，不读取文件或探测实际运行时产物。

use super::{
    MANIFEST_VERSION, ManifestActionAccess, PLUGIN_LOCK_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME,
    PLUGIN_WASM_FILE_NAME, PLUGIN_WEB_ENTRY_FILE_NAME, PluginManifest, PluginManifestLock,
};
use crate::protocol::PLUGIN_API_VERSION;
use std::collections::HashSet;

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.manifest_version != MANIFEST_VERSION {
            return Err(format!(
                "unsupported manifest_version `{}`; supported version is `{MANIFEST_VERSION}`",
                self.manifest_version
            ));
        }
        validate_id("plugin.id", &self.plugin.id, &['.', '-', '_'])?;
        if self.plugin.name.trim().is_empty()
            || self.plugin.version.trim().is_empty()
            || self.plugin.publisher.trim().is_empty()
        {
            return Err(
                "plugin.name, plugin.version and plugin.publisher must not be empty".to_string(),
            );
        }
        validate_plugin_api_version(self.runtime.plugin_api())?;
        if self.permissions.network.enabled() && !self.permissions.network.has_scope() {
            return Err("permissions.network must declare an explicit host scope".to_string());
        }
        if self.permissions.filesystem.enabled() && !self.permissions.filesystem.has_scope() {
            return Err("permissions.filesystem must declare explicit path scopes".to_string());
        }
        validate_capabilities(self)?;
        Ok(())
    }
}

impl PluginManifestLock {
    pub fn validate_for(&self, manifest: &PluginManifest) -> Result<(), String> {
        if self.manifest_version != manifest.manifest_version {
            return Err(format!(
                "manifest.lock.json manifest_version `{}` does not match manifest `{}`",
                self.manifest_version, manifest.manifest_version
            ));
        }
        if self.plugin_id != manifest.plugin.id {
            return Err(format!(
                "manifest.lock.json plugin_id `{}` does not match manifest plugin.id `{}`",
                self.plugin_id, manifest.plugin.id
            ));
        }
        let wasm_path = std::path::Path::new(PLUGIN_WASM_FILE_NAME);
        if !self.integrity.contains_key(wasm_path) {
            return Err(format!(
                "manifest.lock.json integrity must contain `{PLUGIN_WASM_FILE_NAME}` for extism plugins"
            ));
        }
        let has_web_assets = self.integrity.keys().any(|path| path != wasm_path);
        if has_web_assets
            && !self
                .integrity
                .contains_key(std::path::Path::new(PLUGIN_WEB_ENTRY_FILE_NAME))
        {
            return Err(format!(
                "manifest.lock.json integrity must contain `{PLUGIN_WEB_ENTRY_FILE_NAME}` when Web assets are present"
            ));
        }
        for (path, digest) in &self.integrity {
            validate_relative_path("manifest.lock.json integrity path", path)?;
            if is_plugin_metadata_path(path) {
                return Err(format!(
                    "manifest.lock.json integrity must not contain metadata file `{}`",
                    path.display()
                ));
            }
            validate_digest(
                &format!("manifest.lock.json integrity[`{}`]", path.display()),
                digest,
            )?;
        }
        Ok(())
    }
}

fn validate_plugin_api_version(value: &str) -> Result<(), String> {
    if value == PLUGIN_API_VERSION {
        Ok(())
    } else {
        Err(format!(
            "unsupported runtime.plugin_api `{value}`; supported version is `{PLUGIN_API_VERSION}`"
        ))
    }
}

const SUPPORTED_VIEWS: &[&str] = &[
    "text",
    "markdown",
    "html",
    "plugin_frame",
    "json",
    "media",
    "download",
];

fn validate_capabilities(manifest: &PluginManifest) -> Result<(), String> {
    let capabilities = &manifest.capabilities;
    let mut resource_action_ids = HashSet::new();
    let mut directory_action_ids = HashSet::new();
    for kind in &capabilities.kinds {
        validate_id("capabilities.kinds[].kind", &kind.kind, &[':', '-', '_'])?;
        if kind
            .parent
            .as_ref()
            .is_some_and(|parent| parent.trim().is_empty())
        {
            return Err("capabilities.kinds[].parent must not be empty".to_string());
        }
    }
    for kind in &capabilities.directory_kinds {
        validate_id(
            "capabilities.directory_kinds[].kind",
            &kind.kind,
            &[':', '-', '_'],
        )?;
        if kind
            .parent
            .as_ref()
            .is_some_and(|parent| parent.trim().is_empty())
        {
            return Err("capabilities.directory_kinds[].parent must not be empty".to_string());
        }
    }
    for action in &capabilities.resource_actions {
        validate_id(
            "capabilities.resource_actions[].id",
            &action.id,
            &['.', ':', '-', '_'],
        )?;
        if !resource_action_ids.insert(action.id.as_str()) {
            return Err(format!("duplicate resource action `{}`", action.id));
        }
        if let Some(provides) = &action.provides {
            validate_id(
                "capabilities.resource_actions[].provides",
                provides,
                &['.', ':', '-', '_'],
            )?;
        }
        if action.label.trim().is_empty() {
            return Err(format!(
                "capabilities.resource_actions[`{}`].label must not be empty",
                action.id
            ));
        }
        if !manifest.permissions.resource_read() {
            return Err(format!(
                "capabilities.resource_actions[`{}`] lacks resource.read permission",
                action.id
            ));
        }
        validate_id("handler", &action.handler, &['.', '-', '_'])?;
        if action.views.is_empty() {
            return Err(format!(
                "capabilities.resource_actions[`{}`].views must not be empty",
                action.id
            ));
        }
        let unique_views = action.views.iter().collect::<HashSet<_>>();
        if unique_views.len() != action.views.len() {
            return Err(format!(
                "capabilities.resource_actions[`{}`].views must not contain duplicates",
                action.id
            ));
        }
        for view in &action.views {
            if !SUPPORTED_VIEWS.contains(&view.as_str()) {
                return Err(format!(
                    "capabilities.resource_actions[`{}`] declares unsupported view `{view}`",
                    action.id
                ));
            }
        }
        if action
            .requires
            .as_ref()
            .is_some_and(|requires| requires.content)
            && !manifest.permissions.resource_content_read()
        {
            return Err(format!(
                "capabilities.resource_actions[`{}`] requires content without resource.content.read permission",
                action.id
            ));
        }
        if matches!(action.access, ManifestActionAccess::Write)
            && !manifest.permissions.resource_write()
            && !manifest.permissions.resource_content_replace()
            && !manifest.permissions.resource_derived_asset_write()
        {
            return Err(format!(
                "capabilities.resource_actions[`{}`] is writable without a write permission",
                action.id
            ));
        }
    }
    for action in &capabilities.directory_actions {
        validate_id(
            "capabilities.directory_actions[].id",
            &action.id,
            &['.', '-', '_'],
        )?;
        if action.label.trim().is_empty() || action.handler.trim().is_empty() {
            return Err("directory action label and handler must not be empty".to_string());
        }
        if !directory_action_ids.insert(action.id.as_str()) {
            return Err(format!("duplicate directory action `{}`", action.id));
        }
        if let Some(provides) = &action.provides {
            validate_id(
                "capabilities.directory_actions[].provides",
                provides,
                &['.', ':', '-', '_'],
            )?;
        }
        if action.views.is_empty()
            || action
                .views
                .iter()
                .any(|view| !SUPPORTED_VIEWS.contains(&view.as_str()))
        {
            return Err(format!(
                "directory action `{}` must declare only supported views",
                action.id
            ));
        }
        for kind in &action.applies_to.kinds {
            validate_id(
                "capabilities.directory_actions[].applies_to.kinds[]",
                kind,
                &[':', '.', '-', '_'],
            )?;
        }
    }
    Ok(())
}

fn validate_relative_path(field: &str, path: &std::path::Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("{field} must be a safe relative path"));
    }
    Ok(())
}

fn is_plugin_metadata_path(path: &std::path::Path) -> bool {
    [PLUGIN_MANIFEST_FILE_NAME, PLUGIN_LOCK_FILE_NAME]
        .iter()
        .any(|name| path == std::path::Path::new(name))
        || path.components().count() == 1
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!(".{PLUGIN_LOCK_FILE_NAME}."))
                        && name.ends_with(".tmp")
                })
}

fn validate_digest(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be a lowercase SHA-256 digest"));
    }
    Ok(())
}

fn validate_id(field: &str, value: &str, punctuation: &[char]) -> Result<(), String> {
    if value.is_empty() || value.trim() != value {
        return Err(format!("{field} must be non-empty and canonical"));
    }
    if !value.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || punctuation.contains(&character)
    }) {
        return Err(format!("{field} contains invalid characters: `{value}`"));
    }
    Ok(())
}
