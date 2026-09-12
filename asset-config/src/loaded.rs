//! 已加载配置的强类型访问与 TOML 输出。

use crate::registry::{StoredSection, insert_path};
use crate::{ConfigError, ConfigSection};
use std::any::TypeId;
use std::collections::HashMap;

/// 已完成所有注册分区反序列化和归一化的共享配置文档。
///
/// 未注册的顶层配置值仍保留在文档中，因此不同可执行程序和插件可以共享一个文件，且无需让
/// 每个可执行程序依赖所有扩展配置类型。
pub struct LoadedConfig {
    document: toml::Table,
    sections: HashMap<TypeId, StoredSection>,
}

impl LoadedConfig {
    pub(crate) fn new(document: toml::Table, sections: HashMap<TypeId, StoredSection>) -> Self {
        Self { document, sections }
    }

    pub fn section<T: ConfigSection>(&self) -> Result<&T, ConfigError> {
        self.sections
            .get(&TypeId::of::<T>())
            .and_then(|section| section.value.downcast_ref::<T>())
            .ok_or_else(|| ConfigError::SectionUnavailable {
                section: T::SECTION.to_owned(),
            })
    }

    /// 将配置输出为 TOML。已注册分区包含填充后的默认值和归一化结果，未注册分区保留原值。
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        let mut document = self.document.clone();
        for section in self.sections.values() {
            let value = (section.serialize)(section.value.as_ref()).map_err(|source| {
                ConfigError::Serialize {
                    section: section.path.to_owned(),
                    source,
                }
            })?;
            insert_path(&mut document, section.path, value);
        }
        toml::to_string_pretty(&document).map_err(|source| ConfigError::Serialize {
            section: "<document>".to_owned(),
            source,
        })
    }
}
