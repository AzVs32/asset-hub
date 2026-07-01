use asset_core::CoreError;
use asset_plugin_api::PluginManifest;
use std::path::Path;

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

    fn unique_temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "asset-hub-plugin-manifest-{}-{name}",
            std::process::id()
        ));
        path
    }
}
