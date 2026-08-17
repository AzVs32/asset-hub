use extism_pdk::{FnResult, plugin_fn};

const GAMES_KIND: &str = "directory:games";
const GAME_KIND: &str = "directory:games:item";

#[cfg(target_arch = "wasm32")]
const MAX_DOCUMENT_SIZE: u64 = 1024 * 1024;
#[cfg(target_arch = "wasm32")]
const CONTENT_CHUNK_SIZE: u64 = 64 * 1024;

const THUMBNAIL_SVG: &str = include_str!("thumbnail.svg");

#[plugin_fn]
pub fn hello_world(_: ()) -> FnResult<String> {
    Ok("Hello from directory-games-plugin!\n".to_string()+GAME_KIND+GAME_KIND+THUMBNAIL_SVG)
}

// #[plugin_fn]
// pub fn render_thumbnail(input: String) -> FnResult<String> {
//
// }