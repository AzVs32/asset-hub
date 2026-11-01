//! Validated identity and decrypted configuration supplied by the host.
use crate::{application::dto::InstanceConfig, domain::InstanceId};
/// A host-authorized storage open. Configuration is always present.
pub struct OpenContext {
    instance_id: InstanceId,
    config: InstanceConfig,
}
impl OpenContext {
    pub fn new(instance_id: InstanceId, config: InstanceConfig) -> Self {
        Self {
            instance_id,
            config,
        }
    }
    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }
    pub fn config(&self) -> &InstanceConfig {
        &self.config
    }
}
impl std::fmt::Debug for OpenContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenContext")
            .field("instance_id", &self.instance_id)
            .field("config", &"[REDACTED]")
            .finish()
    }
}
