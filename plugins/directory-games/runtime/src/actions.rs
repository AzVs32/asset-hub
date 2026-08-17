pub(crate) mod create_game;
pub(crate) mod thumbnail;
pub(crate) mod workspace;

use asset_plugin_sdk::{Error, Result, Value};

pub(crate) const GAMES_KIND: &str = "directory:games";
pub(crate) const GAME_KIND: &str = "directory:games:item";

pub(crate) fn required_string(input: &Value, field: &str, max: usize) -> Result<String> {
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
