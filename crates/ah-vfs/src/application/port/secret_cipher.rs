//! Host-supplied authenticated encryption for instance secrets.
use super::DriverFuture;
use crate::domain::{DriverKey, InstanceId};

/// Associated-data identity. Ciphers must authenticate the driver, instance and
/// JSON-pointer location to prevent ciphertext substitution across records/fields.
#[derive(Clone, Debug)]
pub struct SecretContext {
    driver: DriverKey,
    instance: InstanceId,
    pointer: String,
}
impl SecretContext {
    pub(crate) fn new(driver: DriverKey, instance: InstanceId, pointer: String) -> Self {
        Self {
            driver,
            instance,
            pointer,
        }
    }
    pub fn driver_key(&self) -> &DriverKey {
        &self.driver
    }
    pub fn instance_id(&self) -> &InstanceId {
        &self.instance
    }
    pub fn pointer(&self) -> &str {
        &self.pointer
    }
}

/// Authenticated encryption supplied by the host, including key management and
/// nonce generation. Implementations must never log plaintext or credentials.
/// Missing ciphers reject secret-bearing schemas before persistence; the core
/// intentionally supplies no placeholder or insecure default cipher.
pub trait SecretCipher: Send + Sync {
    /// Encrypt and authenticate parameters plus context; return opaque bytes including nonce/key metadata.
    fn encrypt<'a>(
        &'a self,
        plaintext: &'a [u8],
        context: SecretContext,
    ) -> DriverFuture<'a, Vec<u8>>;
    /// Authenticate the same context before returning plaintext; failures preserve domain categories.
    fn decrypt<'a>(
        &'a self,
        ciphertext: &'a [u8],
        context: SecretContext,
    ) -> DriverFuture<'a, Vec<u8>>;
}
