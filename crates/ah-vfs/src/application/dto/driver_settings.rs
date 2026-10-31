//! Plugin-wide initialization parameters, distinct from per-instance configuration.
use std::collections::BTreeMap;

/// Host-supplied settings for one driver initialization. Values may contain
/// credentials; Debug omits keys and values. The driver validates its own schema
/// before connecting to its chosen database or allocating persistent resources.
#[derive(Clone, Default, PartialEq)]
pub struct DriverSettings(BTreeMap<String, serde_json::Value>);
impl DriverSettings {
    /// Empty settings for providers requiring no plugin-wide parameters.
    pub fn new() -> Self {
        Self::default()
    }
    /// Extract raw values for authoritative validation inside initialize.
    pub fn into_object(self) -> BTreeMap<String, serde_json::Value> {
        self.0
    }
}
impl std::ops::Deref for DriverSettings {
    type Target = BTreeMap<String, serde_json::Value>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<BTreeMap<String, serde_json::Value>> for DriverSettings {
    fn from(fields: BTreeMap<String, serde_json::Value>) -> Self {
        Self(fields)
    }
}
impl<const N: usize> From<[(String, serde_json::Value); N]> for DriverSettings {
    fn from(fields: [(String, serde_json::Value); N]) -> Self {
        Self(fields.into())
    }
}
impl std::fmt::Debug for DriverSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriverSettings")
            .field("field_count", &self.0.len())
            .finish_non_exhaustive()
    }
}
/// Immutable JSON Schema metadata for plugin-wide settings, including x-secret.
pub type DriverSettingsSchema = BTreeMap<String, serde_json::Value>;
