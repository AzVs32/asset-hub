use argon2::password_hash::{SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHash, PasswordHasher as _, PasswordVerifier as _};
use asset_core::{CoreError, port::PasswordHasher};

#[derive(Debug, Clone, Default)]
pub struct Argon2PasswordHasher;

impl PasswordHasher for Argon2PasswordHasher {
    fn hash(&self, password: &str) -> Result<String, CoreError> {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|h| h.to_string())
            .map_err(|e| CoreError::configuration(format!("password hashing failed: {e}")))
    }
    fn verify(&self, password: &str, hash: &str) -> Result<bool, CoreError> {
        let hash = PasswordHash::new(hash).map_err(|e| {
            CoreError::configuration(format!("stored password hash is invalid: {e}"))
        })?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok())
    }
}
