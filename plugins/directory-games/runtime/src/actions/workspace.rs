use super::{GAME_KIND, GAMES_KIND, required_string};
use asset_plugin_sdk::{
    DirectoryContext, DirectoryResource, DirectoryResponse, Error, Frame, Result, Value,
    encode_base64, json,
};

const MAX_COVER_SIZE: u64 = 1024 * 1024;
const CONTENT_CHUNK_SIZE: u64 = 64 * 1024;

pub(crate) fn handle(context: DirectoryContext) -> Result<DirectoryResponse> {
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

fn load_workspace(context: &DirectoryContext) -> Result<DirectoryResponse> {
    let directory = context.directory();
    let data = if directory.kind() == GAMES_KIND {
        let mut games = Vec::new();
        for child in context
            .children_bounded(1_000)?
            .into_iter()
            .filter(|item| item.kind() == GAME_KIND)
        {
            games.push(json!({
                "id": child.id(),
                "name": child.name(),
                "path": child.path(),
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
            "documents": editable_document_references(&documents),
            "cover": read_cover(context, None)?
        })
    } else {
        return Err(Error::msg("unsupported directory kind for Games workspace").into());
    };
    DirectoryResponse::json(data)
}

fn editable_document_references(resources: &[DirectoryResource]) -> Vec<Value> {
    ["README.md", "METADATA.yml"]
        .iter()
        .filter_map(|name| resources.iter().find(|resource| resource.name() == *name))
        .map(|resource| json!({"id": resource.id(), "name": resource.name()}))
        .collect()
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
    DirectoryResponse::json(json!({"cover": read_cover(context, Some(game.id()))?}))
}

/// 从当前游戏或指定子游戏中读取公开封面。
fn read_cover(context: &DirectoryContext, game_id: Option<&str>) -> Result<Value> {
    let children = match game_id {
        Some(game_id) => context.children_bounded_in(Some(game_id), 100)?,
        None => context.children_bounded(100)?,
    };
    let Some(public) = children.into_iter().find(|child| child.name() == "public") else {
        return Ok(Value::Null);
    };
    let resources = context.resources_bounded(Some(public.id()), 100)?;
    let Some((resource, mime_type)) = resources.iter().find_map(|resource| {
        cover_mime_type(resource.name()).map(|mime_type| (resource, mime_type))
    }) else {
        return Ok(Value::Null);
    };
    let Some(bytes) = resource.read_bytes(MAX_COVER_SIZE, CONTENT_CHUNK_SIZE)? else {
        return Ok(Value::Null);
    };
    Ok(json!({
        "mime_type": mime_type,
        "data": encode_base64(bytes)
    }))
}

/// 根据规范封面文件名返回浏览器使用的 MIME 类型。
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
