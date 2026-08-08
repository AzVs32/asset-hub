mod runtime;
mod upload_finalization;

pub use runtime::AssetRuntime;
pub use upload_finalization::UploadFinalizationDispatcher;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Verified browser assets grouped by external plugin id and package-relative path.
pub type PluginWebAssets = HashMap<String, HashMap<PathBuf, Arc<[u8]>>>;
