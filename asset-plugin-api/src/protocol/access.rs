//! Action access boundary carried on Host-to-plugin requests.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether the Host authorized a handler to request write effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginActionAccess {
    #[default]
    Read,
    Write,
}
