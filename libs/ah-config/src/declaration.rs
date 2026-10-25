use crate::{ConfigError, ConfigErrorKind, merge::DefaultMerge, source::Source};
use figment::Figment;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    any::{Any, TypeId},
    path::Path,
    sync::Arc,
};

pub type ValidationError = Box<dyn std::error::Error + Send + Sync>;
pub(crate) type SharedValue = Arc<dyn Any + Send + Sync>;
type Validator<T> = fn(&T) -> Result<(), ValidationError>;
type Loader = Box<dyn Fn(&Figment, &Path, bool) -> Result<SharedValue, ConfigError> + Send + Sync>;

pub struct Declaration {
    pub(crate) key: &'static str,
    pub(crate) spaces: &'static [&'static str],
    pub(crate) type_id: fn() -> TypeId,
    pub(crate) prepare: fn() -> Result<Prepared, ConfigError>,
}

impl Declaration {
    pub const fn new<T: 'static>(
        key: &'static str,
        spaces: &'static [&'static str],
        prepare: fn() -> Result<Prepared, ConfigError>,
    ) -> Self {
        Self {
            key,
            spaces,
            type_id: TypeId::of::<T>,
            prepare,
        }
    }
}

#[linkme::distributed_slice]
pub static DECLARATIONS: [Declaration];

pub struct Prepared {
    pub(crate) key: &'static str,
    pub(crate) type_id: TypeId,
    pub(crate) defaults: Option<Source>,
    pub(crate) replace: bool,
    pub(crate) empty: Source,
    pub(crate) prepare_defaults: fn(&mut Source, &Source, &str),
    pub(crate) load: Loader,
}

pub fn prepare<T>(
    key: &'static str,
    defaults: Option<T>,
    validate: Option<Validator<T>>,
    replace: bool,
) -> Result<Prepared, ConfigError>
where
    T: Serialize + DeserializeOwned + DefaultMerge + Send + Sync + 'static,
{
    let defaults = defaults
        .map(|value| Source::defaults(key, value))
        .transpose()?;
    let has_defaults = defaults.is_some();
    let empty = Source::defaults(key, std::collections::BTreeMap::<String, ()>::new())?;
    let load = Box::new(move |figment: &Figment, path: &Path, from_file: bool| {
        let value: T = figment.extract_inner(key).map_err(|error| {
            let kind = if from_file {
                ConfigErrorKind::Deserialize
            } else if !has_defaults && matches!(error.kind, figment::error::Kind::MissingField(_)) {
                ConfigErrorKind::MissingSection
            } else {
                ConfigErrorKind::Defaults
            };
            ConfigError::for_section(kind, path, key, error)
        })?;
        if let Some(validate) = validate {
            validate(&value).map_err(|error| {
                ConfigError::for_section(ConfigErrorKind::Validation, path, key, error)
            })?;
        }
        Ok(Arc::new(value) as SharedValue)
    });
    Ok(Prepared {
        key,
        type_id: TypeId::of::<T>(),
        defaults,
        replace,
        empty,
        prepare_defaults: Source::prepare_defaults::<T>,
        load,
    })
}
