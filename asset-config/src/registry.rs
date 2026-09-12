//! 配置分区的注册与统一加载。

use crate::document::{paths_overlap, validate_section_name};
use crate::registration::{SectionRegistration, TypedRegistration};
use crate::{ConfigError, ConfigSection, LoadedConfig};
use std::any::TypeId;
use std::collections::HashMap;
use std::path::Path;

/// 可执行程序未指定配置路径时使用的默认配置文件。
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";

/// 由可执行程序按照实际包含的模块装配的配置注册表。
#[derive(Default)]
pub struct ConfigRegistry {
    registrations: Vec<Box<dyn SectionRegistration>>,
    types: HashMap<TypeId, &'static str>,
}

impl ConfigRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个由模块独立拥有的配置分区。
    ///
    /// 分区不能重复，也不能与其他已注册分区形成父子包含关系。一个模块注册某个分区后，
    /// 它的 Serde 模型就拥有并校验该路径下的完整配置子树。
    pub fn register<T: ConfigSection>(&mut self) -> Result<&mut Self, ConfigError> {
        validate_section_name(T::SECTION)?;

        if let Some(registered) = self.types.get(&TypeId::of::<T>()) {
            return Err(ConfigError::DuplicateType {
                section: T::SECTION.to_owned(),
                registered: (*registered).to_owned(),
            });
        }

        for registration in &self.registrations {
            if registration.section() == T::SECTION {
                return Err(ConfigError::DuplicateSection {
                    section: T::SECTION.to_owned(),
                });
            }
            if paths_overlap(registration.section(), T::SECTION) {
                return Err(ConfigError::OverlappingSection {
                    section: T::SECTION.to_owned(),
                    registered: registration.section().to_owned(),
                });
            }
        }

        self.registrations
            .push(Box::new(TypedRegistration::<T>::new()));
        self.types.insert(TypeId::of::<T>(), T::SECTION);
        Ok(self)
    }

    /// 以链式构造方式向注册表增加一个强类型配置分区。
    pub fn with<T: ConfigSection>(mut self) -> Result<Self, ConfigError> {
        self.register::<T>()?;
        Ok(self)
    }

    /// 加载显式指定的文件；`path` 为 `None` 时尝试加载可选的 `./config.toml`。
    ///
    /// 显式指定的文件必须存在。默认文件不存在时，根据所有已注册分区的默认值生成配置。
    pub fn load(&self, path: Option<&Path>) -> Result<LoadedConfig, ConfigError> {
        match path {
            Some(path) => self.load_file(path),
            None => self.load_optional_file(DEFAULT_CONFIG_FILE),
        }
    }

    pub fn load_file(&self, path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        self.load_str(&source)
    }

    pub fn load_optional_file(&self, path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(source) => self.load_str(&source),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => self.load_str(""),
            Err(source) => Err(ConfigError::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }

    pub fn load_str(&self, source: &str) -> Result<LoadedConfig, ConfigError> {
        let document = toml::from_str::<toml::Table>(source)?;
        let mut sections = HashMap::with_capacity(self.registrations.len());

        for registration in &self.registrations {
            sections.insert(
                registration.registered_type_id(),
                registration.load(&document)?,
            );
        }

        Ok(LoadedConfig::new(document, sections))
    }
}
