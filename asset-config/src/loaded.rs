use crate::error::ConfigError;
use crate::section::ConfigSection;
use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) type SharedValue = Arc<dyn Any + Send + Sync>;

/// Shared configuration instances deserialized and validated during loading.
///
/// Created by [`crate::load`] or [`crate::Registry::load`] only after every
/// registered section has loaded successfully. Each load creates an independent
/// set of instances; later file changes do not affect this result.
///
/// [`Self::get`] clones the stored `Arc` without deserializing or validating again.
/// Returned handles can outlive this container, and the registered type set is fixed.
pub struct LoadedConfig {
    pub(crate) values: HashMap<TypeId, SharedValue>,
}

impl LoadedConfig {
    /// Returns an `Arc` pointing to this type's validated configuration instance.
    ///
    /// Repeated reads share the same instance and do not deserialize or validate again.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::NotRegistered`] if this concrete type was not
    /// registered for this load, even when another type uses the same section key.
    pub fn get<T>(&self) -> Result<Arc<T>, ConfigError>
    where
        T: ConfigSection,
    {
        let value = self
            .values
            .get(&TypeId::of::<T>())
            .ok_or(ConfigError::NotRegistered {
                type_name: type_name::<T>(),
                key: T::KEY,
            })?;
        Ok(Arc::clone(value).downcast::<T>().unwrap())
    }
}
