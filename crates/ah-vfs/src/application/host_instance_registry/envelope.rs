//! Private persisted representation of encrypted parameters and public details.

use crate::domain::{
    DriverError,
    storage::{StorageError, StorageErrorKind},
};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone)]
struct EncryptedSecret {
    path: String,
    ciphertext: Vec<u8>,
}

/// Parsed once at the record boundary, including pagination lookahead records.
/// No Debug implementation: parameters and ciphertext are never log fields.
#[derive(Clone)]
pub(super) struct Envelope {
    json: String,
    parameters: Value,
    secrets: Vec<EncryptedSecret>,
    details: BTreeMap<String, Value>,
}

fn invalid() -> DriverError {
    StorageError::new(StorageErrorKind::InvalidData).into()
}

impl Envelope {
    pub(super) fn parse(json: String) -> Result<Self, DriverError> {
        let mut fields: serde_json::Map<String, Value> = serde_json::from_str(&json)
            .map_err(|source| StorageError::with_source(StorageErrorKind::InvalidData, source))?;
        let parameters = fields.remove("parameters").ok_or_else(invalid)?;
        let paths: Vec<String> = serde_json::from_value(
            fields.remove("secret_paths").ok_or_else(invalid)?,
        )
        .map_err(|source| StorageError::with_source(StorageErrorKind::InvalidData, source))?;
        let details = serde_json::from_value(fields.remove("details").ok_or_else(invalid)?)
            .map_err(|source| StorageError::with_source(StorageErrorKind::InvalidData, source))?;
        let secrets = Self::validate_secrets(&parameters, paths)?;
        Ok(Self {
            json,
            parameters,
            secrets,
            details,
        })
    }

    pub(super) fn new(
        parameters: Value,
        paths: Vec<String>,
        details: BTreeMap<String, Value>,
    ) -> Result<Self, DriverError> {
        let secrets = Self::validate_secrets(&parameters, paths)?;
        let paths: Vec<_> = secrets.iter().map(|secret| secret.path.as_str()).collect();
        let json = serde_json::to_string(&serde_json::json!({
            "parameters": parameters,
            "secret_paths": paths,
            "details": details,
        }))
        .map_err(|source| StorageError::with_source(StorageErrorKind::InvalidData, source))?;
        Ok(Self {
            json,
            parameters,
            secrets,
            details,
        })
    }

    fn validate_secrets(
        parameters: &Value,
        paths: Vec<String>,
    ) -> Result<Vec<EncryptedSecret>, DriverError> {
        if !parameters.is_object() && !(paths.len() == 1 && paths[0].is_empty()) {
            return Err(invalid());
        }
        let mut secrets: Vec<EncryptedSecret> = Vec::with_capacity(paths.len());
        for path in paths {
            if secrets.iter().any(|other| {
                path == other.path
                    || path.starts_with(&format!("{}/", other.path))
                    || other.path.starts_with(&format!("{path}/"))
            }) {
                return Err(invalid());
            }
            let ciphertext =
                serde_json::from_value(parameters.pointer(&path).ok_or_else(invalid)?.clone())
                    .map_err(|source| {
                        StorageError::with_source(StorageErrorKind::InvalidData, source)
                    })?;
            secrets.push(EncryptedSecret { path, ciphertext });
        }
        Ok(secrets)
    }

    pub(super) fn json(&self) -> &str {
        &self.json
    }

    pub(super) fn parameters(&self) -> &Value {
        &self.parameters
    }

    pub(super) fn secrets(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.secrets
            .iter()
            .map(|secret| (secret.path.as_str(), secret.ciphertext.as_slice()))
    }

    pub(super) fn details(&self) -> &BTreeMap<String, Value> {
        &self.details
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn corrupted_records_keep_the_json_cause_without_exposing_the_record()
    -> Result<(), Box<dyn Error>> {
        let error = Envelope::parse("private malformed record".into())
            .err()
            .ok_or("malformed record accepted")?;
        let storage = error.storage_error().ok_or("storage error")?;
        assert_eq!(storage.kind(), StorageErrorKind::InvalidData);
        assert!(
            storage
                .source()
                .and_then(|e| e.downcast_ref::<serde_json::Error>())
                .is_some()
        );
        assert!(!storage.to_string().contains("private"));
        assert!(!format!("{storage:?}").contains("private"));
        Ok(())
    }
}
