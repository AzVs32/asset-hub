use crate::CoreError;

pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: &str) -> Result<String, CoreError>;
    fn verify(&self, password: &str, hash: &str) -> Result<bool, CoreError>;
}
