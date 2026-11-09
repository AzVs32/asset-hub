//! Transport-neutral creation parameters and driver-owned configuration descriptions.

use crate::application::driver_error::{DriverError, InstanceConfigErrorKind};
use std::collections::BTreeMap;

/// An object of driver-specific creation parameters. These are inputs, unlike
/// InstanceInfo details, and may contain secrets. Drivers validate them before
/// side effects. Hosts pass them through without selecting concrete Rust types.
#[derive(Clone, Default, PartialEq)]
pub struct InstanceConfig(BTreeMap<String, serde_json::Value>);
impl InstanceConfig {
    /// Construct an empty object; the driver provisioner validates driver-specific fields.
    pub fn new() -> Self {
        Self::default()
    }
    /// Wrap an object. Value validation belongs to the driver provisioner.
    pub fn from_object(fields: BTreeMap<String, serde_json::Value>) -> Self {
        Self(fields)
    }
    /// Return raw parameters for the owning adapter; callers are responsible for secret handling.
    pub fn into_object(self) -> BTreeMap<String, serde_json::Value> {
        self.0
    }
}
impl std::ops::Deref for InstanceConfig {
    type Target = BTreeMap<String, serde_json::Value>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<BTreeMap<String, serde_json::Value>> for InstanceConfig {
    fn from(fields: BTreeMap<String, serde_json::Value>) -> Self {
        Self(fields)
    }
}
impl<const N: usize> From<[(String, serde_json::Value); N]> for InstanceConfig {
    fn from(fields: [(String, serde_json::Value); N]) -> Self {
        Self(BTreeMap::from(fields))
    }
}
impl std::iter::FromIterator<(String, serde_json::Value)> for InstanceConfig {
    fn from_iter<T: IntoIterator<Item = (String, serde_json::Value)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}
impl std::fmt::Debug for InstanceConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceConfig")
            .field("field_count", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// A driver-owned JSON Schema object describing its creation parameters.
/// The `assethub.config.annotations.v1` profile locates x-secret/x-immutable
/// through properties, items and additionalProperties. References and annotations
/// inside other schema constructs are unsupported and rejected before registration.
/// The host does not evaluate a complete JSON Schema dialect: authoritative
/// value validation stays in the driver or provisioner.
pub type InstanceConfigSchema = BTreeMap<String, serde_json::Value>;

/// Reject unknown parameter names without copying their values into errors.
pub fn validate_instance_config_fields(
    config: &InstanceConfig,
    allowed: &[&str],
) -> Result<(), DriverError> {
    if let Some(field) = config
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(DriverError::InvalidInstanceConfig {
            field: field.clone(),
            kind: InstanceConfigErrorKind::UnknownField,
        });
    }
    Ok(())
}

/// Require one string parameter. Error messages never include parameter values.
pub fn instance_config_string<'a>(
    config: &'a InstanceConfig,
    field: &str,
) -> Result<&'a str, DriverError> {
    let value = config
        .get(field)
        .ok_or_else(|| DriverError::InvalidInstanceConfig {
            field: field.into(),
            kind: InstanceConfigErrorKind::MissingField,
        })?;
    value
        .as_str()
        .ok_or_else(|| DriverError::InvalidInstanceConfig {
            field: field.into(),
            kind: InstanceConfigErrorKind::InvalidType,
        })
}

/// Require an unsigned JSON integer in the u64 range, without lossy conversion.
pub fn instance_config_u64(config: &InstanceConfig, field: &str) -> Result<u64, DriverError> {
    let value = config
        .get(field)
        .ok_or_else(|| DriverError::InvalidInstanceConfig {
            field: field.into(),
            kind: InstanceConfigErrorKind::MissingField,
        })?;
    value
        .as_u64()
        .ok_or_else(|| DriverError::InvalidInstanceConfig {
            field: field.into(),
            kind: if value.is_number() {
                InstanceConfigErrorKind::InvalidValue
            } else {
                InstanceConfigErrorKind::InvalidType
            },
        })
}
