use asset_plugin_api::protocol::{
    CreateDirectoryTreeEffect, CreateTreeDirectory, CreateTreeResource, CreateTreeResourceEncoding,
    DirectoryActionEffect, JsonView, MediaView, PLUGIN_API_VERSION, PluginDirectoryActionOutput,
    PluginDirectoryActionRequest, PluginFrameView, PluginMediaEncoding, PluginView,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use extism_pdk::{Error, FnResult, plugin_fn};
#[cfg(target_arch = "wasm32")]
use serde::Serialize;
use serde_json::{Value, json};

const GAMES_KIND: &str = "directory:games";
const GAME_KIND: &str = "directory:games:item";
#[cfg(target_arch = "wasm32")]
const MAX_DOCUMENT_SIZE: u64 = 1024 * 1024;
#[cfg(target_arch = "wasm32")]
const CONTENT_CHUNK_SIZE: u64 = 64 * 1024;
const THUMBNAIL_SVG: &str = include_str!("thumbnail.svg");

#[plugin_fn]
pub fn render_thumbnail(input: String) -> FnResult<String> {
    let request: PluginDirectoryActionRequest = serde_json::from_str(&input)?;
    let output = PluginDirectoryActionOutput::new(PluginView::Media(MediaView {
        mime_type: "image/svg+xml".to_string(),
        title: Some(request.directory.name),
        encoding: PluginMediaEncoding::Base64,
        data: BASE64_STANDARD.encode(THUMBNAIL_SVG.as_bytes()),
    }));
    Ok(serde_json::to_string(&output)?)
}

#[plugin_fn]
pub fn render_workspace(input: String) -> FnResult<String> {
    render_workspace_payload(&input)
}

#[plugin_fn]
pub fn create_game(input: String) -> FnResult<String> {
    create_game_payload(&input)
}

fn render_workspace_payload(input: &str) -> FnResult<String> {
    let request: PluginDirectoryActionRequest = serde_json::from_str(input)?;
    let operation = request
        .input
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("initial");
    if operation == "initial" {
        return frame_output();
    }
    if operation != "load" {
        return Err(Error::msg("unsupported Games workspace operation").into());
    }

    #[cfg(target_arch = "wasm32")]
    {
        return load_workspace(&request);
    }
    #[cfg(not(target_arch = "wasm32"))]
    Err(Error::msg("workspace loading requires the Wasm Host ABI").into())
}

fn frame_output() -> FnResult<String> {
    let output = PluginDirectoryActionOutput::new(PluginView::PluginFrame(PluginFrameView {
        plugin_api: PLUGIN_API_VERSION.to_string(),
        title: Some("Games".to_string()),
        url: "index.html".to_string(),
    }));
    Ok(serde_json::to_string(&output)?)
}

fn create_game_payload(input: &str) -> FnResult<String> {
    let request: PluginDirectoryActionRequest = serde_json::from_str(input)?;
    if request.directory.kind != GAMES_KIND {
        return Err(Error::msg("games can only be created inside a Games directory").into());
    }
    let name = required_string(&request.input, "name", 64)?;
    validate_directory_name(&name)?;
    let title = required_string(&request.input, "title", 120)?;
    let summary = required_string(&request.input, "summary", 1000)?;
    let version = optional_string(&request.input, "version", 64)?.unwrap_or_else(|| "0.1.0".into());

    let readme = format!(
        "# {title}\n\n{summary}\n\n## Game information\n\n- Directory: `{name}`\n- Version: `{version}`\n"
    );
    let mut output = PluginDirectoryActionOutput::new(PluginView::Json(JsonView {
        data: json!({"created": name, "title": title}),
    }));
    output.effects.push(DirectoryActionEffect::CreateTree(
        CreateDirectoryTreeEffect {
            directories: vec![
                CreateTreeDirectory {
                    path: name.clone(),
                    kind: Some(GAME_KIND.to_string()),
                },
                CreateTreeDirectory {
                    path: format!("{name}/public"),
                    kind: Some("core:directory".to_string()),
                },
            ],
            resources: vec![
                generated_markdown(&name, "README.md", readme),
                generated_markdown(&name, "HASH.md", String::new()),
            ],
        },
    ));
    Ok(serde_json::to_string(&output)?)
}

fn generated_markdown(directory: &str, name: &str, data: String) -> CreateTreeResource {
    CreateTreeResource {
        directory: directory.to_string(),
        name: name.to_string(),
        kind: None,
        mime_type: Some("text/markdown; charset=utf-8".to_string()),
        encoding: CreateTreeResourceEncoding::Base64,
        data: BASE64_STANDARD.encode(data.as_bytes()),
    }
}

fn required_string(input: &Value, field: &str, max: usize) -> FnResult<String> {
    optional_string(input, field, max)?
        .ok_or_else(|| Error::msg(format!("{field} is required")).into())
}

fn optional_string(input: &Value, field: &str, max: usize) -> FnResult<Option<String>> {
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

fn validate_directory_name(name: &str) -> FnResult<()> {
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

#[cfg(target_arch = "wasm32")]
#[derive(Serialize)]
struct GameDocument {
    id: String,
    name: String,
    path: String,
    readme: Option<String>,
    hash: Option<String>,
}

#[cfg(target_arch = "wasm32")]
fn load_workspace(request: &PluginDirectoryActionRequest) -> FnResult<String> {
    use asset_plugin_api::abi::directory::guest::{list_children, list_resources_in};

    let data = if request.directory.kind == GAMES_KIND {
        let mut games = Vec::new();
        let mut cursor = None;
        loop {
            let page = list_children(&request.directory_ref, cursor.as_deref(), 100)?;
            for child in page.items.into_iter().filter(|item| item.kind == GAME_KIND) {
                let documents =
                    list_resources_in(&request.directory_ref, Some(&child.id), None, 100)?;
                games.push(GameDocument {
                    id: child.id,
                    name: child.name,
                    path: child.path,
                    readme: read_document(&documents.items, "README.md")?,
                    hash: read_document(&documents.items, "HASH.md")?,
                });
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        json!({
            "mode": "library",
            "directory": {"name": request.directory.name, "path": request.directory.path},
            "games": games
        })
    } else if request.directory.kind == GAME_KIND {
        let documents = list_resources_in(&request.directory_ref, None, None, 100)?;
        json!({
            "mode": "game",
            "directory": {"name": request.directory.name, "path": request.directory.path},
            "readme": read_document(&documents.items, "README.md")?,
            "hash": read_document(&documents.items, "HASH.md")?
        })
    } else {
        return Err(Error::msg("unsupported directory kind for Games workspace").into());
    };
    let output = PluginDirectoryActionOutput::new(PluginView::Json(JsonView { data }));
    Ok(serde_json::to_string(&output)?)
}

#[cfg(target_arch = "wasm32")]
fn read_document(
    resources: &[asset_plugin_api::protocol::PluginDirectoryResource],
    name: &str,
) -> FnResult<Option<String>> {
    use asset_plugin_api::abi::content::guest::read_all;

    let Some(resource) = resources.iter().find(|resource| resource.name == name) else {
        return Ok(None);
    };
    let Some(reference) = resource.content_ref.as_ref() else {
        return Ok(None);
    };
    let bytes = read_all(&reference.reference, MAX_DOCUMENT_SIZE, CONTENT_CHUNK_SIZE)?;
    Ok(Some(String::from_utf8(bytes).map_err(|_| {
        Error::msg(format!("{name} is not valid UTF-8"))
    })?))
}

#[cfg(test)]
mod tests {
    use super::create_game_payload;
    use asset_plugin_api::protocol::{DirectoryActionEffect, PluginDirectoryActionOutput};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

    fn request(action: &str, input: serde_json::Value) -> String {
        serde_json::json!({
            "action": action,
            "access": if action.ends_with("create") { "write" } else { "read" },
            "input": input,
            "directory": {
                "id": "0198a1b2-c3d4-7e5f-8012-3456789abcde",
                "parent_id": null,
                "path": "/Games",
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
            &create_game_payload(&request(
                "azvs.games.create",
                serde_json::json!({
                    "name": "game_name_1",
                    "title": "Game One",
                    "summary": "A test game",
                "version": "1.0.0"
                }),
            ))
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
        let readme = BASE64_STANDARD.decode(&tree.resources[0].data).unwrap();
        assert!(String::from_utf8(readme).unwrap().contains("# Game One"));
        assert!(
            BASE64_STANDARD
                .decode(&tree.resources[1].data)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn create_game_rejects_a_nested_name() {
        assert!(
            create_game_payload(&request(
                "azvs.games.create",
                serde_json::json!({
                    "name": "nested/game", "title": "Bad", "summary": "Bad"
                })
            ))
            .is_err()
        );
    }
}
