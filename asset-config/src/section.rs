use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::ConfigError;

pub trait ConfigSection: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    const KEY: &'static str;
    fn validate(&self) -> Result<(), ConfigError> {
        Ok(())
    }
}
