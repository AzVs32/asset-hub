use super::{GAME_KIND, GAMES_KIND, required_string};
use crate::cover;
use asset_plugin_sdk::{DirectoryContext, DirectoryResponse, Error, Result, Tree, Value, json};

const MAX_GAME_NAME_CHARS: usize = 255;
const MAX_ALIASES: usize = 32;

pub(crate) fn handle(context: DirectoryContext) -> Result<DirectoryResponse> {
    if context.directory().kind() != GAMES_KIND {
        return Err(Error::msg("games can only be created inside a Games directory").into());
    }
    let name = required_string(context.input(), "name", MAX_GAME_NAME_CHARS)?;
    validate_english_directory_name(&name)?;
    let aliases = aliases(context.input())?;
    let mut names = vec![name.clone()];
    for alias in aliases {
        if !names.contains(&alias) {
            names.push(alias);
        }
    }

    let readme = format!("# {name}\n");
    let metadata = metadata_yaml(&names);
    let icon = cover::optional_icon(context.input())?;
    let mut tree = Tree::new()
        .directory_kind(&name, GAME_KIND)
        .markdown(&name, "README.md", readme)
        .resource(
            &name,
            "METADATA.yml",
            metadata,
            None,
            Some("application/yaml; charset=utf-8"),
        );
    let cover = if let Some(icon) = icon {
        let public_path = format!("{name}/public");
        tree = tree.directory(&public_path).resource(
            &public_path,
            icon.filename,
            icon.bytes,
            None,
            Some(icon.mime_type),
        );
        Some(format!("public/{}", icon.filename))
    } else {
        None
    };

    Ok(DirectoryResponse::json(json!({
        "created": name,
        "names": names,
        "cover": cover
    }))?
    .create_tree(tree))
}

fn aliases(input: &Value) -> Result<Vec<String>> {
    let Some(values) = input.get("aliases") else {
        return Ok(Vec::new());
    };
    let values = values
        .as_array()
        .ok_or_else(|| Error::msg("aliases must be an array of strings"))?;
    if values.len() > MAX_ALIASES {
        return Err(Error::msg(format!("aliases exceeds {MAX_ALIASES} entries")).into());
    }
    values
        .iter()
        .map(|value| {
            let alias = value
                .as_str()
                .ok_or_else(|| Error::msg("aliases must contain only strings"))?
                .trim();
            if alias.is_empty() {
                return Err(Error::msg("aliases must not contain blank names").into());
            }
            if alias.chars().count() > MAX_GAME_NAME_CHARS {
                return Err(
                    Error::msg(format!("alias exceeds {MAX_GAME_NAME_CHARS} characters")).into(),
                );
            }
            if alias.chars().any(char::is_control) {
                return Err(Error::msg("aliases must not contain control characters").into());
            }
            Ok(alias.to_string())
        })
        .collect()
}

fn validate_english_directory_name(name: &str) -> Result<()> {
    let valid = !matches!(name, "." | "..")
        && !name.contains(['/', '\\'])
        && name
            .chars()
            .all(|character| character.is_ascii() && !character.is_ascii_control());
    if valid {
        Ok(())
    } else {
        Err(Error::msg(
            "name must be a printable English directory name without slash or backslash",
        )
        .into())
    }
}

fn metadata_yaml(names: &[String]) -> String {
    let mut metadata = String::from("name:\n");
    for name in names {
        metadata.push_str("  - \"");
        for character in name.chars() {
            match character {
                '\\' => metadata.push_str("\\\\"),
                '"' => metadata.push_str("\\\""),
                _ => metadata.push(character),
            }
        }
        metadata.push_str("\"\n");
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::handle;
    use asset_plugin_sdk::protocol::{DirectoryActionEffect, PluginDirectoryActionOutput};
    use asset_plugin_sdk::runtime::{decode_base64, encode_base64, run_directory_action};
    use asset_plugin_sdk::serde_json;

    fn request(input: serde_json::Value) -> String {
        serde_json::json!({
            "action": "directory.games.create",
            "access": "write",
            "input": input,
            "directory": {
                "id": "0198a1b2-c3d4-7e5f-8012-3456789abcde",
                "parent_id": null,
                "path": "Games",
                "name": "Games",
                "kind": "directory:games",
                "revision": 1,
                "created_at": "2026-08-14T00:00:00Z",
                "updated_at": "2026-08-14T00:00:00Z"
            },
            "directory_ref": "opaque-directory-ref"
        })
        .to_string()
    }

    #[test]
    fn create_game_emits_the_complete_game_tree() {
        let output: PluginDirectoryActionOutput = serde_json::from_str(
            &run_directory_action(
                request(serde_json::json!({
                    "name": "Game One",
                    "aliases": ["Game 1", "游戏一", "Game One", "Game \"One\""],
                    "icon": {
                        "mime_type": "image/svg+xml",
                        "data": encode_base64(br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"></svg>"#)
                    }
                })),
                handle,
            )
            .unwrap(),
        )
        .unwrap();
        let DirectoryActionEffect::CreateTree(tree) = &output.effects[0] else {
            panic!("expected create tree")
        };
        assert_eq!(tree.directories.len(), 2);
        assert_eq!(tree.directories[0].path, "Game One");
        assert_eq!(
            tree.directories[0].kind.as_deref(),
            Some("directory:games:item")
        );
        assert_eq!(tree.directories[1].path, "Game One/public");
        assert_eq!(tree.directories[1].kind, None);
        assert_eq!(tree.resources.len(), 3);
        assert!(
            tree.resources
                .iter()
                .all(|resource| resource.kind.is_none())
        );
        let resource = |name: &str| {
            tree.resources
                .iter()
                .find(|resource| resource.name == name)
                .unwrap()
        };
        assert_eq!(
            String::from_utf8(decode_base64(&resource("README.md").data).unwrap()).unwrap(),
            "# Game One\n"
        );
        assert_eq!(
            resource("METADATA.yml").mime_type.as_deref(),
            Some("application/yaml; charset=utf-8")
        );
        assert_eq!(
            String::from_utf8(decode_base64(&resource("METADATA.yml").data).unwrap()).unwrap(),
            "name:\n  - \"Game One\"\n  - \"Game 1\"\n  - \"游戏一\"\n  - \"Game \\\"One\\\"\"\n"
        );
        assert_eq!(resource("cover.svg").directory, "Game One/public");
        assert_eq!(
            resource("cover.svg").mime_type.as_deref(),
            Some("image/svg+xml")
        );
        let cover = String::from_utf8(decode_base64(&resource("cover.svg").data).unwrap()).unwrap();
        assert!(cover.contains("<svg"));
    }

    #[test]
    fn create_game_rejects_a_nested_name() {
        let failure = run_directory_action(
            request(serde_json::json!({
                "name": "nested/game"
            })),
            handle,
        )
        .unwrap();
        let failure: serde_json::Value = serde_json::from_str(&failure).unwrap();
        assert!(
            failure["error"]["message"]
                .as_str()
                .unwrap()
                .contains("without slash")
        );
    }
}
