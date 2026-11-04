//! Stateless driver logic for host-owned instance records.
use super::DriverFuture;
use crate::{
    application::dto::InstanceConfig,
    domain::storage::{StorageError, StorageErrorKind},
};
use std::collections::BTreeMap;

/// Validated normalized parameters and a curated public view. Construction accepts
/// JSON objects only; the provisioner authoritatively validates its own schema.
/// Host registries additionally redact all declared secrets from details.
#[derive(Clone)]
pub struct PreparedConfig {
    config: InstanceConfig,
    details: BTreeMap<String, serde_json::Value>,
}
impl PreparedConfig {
    /// Construct the result after driver-specific validation; both inputs are JSON objects.
    pub fn new(config: InstanceConfig, details: BTreeMap<String, serde_json::Value>) -> Self {
        Self { config, details }
    }
    /// Borrow normalized parameters, which may contain secrets.
    pub fn config(&self) -> &InstanceConfig {
        &self.config
    }
    /// Borrow the curated public view; providers must exclude secrets and their encodings.
    pub fn details(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.details
    }
}
impl std::fmt::Debug for PreparedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedConfig").finish_non_exhaustive()
    }
}

/// Driver-specific validation without driver-owned persistence or storage opens.
pub trait InstanceProvisioner: Send + Sync {
    /// Normalize parameters before persistence. Never return secrets as diagnostics
    /// or open storage; failure must occur before any instance record is written.
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig>;
    /// Optional explicit connectivity probe, separate from get/list and preparation.
    fn test_connection<'a>(&'a self, _config: &'a InstanceConfig) -> DriverFuture<'a, ()> {
        Box::pin(async { Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into()) })
    }
    /// Upgrade one older schema without persisting records or opening storage.
    /// This may await plugin execution. Cancellation must leave host records
    /// unchanged, and a retry may receive the same input. The host revalidates
    /// the result before a revision-checked write. Invalid configuration becomes
    /// InvalidData; storage/transport errors retain their categories. The default
    /// leaves input intact for backward-compatible schemas.
    fn migrate_config(
        &self,
        _from_version: u32,
        config: InstanceConfig,
    ) -> DriverFuture<'_, InstanceConfig> {
        Box::pin(async move { Ok(config) })
    }
}
