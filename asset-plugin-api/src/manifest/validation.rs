//! Manifest 与锁文件的跨字段不变量校验。
//!
//! 本模块在文档完成反序列化后检查版本、标识符、相对路径、权限与 capability 的组合
//! 约束。它只验证声明，不读取文件或探测实际运行时产物。

use super::{
    MANIFEST_VERSION, ManifestActionAccess, PLUGIN_API_VERSION, PluginManifest, PluginManifestLock,
    PluginRuntime,
};
use std::collections::HashSet;
use std::path::Path;

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
        match &self.runtime {
            PluginRuntime::Builtin => {}
            PluginRuntime::Extism {
                wasm, plugin_api, ..
            } => {
                if wasm.as_os_str().is_empty() {
                    return Err("runtime.wasm must not be empty".to_string());
                }
                validate_relative_path("runtime.wasm", wasm)?;
                validate_plugin_api_version(plugin_api)?;
            }
        }
        if let Some(web) = &self.web {
            validate_relative_path("web.root", &web.root)?;
        }
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
        match &manifest.runtime {
            PluginRuntime::Builtin => {
                if self.runtime.is_some() {
                    return Err(
                        "manifest.lock.json runtime is only valid for extism plugins".to_string(),
                    );
                }
            }
            PluginRuntime::Extism { .. } => {
                let Some(runtime) = &self.runtime else {
                    return Err("manifest.lock.json runtime.wasm_sha256 is required".to_string());
                };
                validate_digest(
                    "manifest.lock.json runtime.wasm_sha256",
                    &runtime.wasm_sha256,
                )?;
            }
        }
        match (&manifest.web, &self.web) {
            (None, Some(_)) => {
                return Err(
                    "manifest.lock.json web is only valid when manifest.web is present".to_string(),
                );
            }
            (Some(_), None) => {
                return Err("manifest.lock.json web.integrity is required".to_string());
            }
            (Some(_), Some(web)) => {
                if web.integrity.is_empty() {
                    return Err("manifest.lock.json web.integrity must not be empty".to_string());
                }
                for (path, digest) in &web.integrity {
                    validate_relative_path("manifest.lock.json web.integrity path", path)?;
                    validate_digest(
                        &format!("manifest.lock.json web.integrity[`{}`]", path.display()),
                        digest,
                    )?;
                }
            }
            (None, None) => {}
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
    let mut action_ids = HashSet::new();
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
    for action in &capabilities.actions {
        validate_id(
            "capabilities.actions[].id",
            &action.id,
            &['.', ':', '-', '_'],
        )?;
        if !action_ids.insert(action.id.as_str()) {
            return Err(format!("duplicate resource action `{}`", action.id));
        }
        if action.label.trim().is_empty() {
            return Err(format!(
                "capabilities.actions[`{}`].label must not be empty",
                action.id
            ));
        }
        if !manifest.permissions.resource_read() {
            return Err(format!(
                "capabilities.actions[`{}`] lacks resource.read permission",
                action.id
            ));
        }
        validate_id("handler", &action.handler, &['.', '-', '_'])?;
        if action.views.is_empty() {
            return Err(format!(
                "capabilities.actions[`{}`].views must not be empty",
                action.id
            ));
        }
        let unique_views = action.views.iter().collect::<HashSet<_>>();
        if unique_views.len() != action.views.len() {
            return Err(format!(
                "capabilities.actions[`{}`].views must not contain duplicates",
                action.id
            ));
        }
        for view in &action.views {
            if !SUPPORTED_VIEWS.contains(&view.as_str()) {
                return Err(format!(
                    "capabilities.actions[`{}`] declares unsupported view `{view}`",
                    action.id
                ));
            }
        }
        if action.views.iter().any(|view| view == "plugin_frame") && manifest.web.is_none() {
            return Err(format!(
                "capabilities.actions[`{}`] returns plugin_frame but plugin.web is missing",
                action.id
            ));
        }
        if action
            .requires
            .as_ref()
            .is_some_and(|requires| requires.content)
            && !manifest.permissions.content_read()
        {
            return Err(format!(
                "capabilities.actions[`{}`] requires content without content.read permission",
                action.id
            ));
        }
        if matches!(action.access, ManifestActionAccess::Write)
            && !manifest.permissions.resource_write()
            && !manifest.permissions.content_replace()
            && !manifest.permissions.derived_asset_write()
        {
            return Err(format!(
                "capabilities.actions[`{}`] is writable without a write permission",
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
        if !action_ids.insert(action.id.as_str()) {
            return Err(format!("duplicate action id `{}`", action.id));
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

fn validate_relative_path(field: &str, path: &Path) -> Result<(), String> {
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
