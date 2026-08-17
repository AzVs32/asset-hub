use asset_plugin_sdk::{
    DirectoryContext, DirectoryResource, DirectoryResponse, Error, Frame, Media, Result, Tree,
    Value, decode_base64, encode_base64, export_directory_action, json,
};

const GAMES_KIND: &str = "directory:games";
const GAME_KIND: &str = "directory:games:item";
const MAX_GAME_NAME_CHARS: usize = 255;
const MAX_ALIASES: usize = 32;
const MAX_DOCUMENT_SIZE: u64 = 1024 * 1024;
const MAX_COVER_SIZE: usize = 1024 * 1024;
const CONTENT_CHUNK_SIZE: u64 = 64 * 1024;
const THUMBNAIL_SVG: &str = include_str!("thumbnail.svg");

export_directory_action!(render_thumbnail => render_thumbnail_payload);
export_directory_action!(render_workspace => render_workspace_payload);
export_directory_action!(create_game => create_game_payload);

fn render_thumbnail_payload(context: DirectoryContext) -> Result<DirectoryResponse> {
    Ok(DirectoryResponse::media(
        Media::base64("image/svg+xml", THUMBNAIL_SVG).title(context.directory().name()),
    ))
}

fn render_workspace_payload(context: DirectoryContext) -> Result<DirectoryResponse> {
    let operation = context
        .input()
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("initial");
    if operation == "initial" {
        return Ok(DirectoryResponse::frame(
            Frame::new("index.html").title("Games"),
        ));
    }
    match operation {
        "load" => load_workspace(&context),
        "cover" => load_cover(&context),
        _ => Err(Error::msg("unsupported Games workspace operation").into()),
    }
}

fn create_game_payload(context: DirectoryContext) -> Result<DirectoryResponse> {
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
    let icon = optional_icon(context.input())?;
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

struct GameIcon {
    filename: &'static str,
    mime_type: &'static str,
    bytes: Vec<u8>,
}

fn optional_icon(input: &Value) -> Result<Option<GameIcon>> {
    let Some(icon) = input.get("icon") else {
        return Ok(None);
    };
    if icon.is_null() {
        return Ok(None);
    }
    let icon = icon
        .as_object()
        .ok_or_else(|| Error::msg("icon must be an object"))?;
    let mime_type = icon
        .get("mime_type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::msg("icon.mime_type is required"))?;
    let data = icon
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::msg("icon.data is required"))?;
    if data.len() > MAX_COVER_SIZE.saturating_mul(4) / 3 + 4 {
        return Err(Error::msg("game icon exceeds 1 MiB").into());
    }
    let bytes = decode_base64(data).map_err(|_| Error::msg("game icon is not valid base64"))?;
    if bytes.is_empty() {
        return Err(Error::msg("game icon must not be empty").into());
    }
    if bytes.len() > MAX_COVER_SIZE {
        return Err(Error::msg("game icon exceeds 1 MiB").into());
    }

    let (filename, canonical_mime, valid_signature) = match mime_type {
        "image/png" => (
            "cover.png",
            "image/png",
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        ),
        "image/jpeg" => (
            "cover.jpg",
            "image/jpeg",
            bytes.starts_with(b"\xff\xd8\xff"),
        ),
        "image/webp" => (
            "cover.webp",
            "image/webp",
            bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        ),
        "image/gif" => (
            "cover.gif",
            "image/gif",
            bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        ),
        "image/svg+xml" => ("cover.svg", "image/svg+xml", has_svg_root(&bytes)),
        _ => return Err(Error::msg("game icon must be PNG, JPEG, WebP, GIF, or SVG").into()),
    };
    if !valid_signature {
        return Err(Error::msg("game icon content does not match its image type").into());
    }
    Ok(Some(GameIcon {
        filename,
        mime_type: canonical_mime,
        bytes,
    }))
}

fn has_svg_root(bytes: &[u8]) -> bool {
    let Ok(mut document) = std::str::from_utf8(bytes) else {
        return false;
    };
    document = document.trim_start_matches('\u{feff}').trim_start();
    loop {
        if document.starts_with("<?xml") {
            let Some(end) = document.find("?>") else {
                return false;
            };
            document = document[end + 2..].trim_start();
            continue;
        }
        if document.starts_with("<!--") {
            let Some(end) = document.find("-->") else {
                return false;
            };
            document = document[end + 3..].trim_start();
            continue;
        }
        break;
    }
    document
        .strip_prefix("<svg")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|character| {
            character == '>' || character == '/' || character.is_ascii_whitespace()
        })
}

fn required_string(input: &Value, field: &str, max: usize) -> Result<String> {
    optional_string(input, field, max)?
        .ok_or_else(|| Error::msg(format!("{field} is required")).into())
}

fn optional_string(input: &Value, field: &str, max: usize) -> Result<Option<String>> {
    let Some(value) = input.get(field) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| Error::msg(format!("{field} must be a string")))?
        .trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > max {
        return Err(Error::msg(format!("{field} exceeds {max} characters")).into());
    }
    Ok(Some(value.to_string()))
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

fn load_workspace(context: &DirectoryContext) -> Result<DirectoryResponse> {
    let directory = context.directory();
    let data = if directory.kind() == GAMES_KIND {
        let mut games = Vec::new();
        for child in context
            .children_bounded(1_000)?
            .into_iter()
            .filter(|item| item.kind() == GAME_KIND)
        {
            let documents = context.resources_bounded(Some(child.id()), 100)?;
            games.push(json!({
                "id": child.id(),
                "name": child.name(),
                "path": child.path(),
                "readme": read_document(&documents, "README.md")?,
            }));
        }
        json!({
            "mode": "library",
            "directory": {"name": directory.name(), "path": directory.path()},
            "games": games
        })
    } else if directory.kind() == GAME_KIND {
        let documents = context.resources_bounded(None, 100)?;
        json!({
            "mode": "game",
            "directory": {"name": directory.name(), "path": directory.path()},
            "readme": read_document(&documents, "README.md")?
        })
    } else {
        return Err(Error::msg("unsupported directory kind for Games workspace").into());
    };
    DirectoryResponse::json(data)
}

fn load_cover(context: &DirectoryContext) -> Result<DirectoryResponse> {
    if context.directory().kind() != GAMES_KIND {
        return Err(Error::msg("game covers can only be loaded from a Games directory").into());
    }
    let game_id = required_string(context.input(), "game_id", 64)?;
    let games = context.children_bounded(1_000)?;
    let game = games
        .iter()
        .find(|child| child.id() == game_id && child.kind() == GAME_KIND)
        .ok_or_else(|| Error::msg("game is not a direct child of this Games directory"))?;
    let Some(public) = context
        .children_bounded_in(Some(game.id()), 100)?
        .into_iter()
        .find(|child| child.name() == "public")
    else {
        return DirectoryResponse::json(json!({"cover": null}));
    };
    let resources = context.resources_bounded(Some(public.id()), 100)?;
    let Some((resource, mime_type)) = resources.iter().find_map(|resource| {
        cover_mime_type(resource.name()).map(|mime_type| (resource, mime_type))
    }) else {
        return DirectoryResponse::json(json!({"cover": null}));
    };
    let Some(bytes) = resource.read_bytes(MAX_COVER_SIZE as u64, CONTENT_CHUNK_SIZE)? else {
        return DirectoryResponse::json(json!({"cover": null}));
    };
    DirectoryResponse::json(json!({
        "cover": {
            "mime_type": mime_type,
            "data": encode_base64(bytes)
        }
    }))
}

fn cover_mime_type(name: &str) -> Option<&'static str> {
    match name {
        "cover.png" => Some("image/png"),
        "cover.jpg" | "cover.jpeg" => Some("image/jpeg"),
        "cover.webp" => Some("image/webp"),
        "cover.gif" => Some("image/gif"),
        "cover.svg" => Some("image/svg+xml"),
        _ => None,
    }
}

fn read_document(resources: &[DirectoryResource], name: &str) -> Result<Option<String>> {
    let Some(resource) = resources.iter().find(|resource| resource.name() == name) else {
        return Ok(None);
    };
    resource.read_text(MAX_DOCUMENT_SIZE, CONTENT_CHUNK_SIZE)
}

#[cfg(test)]
mod tests {
    use super::create_game_payload;
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
                create_game_payload,
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
        assert_eq!(
            decode_base64(&resource("cover.svg").data).unwrap(),
            br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"></svg>"#
        );
    }

    #[test]
    fn create_game_rejects_a_nested_name() {
        let failure = run_directory_action(
            request(serde_json::json!({
                "name": "nested/game"
            })),
            create_game_payload,
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
