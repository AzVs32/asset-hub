//! Immutable declarations owned by a driver implementation.

use crate::{
    application::dto::{DriverSettingsSchema, InstanceConfigSchema},
    domain::{DriverKey, namespace::ChildMountPolicy, storage::StorageCapabilities},
};

use bon::Builder;
use getset::{CopyGetters, Getters};
use semver::Version;

/// Driver identity, display name, release version and nesting policy, stable for its lifetime.
///
/// This is declaration data, not a registration wrapper or a storage instance.
#[derive(Debug, Builder, Getters, CopyGetters)]
pub struct DriverInfo {
    #[getset(get = "pub")]
    key: DriverKey,
    #[getset(get = "pub")]
    name: String,
    /// The driver's release version, independent of RPC protocol or WASM ABI versions.
    #[getset(get = "pub")]
    version: Version,
    #[getset(get_copy = "pub")]
    child_mount_policy: ChildMountPolicy,
    /// Implementation upper bound; inspect the opened Storage for instance support.
    #[getset(get_copy = "pub")]
    capabilities: StorageCapabilities,
    /// Stable creation metadata using the shared annotation profile.
    /// Incompatible metadata prevents registration. None disables creation.
    instance_config_schema: Option<InstanceConfigSchema>,
    /// Plugin-wide settings metadata, independent of instance creation parameters.
    settings_schema: Option<DriverSettingsSchema>,
    /// Configuration schema version, independent of software version and record revision.
    #[builder(default = std::num::NonZeroU32::MIN)]
    #[getset(get_copy = "pub")]
    instance_config_version: std::num::NonZeroU32,
}

impl DriverInfo {
    /// Borrow stable plugin-wide settings metadata. Schemas contain no credentials;
    /// x-secret marks values that must be redacted by settings consumers.
    pub fn settings_schema(&self) -> Option<&DriverSettingsSchema> {
        self.settings_schema.as_ref()
    }
    /// Borrow immutable creation metadata under `assethub.config.annotations.v1`;
    /// None means creation is unsupported.
    pub fn instance_config_schema(&self) -> Option<&InstanceConfigSchema> {
        self.instance_config_schema.as_ref()
    }
}
