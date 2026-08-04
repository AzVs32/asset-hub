mod runtime;

pub use runtime::AssetRuntime;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Verified browser assets grouped by external plugin id and package-relative path.
pub type PluginWebAssets = HashMap<String, HashMap<PathBuf, Arc<[u8]>>>;
