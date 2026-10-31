//! Validated instance identity supplied when opening a storage handle.
use crate::{application::dto::InstanceConfig, domain::InstanceId};

/// An instance opening request. Configuration is absent for self-managed drivers,
/// which resolve their own records. Identity is validated by InstanceId.
pub struct OpenContext {
    instance_id: InstanceId,
    config: Option<InstanceConfig>,
}
impl OpenContext {
    /// Open a self-managed instance by its already validated identity.
    pub fn self_managed(instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            config: None,
        }
    }
    /// Host-supplied decrypted configuration; schema validation precedes this call.
    pub fn host_managed(instance_id: InstanceId, config: InstanceConfig) -> Self {
        Self {
            instance_id,
            config: Some(config),
        }
    }
    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }
    pub fn config(&self) -> Option<&InstanceConfig> {
        self.config.as_ref()
    }
}
impl From<InstanceId> for OpenContext {
    fn from(id: InstanceId) -> Self {
        Self::self_managed(id)
    }
}
impl std::fmt::Debug for OpenContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenContext")
            .field("instance_id", &self.instance_id)
            .field("config", &self.config.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}
