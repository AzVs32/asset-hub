//! 将不同的强类型配置分区转换为注册表可统一处理的内部表示。

use crate::document::find_path;
use crate::{ConfigError, ConfigSection};
use std::any::{Any, TypeId};
use std::marker::PhantomData;

pub(crate) trait SectionRegistration: Send + Sync {
    fn section(&self) -> &'static str;
    fn registered_type_id(&self) -> TypeId;
    fn load(&self, document: &toml::Table) -> Result<StoredSection, ConfigError>;
}

pub(crate) struct TypedRegistration<T>(PhantomData<T>);

impl<T> TypedRegistration<T> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T: ConfigSection> SectionRegistration for TypedRegistration<T> {
    fn section(&self) -> &'static str {
        T::SECTION
    }

    fn registered_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn load(&self, document: &toml::Table) -> Result<StoredSection, ConfigError> {
        let value = match find_path(document, T::SECTION)? {
            Some(value) => {
                value
                    .clone()
                    .try_into::<T>()
                    .map_err(|source| ConfigError::Deserialize {
                        section: T::SECTION.to_owned(),
                        source,
                    })?
            }
            None => T::default(),
        };
        let value = value.normalize().map_err(|reason| ConfigError::Validate {
            section: T::SECTION.to_owned(),
            reason,
        })?;

        Ok(StoredSection {
            path: T::SECTION,
            value: Box::new(value),
            serialize: serialize::<T>,
        })
    }
}

pub(crate) struct StoredSection {
    pub(crate) path: &'static str,
    pub(crate) value: Box<dyn Any + Send + Sync>,
    pub(crate) serialize: fn(&dyn Any) -> Result<toml::Value, toml::ser::Error>,
}

fn serialize<T: ConfigSection>(value: &dyn Any) -> Result<toml::Value, toml::ser::Error> {
    toml::Value::try_from(
        value
            .downcast_ref::<T>()
            .expect("stored configuration type must match its registration"),
    )
}
