use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tonic::metadata::MetadataMap;

#[derive(Clone)]
pub(crate) struct AuthToken([u8; 32]);
impl AuthToken {
    pub fn new(token: &str) -> Result<Self, DriverError> {
        if token.is_empty()
            || token.len() > 4096
            || !token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(StorageError::new(StorageErrorKind::PermissionDenied).into());
        }
        Ok(Self(
            Sha256::digest(format!("Bearer {token}").as_bytes()).into(),
        ))
    }
    pub fn accepts(&self, metadata: &MetadataMap) -> bool {
        let mut values = metadata.get_all("authorization").iter();
        let Some(value) = values.next() else {
            return false;
        };
        if values.next().is_some() || value.as_bytes().len() > 4103 {
            return false;
        }
        // Compare fixed-size digests so neither the secret's prefix nor its length
        // changes the comparison path. Hashing time depends only on public input.
        let candidate: [u8; 32] = Sha256::digest(value.as_bytes()).into();
        bool::from(self.0.ct_eq(&candidate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type Result = std::result::Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn authorization_requires_one_exact_bearer_value() -> Result {
        let token = AuthToken::new("test-token")?;
        let mut metadata = MetadataMap::new();
        assert!(!token.accepts(&metadata));
        for value in [
            "Bearer test-tokenX",
            "Bearer test-toke",
            "Bearer Xest-token",
            "Basic test-token",
            "test-token",
        ] {
            metadata.insert("authorization", value.parse()?);
            assert!(!token.accepts(&metadata));
        }
        metadata.insert("authorization", "Bearer test-token".parse()?);
        assert!(token.accepts(&metadata));
        metadata.append("authorization", "Bearer test-token".parse()?);
        assert!(!token.accepts(&metadata));
        metadata.insert(
            "authorization",
            format!("Bearer {}", "x".repeat(4097)).parse()?,
        );
        assert!(!token.accepts(&metadata));
        Ok(())
    }

    #[test]
    fn invalid_tokens_cannot_configure_a_server() {
        for value in ["", "has space", "line\r\nbreak", "令牌"] {
            assert!(AuthToken::new(value).is_err());
        }
        assert!(AuthToken::new(&"x".repeat(4097)).is_err());
        assert!(AuthToken::new(&"x".repeat(4096)).is_ok());
    }
}
