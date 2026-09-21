use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::ConfigError;

pub trait ConfigSection: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// A case-sensitive, dot-separated path. Each segment may contain ASCII
    /// letters, digits, `_`, or `-`. Registered paths cannot overlap.
    const KEY: &'static str;
    fn validate(&self) -> Result<(), ConfigError> {
        Ok(())
    }
}
