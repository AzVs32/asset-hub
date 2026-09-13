//! 配置注册、加载和输出过程的错误类型。

use std::path::PathBuf;
use thiserror::Error;

/// 注册、加载或提取配置分区时产生的错误。
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("failed to read configuration file `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid TOML configuration: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("configuration section name `{section}` is invalid")]
    InvalidSectionName { section: String },

    #[error("configuration section `{section}` is already registered")]
    DuplicateSection { section: String },

    #[error("configuration type for section `{section}` is already registered as `{registered}`")]
    DuplicateType { section: String, registered: String },

    #[error("configuration section `{section}` overlaps registered section `{registered}`")]
    OverlappingSection { section: String, registered: String },

    #[error("configuration namespace `{namespace}` must be a TOML table")]
    InvalidNamespace { namespace: String },

    #[error("configuration section `{section}` is invalid: {source}")]
    Deserialize {
        section: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("configuration section `{section}` is invalid: {reason}")]
    Validate { section: String, reason: String },

    #[error("configuration section `{section}` was not registered or loaded")]
    SectionUnavailable { section: String },

    #[error("failed to serialize configuration section `{section}`: {source}")]
    Serialize {
        section: String,
        #[source]
        source: toml::ser::Error,
    },
}
