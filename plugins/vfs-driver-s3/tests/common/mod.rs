use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
use ah_vfs::application::port::{DriverFuture, SecretCipher, SecretContext};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Cipher {
    key: Aes256Gcm,
    nonce: AtomicU64,
}
impl Cipher {
    pub fn new() -> Self {
        Self {
            key: Aes256Gcm::new(&[31; 32].into()),
            nonce: AtomicU64::new(1),
        }
    }
}
fn aad(context: &SecretContext) -> Vec<u8> {
    serde_json::to_vec(&(
        context.driver_key().as_str(),
        context.instance_id().as_str(),
        context.pointer(),
    ))
    .unwrap_or_default()
}
impl SecretCipher for Cipher {
    fn encrypt<'a>(
        &'a self,
        plaintext: &'a [u8],
        context: SecretContext,
    ) -> DriverFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let mut nonce = [0; 12];
            nonce[4..].copy_from_slice(&self.nonce.fetch_add(1, Ordering::Relaxed).to_be_bytes());
            let mut output = nonce.to_vec();
            output.extend(
                self.key
                    .encrypt(
                        Nonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad: &aad(&context),
                        },
                    )
                    .map_err(|_| LifecycleError::new(LifecycleFailure::InvalidRecord))?,
            );
            Ok(output)
        })
    }
    fn decrypt<'a>(
        &'a self,
        ciphertext: &'a [u8],
        context: SecretContext,
    ) -> DriverFuture<'a, Vec<u8>> {
        Box::pin(async move {
            if ciphertext.len() < 12 {
                return Err(LifecycleError::new(LifecycleFailure::InvalidRecord).into());
            }
            self.key
                .decrypt(
                    Nonce::from_slice(&ciphertext[..12]),
                    Payload {
                        msg: &ciphertext[12..],
                        aad: &aad(&context),
                    },
                )
                .map_err(|_| LifecycleError::new(LifecycleFailure::InvalidRecord).into())
        })
    }
}
