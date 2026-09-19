use crate::error::ConfigError;
use crate::section::ConfigSection;
use config::Config;
use std::any::TypeId;
use std::collections::HashSet;

pub struct LoadedConfig {
    pub(crate) inner: Config,
    pub(crate) registered: HashSet<TypeId>,
}

impl LoadedConfig {
    pub fn get<T>(&self) -> Result<T, ConfigError>
    where
        T: ConfigSection,
    {
        if !self.registered.contains(&TypeId::of::<T>()) {
            return Err(ConfigError::NotRegistered(T::KEY));
        }
        Ok(self.inner.get::<T>(T::KEY)?)
    }
}
