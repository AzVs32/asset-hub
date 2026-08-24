use super::{GAME_KIND, GAMES_KIND, required_string};
use crate::cover;
use asset_rust_sdk::{DirectoryContext, DirectoryResponse, Error, Result, Tree, Value, json};

const MAX_GAME_NAME_CHARS: usize = 255;
const MAX_ALIASES: usize = 32;

pub(crate) fn handle(context: DirectoryContext) -> Result<DirectoryResponse> {
    if context.directory().kind() != GAMES_KIND {
        return Err(Error::msg(
            "games can only be created inside a Games directory",
        ));
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
        return Err(Error::msg(format!("aliases exceeds {MAX_ALIASES} entries")));
    }
    values
        .iter()
        .map(|value| {
            let alias = value
                .as_str()
                .ok_or_else(|| Error::msg("aliases must contain only strings"))?
                .trim();
            if alias.is_empty() {
                return Err(Error::msg("aliases must not contain blank names"));
            }
            if alias.chars().count() > MAX_GAME_NAME_CHARS {
                return Err(Error::msg(format!(
                    "alias exceeds {MAX_GAME_NAME_CHARS} characters"
                )));
            }
            if alias.chars().any(char::is_control) {
                return Err(Error::msg("aliases must not contain control characters"));
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
        ))
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
    use asset_rust_sdk::__private::run_directory_action;
    use asset_rust_sdk::serde_json;
    use asset_rust_sdk::{decode_base64, encode_base64};

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
        let output: serde_json::Value = serde_json::from_str(
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
        let tree = &output["effects"][0];
        assert_eq!(tree["type"], "create_tree");
        let directories = tree["directories"].as_array().unwrap();
        assert_eq!(directories.len(), 2);
        assert_eq!(directories[0]["path"], "Game One");
        assert_eq!(directories[0]["kind"], "directory:games:item");
        assert_eq!(directories[1]["path"], "Game One/public");
        assert!(directories[1]["kind"].is_null());
        let resources = tree["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 3);
        assert!(resources.iter().all(|resource| resource["kind"].is_null()));
        let resource = |name: &str| {
            resources
                .iter()
                .find(|resource| resource["name"] == name)
                .unwrap()
        };
        assert_eq!(
            String::from_utf8(
                decode_base64(resource("README.md")["data"].as_str().unwrap()).unwrap()
            )
            .unwrap(),
            "# Game One\n"
        );
        assert_eq!(
            resource("METADATA.yml")["mime_type"],
            "application/yaml; charset=utf-8"
        );
        assert_eq!(
            String::from_utf8(
                decode_base64(resource("METADATA.yml")["data"].as_str().unwrap()).unwrap()
            )
            .unwrap(),
            "name:\n  - \"Game One\"\n  - \"Game 1\"\n  - \"游戏一\"\n  - \"Game \\\"One\\\"\"\n"
        );
        assert_eq!(resource("cover.svg")["directory"], "Game One/public");
        assert_eq!(resource("cover.svg")["mime_type"], "image/svg+xml");
        let cover = String::from_utf8(
            decode_base64(resource("cover.svg")["data"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
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
