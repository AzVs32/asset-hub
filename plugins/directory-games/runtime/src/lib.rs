use asset_plugin_sdk::{
    DirectoryContext, DirectoryResource, DirectoryResponse, Error, Frame, Media, Result, Tree,
    Value, export_directory_action, json,
};

const GAMES_KIND: &str = "directory:games";
const GAME_KIND: &str = "directory:games:item";
const MAX_DOCUMENT_SIZE: u64 = 1024 * 1024;
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
    if operation != "load" {
        return Err(Error::msg("unsupported Games workspace operation").into());
    }

    load_workspace(&context)
}

fn create_game_payload(context: DirectoryContext) -> Result<DirectoryResponse> {
    if context.directory().kind() != GAMES_KIND {
        return Err(Error::msg("games can only be created inside a Games directory").into());
    }
    let name = required_string(context.input(), "name", 64)?;
    validate_directory_name(&name)?;
    let title = required_string(context.input(), "title", 120)?;
    let summary = required_string(context.input(), "summary", 1000)?;
    let version =
        optional_string(context.input(), "version", 64)?.unwrap_or_else(|| "0.1.0".into());

    let readme = format!(
        "# {title}\n\n{summary}\n\n## Game information\n\n- Directory: `{name}`\n- Version: `{version}`\n"
    );
    Ok(
        DirectoryResponse::json(json!({"created": name, "title": title}))?.create_tree(
            Tree::new()
                .directory_kind(&name, GAME_KIND)
                .directory_kind(format!("{name}/public"), "core:directory")
                .markdown(&name, "README.md", readme)
                .markdown(&name, "HASH.md", ""),
        ),
    )
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

fn validate_directory_name(name: &str) -> Result<()> {
    let valid = name.len() <= 64
        && !matches!(name, "." | "..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if !valid {
        return Err(Error::msg(
            "name may contain only ASCII letters, numbers, dot, dash and underscore",
        )
        .into());
    }
    Ok(())
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
                "hash": read_document(&documents, "HASH.md")?,
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
            "readme": read_document(&documents, "README.md")?,
            "hash": read_document(&documents, "HASH.md")?
        })
    } else {
        return Err(Error::msg("unsupported directory kind for Games workspace").into());
    };
    DirectoryResponse::json(data)
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
    use asset_plugin_sdk::runtime::{decode_base64, run_directory_action};
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
                    "name": "game_name_1",
                    "title": "Game One",
                    "summary": "A test game",
                    "version": "1.0.0"
                })),
                create_game_payload,
            )
            .unwrap(),
        )
        .unwrap();
        let DirectoryActionEffect::CreateTree(tree) = &output.effects[0] else {
            panic!("expected create tree")
        };
        assert_eq!(tree.directories[0].path, "game_name_1");
        assert_eq!(
            tree.directories[0].kind.as_deref(),
            Some("directory:games:item")
        );
        assert_eq!(tree.directories[1].path, "game_name_1/public");
        assert_eq!(tree.resources.len(), 2);
        assert!(
            tree.resources
                .iter()
                .all(|resource| resource.kind.is_none())
        );
        let readme = decode_base64(&tree.resources[0].data).unwrap();
        assert!(String::from_utf8(readme).unwrap().contains("# Game One"));
        assert!(decode_base64(&tree.resources[1].data).unwrap().is_empty());
    }

    #[test]
    fn create_game_rejects_a_nested_name() {
        let failure = run_directory_action(
            request(serde_json::json!({
                "name": "nested/game",
                "title": "Bad",
                "summary": "Bad"
            })),
            create_game_payload,
        )
        .unwrap();
        let failure: serde_json::Value = serde_json::from_str(&failure).unwrap();
        assert!(
            failure["error"]["message"]
                .as_str()
                .unwrap()
                .contains("ASCII letters")
        );
    }
}
