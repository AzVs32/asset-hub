use crate::error::ConfigError;
use crate::loaded::LoadedConfig;
use crate::section::ConfigSection;
use config::{Config, File, Value};
use std::any::TypeId;
use std::collections::HashSet;
use std::path::Path;

type Validator = Box<dyn Fn(&Config) -> Result<(), ConfigError> + Send + Sync>;

struct Registration {
    key: &'static str,
    type_id: TypeId,
    default: Value,
    validate: Validator,
}

#[derive(Default)]
pub struct Registry {
    registrations: Vec<Registration>,
    keys: HashSet<&'static str>,
    types: HashSet<TypeId>,
}

impl Registry {
    /// Register a configuration type.
    pub fn register<T>(&mut self) -> Result<&mut Self, ConfigError>
    where
        T: ConfigSection,
    {
        let type_id = TypeId::of::<T>();
        if self.types.contains(&type_id) {
            return Err(ConfigError::AlreadyRegistered(T::KEY));
        }

        if self.keys.contains(&T::KEY) {
            return Err(ConfigError::DuplicateKey(T::KEY));
        }

        let default = Config::try_from(&T::default())?.cache;
        let validate: Validator = Box::new(|config| {
            let value = config.get::<T>(T::KEY)?;
            value.validate()
        });

        self.registrations.push(Registration {
            key: T::KEY,
            type_id,
            default,
            validate,
        });
        self.types.insert(type_id);
        self.keys.insert(T::KEY);
        Ok(self)
    }

    /// Loading configuration.
    /// Priority: config.toml > T:default()
    pub fn load(self, path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
        let mut builder = Config::builder();

        for registration in &self.registrations {
            builder = builder.set_default(registration.key, registration.default.clone())?
        }
        builder = builder.add_source(File::from(path.as_ref()).required(false));

        let inner = builder.build()?;

        for registration in &self.registrations {
            (registration.validate)(&inner)?;
        }
        let registered = self.registrations.into_iter().map(|r| r.type_id).collect();
        Ok(LoadedConfig { inner, registered })
    }
}
