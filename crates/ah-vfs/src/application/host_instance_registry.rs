//! Host-managed records with encrypted secrets and independently versioned schemas.

mod envelope;
pub(super) mod record;

use super::schema::{has_annotation, validate_annotations};
use envelope::Envelope;

use super::{
    driver_info::DriverInfo,
    dto::{
        InstanceConfig, InstanceConfigSchema, InstanceInfo, InstancePage, InstanceQuery,
        RemoveInstanceOutcome,
    },
    port::{
        DriverFuture, InstanceProvisioner, InstanceRecord, InstanceRecordRepository, OpenContext,
        PreparedConfig, SecretCipher, SecretContext,
    },
};
use crate::domain::{
    DriverError, DriverInstance, DriverKey, InstanceId, StorageError, StorageErrorKind,
    new_instance_id,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

fn failure(kind: StorageErrorKind) -> DriverError {
    StorageError::new(kind).into()
}
fn cipher_failure(source: DriverError) -> DriverError {
    let kind = match &source {
        DriverError::Storage(error) => error.kind(),
        _ => StorageErrorKind::Backend,
    };
    StorageError::with_source(kind, source).into()
}
fn invalid() -> DriverError {
    failure(StorageErrorKind::InvalidData)
}
fn migration_failure(source: DriverError) -> DriverError {
    match source {
        error @ DriverError::Storage(_) => error,
        error => StorageError::with_source(StorageErrorKind::InvalidData, error).into(),
    }
}
fn pointer(base: &str, key: &str) -> String {
    format!("{base}/{}", key.replace('~', "~0").replace('/', "~1"))
}
fn paths(schema: &Value, value: &Value, name: &str, base: &str, output: &mut Vec<String>) {
    if schema.get(name) == Some(&Value::Bool(true)) {
        output.push(base.into());
        return;
    }
    if let Some(object) = value.as_object() {
        for (key, child) in object {
            if let Some(rule) = schema
                .get("properties")
                .and_then(|p| p.get(key))
                .or_else(|| schema.get("additionalProperties").filter(|p| p.is_object()))
            {
                paths(rule, child, name, &pointer(base, key), output);
            }
        }
    }
    if let (Some(array), Some(rule)) = (value.as_array(), schema.get("items")) {
        for (index, child) in array.iter().enumerate() {
            paths(
                rule,
                child,
                name,
                &pointer(base, &index.to_string()),
                output,
            );
        }
    }
}
fn redaction_values(value: &Value, values: &mut Vec<Value>) {
    values.push(value.clone());
    match value {
        Value::Object(map) => {
            for child in map.values() {
                redaction_values(child, values)
            }
        }
        Value::Array(array) => {
            for child in array {
                redaction_values(child, values)
            }
        }
        _ => {}
    }
}
fn redact(value: &mut Value, secrets: &[Value]) {
    if secrets.iter().any(|s| s == value) {
        *value = Value::String("[REDACTED]".into());
        return;
    }
    match value {
        Value::String(text) => {
            for secret in secrets
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                *text = text.replace(secret, "[REDACTED]");
            }
        }
        Value::Object(map) => {
            let previous = std::mem::take(map);
            for (key, mut child) in previous {
                let mut redacted_key = Value::String(key);
                redact(&mut redacted_key, secrets);
                redact(&mut child, secrets);
                if let Some(key) = redacted_key.as_str() {
                    map.insert(key.to_owned(), child);
                }
            }
        }
        Value::Array(array) => {
            for child in array {
                redact(child, secrets)
            }
        }
        _ => {}
    }
}
fn now() -> Result<u64, DriverError> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid())?
            .as_millis(),
    )
    .map_err(|_| invalid())
}

/// One driver's host-owned registry. Supports nested properties, array items and
/// additionalProperties annotations; ambiguous schema constructs fail closed.
/// Public get/list use persisted redacted details and never call prepare/open.
pub struct HostInstanceRegistry {
    key: DriverKey,
    schema: Option<InstanceConfigSchema>,
    version: u32,
    provisioner: Arc<dyn InstanceProvisioner>,
    repository: Arc<dyn InstanceRecordRepository>,
    cipher: Option<Arc<dyn SecretCipher>>,
}
impl HostInstanceRegistry {
    pub fn new(
        info: &DriverInfo,
        provisioner: Arc<dyn InstanceProvisioner>,
        repository: Arc<dyn InstanceRecordRepository>,
        cipher: Option<Arc<dyn SecretCipher>>,
    ) -> Self {
        Self {
            key: info.key().clone(),
            schema: info.instance_config_schema().cloned(),
            version: info.instance_config_version().get(),
            provisioner,
            repository,
            cipher,
        }
    }
    fn reference(&self, id: InstanceId) -> DriverInstance {
        DriverInstance::new(self.key.clone(), id)
    }
    fn schema(&self) -> Result<Value, DriverError> {
        let schema = serde_json::to_value(
            self.schema
                .as_ref()
                .ok_or_else(|| failure(StorageErrorKind::UnsupportedOperation))?,
        )
        .map_err(|_| invalid())?;
        validate_annotations(&schema)?;
        if has_annotation(&schema, "x-secret") && self.cipher.is_none() {
            return Err(failure(StorageErrorKind::UnsupportedOperation));
        }
        Ok(schema)
    }
    fn info(record: &InstanceRecord) -> InstanceInfo {
        InstanceInfo {
            id: record.instance().instance_id().clone(),
            details: record.envelope().details().clone(),
        }
    }
    fn secret_context(&self, id: &InstanceId, path: String) -> SecretContext {
        SecretContext::new(self.key.clone(), id.clone(), path)
    }
    async fn decode_config(&self, record: &InstanceRecord) -> Result<InstanceConfig, DriverError> {
        let envelope = record.envelope();
        let mut value = envelope.parameters().clone();
        for (path, ciphertext) in envelope.secrets() {
            let cipher = self
                .cipher
                .as_ref()
                .ok_or_else(|| failure(StorageErrorKind::UnsupportedOperation))?;
            let target = value.pointer_mut(path).ok_or_else(invalid)?;
            let plaintext = cipher
                .decrypt(
                    ciphertext,
                    self.secret_context(record.instance().instance_id(), path.into()),
                )
                .await
                .map_err(cipher_failure)?;
            *target = serde_json::from_slice(&plaintext).map_err(|_| invalid())?;
        }
        let fields: BTreeMap<String, Value> =
            serde_json::from_value(value).map_err(|_| invalid())?;
        Ok(fields.into())
    }
    async fn prepare(&self, config: InstanceConfig) -> Result<PreparedConfig, DriverError> {
        match self.provisioner.prepare(config.clone()).await {
            Ok(prepared) => Ok(prepared),
            Err(DriverError::InvalidInstanceConfig { field, kind }) => {
                let schema = self.schema()?;
                let value = serde_json::to_value(config.into_object()).map_err(|_| invalid())?;
                let mut locations = Vec::new();
                paths(&schema, &value, "x-secret", "", &mut locations);
                let mut secrets = Vec::new();
                for path in locations {
                    if let Some(secret) = value.pointer(&path) {
                        redaction_values(secret, &mut secrets);
                    }
                }
                let mut field = Value::String(field);
                redact(&mut field, &secrets);
                Err(DriverError::InvalidInstanceConfig {
                    field: field.as_str().unwrap_or("[REDACTED]").into(),
                    kind,
                })
            }
            Err(error @ DriverError::Storage(_)) => Err(error),
            Err(error) => {
                Err(StorageError::with_source(StorageErrorKind::InvalidRequest, error).into())
            }
        }
    }
    async fn encode(
        &self,
        id: &InstanceId,
        prepared: PreparedConfig,
        raw: &InstanceConfig,
    ) -> Result<Envelope, DriverError> {
        let schema = self.schema()?;
        let mut value =
            serde_json::to_value(prepared.config().clone().into_object()).map_err(|_| invalid())?;
        let mut secret_paths = Vec::new();
        paths(&schema, &value, "x-secret", "", &mut secret_paths);
        let mut secrets = Vec::new();
        let raw_value = serde_json::to_value(raw.clone().into_object()).map_err(|_| invalid())?;
        let mut raw_paths = Vec::new();
        paths(&schema, &raw_value, "x-secret", "", &mut raw_paths);
        for path in raw_paths {
            if let Some(secret) = raw_value.pointer(&path) {
                redaction_values(secret, &mut secrets);
            }
        }
        for path in &secret_paths {
            let cipher = self
                .cipher
                .as_ref()
                .ok_or_else(|| failure(StorageErrorKind::UnsupportedOperation))?;
            let target = value.pointer_mut(path).ok_or_else(invalid)?;
            redaction_values(target, &mut secrets);
            let plaintext = serde_json::to_vec(target).map_err(|_| invalid())?;
            let ciphertext = cipher
                .encrypt(&plaintext, self.secret_context(id, path.clone()))
                .await
                .map_err(cipher_failure)?;
            *target = serde_json::to_value(ciphertext).map_err(|_| invalid())?;
        }
        let mut details = serde_json::to_value(prepared.details()).map_err(|_| invalid())?;
        redact(&mut details, &secrets);
        if !details.is_object() {
            details = serde_json::json!({});
        }
        Envelope::new(
            value,
            secret_paths,
            serde_json::from_value(details).map_err(|_| invalid())?,
        )
    }
    async fn load(&self, id: &InstanceId) -> Result<InstanceRecord, DriverError> {
        self.repository
            .get(self.reference(id.clone()))
            .await?
            .ok_or_else(|| DriverError::InstanceNotFound(id.clone()))
    }
    /// Replace normalized parameters. Immutable annotations are compared after
    /// preparation; revision and updated_at advance independently of schema version.
    pub async fn update(
        &self,
        id: InstanceId,
        config: InstanceConfig,
    ) -> Result<InstanceInfo, DriverError> {
        let schema = self.schema()?;
        let (old, previous) = self.load_current(&id).await?;
        let prepared = self.prepare(config.clone()).await?;
        let old_value = serde_json::to_value(previous.into_object()).map_err(|_| invalid())?;
        let new_value =
            serde_json::to_value(prepared.config().clone().into_object()).map_err(|_| invalid())?;
        let mut immutable = Vec::new();
        paths(&schema, &old_value, "x-immutable", "", &mut immutable);
        paths(&schema, &new_value, "x-immutable", "", &mut immutable);
        if immutable
            .iter()
            .any(|path| old_value.pointer(path) != new_value.pointer(path))
        {
            return Err(failure(StorageErrorKind::InvalidRequest));
        }
        let envelope = self.encode(&id, prepared, &config).await?;
        let next = InstanceRecord::from_envelope(
            self.reference(id.clone()),
            self.version,
            old.revision().checked_add(1).ok_or_else(invalid)?,
            envelope,
            old.created_at(),
            now()?.max(old.updated_at().checked_add(1).ok_or_else(invalid)?),
        )?;
        let info = Self::info(&next);
        self.repository.update(next, old.revision()).await?;
        Ok(info)
    }
    /// Decrypt and migrate only the requested instance. Invalid configuration
    /// returns InvalidData; runtime failures retain their storage category.
    /// Failure or cancellation before persistence leaves records unchanged.
    /// Cancellation during persistence can have an unknown write outcome.
    pub async fn open_context(&self, id: InstanceId) -> Result<OpenContext, DriverError> {
        let (_, config) = self.load_current(&id).await?;
        Ok(OpenContext::new(id, config))
    }

    /// Keep decrypted parameters and the revision used for writes in one snapshot.
    async fn load_current(
        &self,
        id: &InstanceId,
    ) -> Result<(InstanceRecord, InstanceConfig), DriverError> {
        let mut record = self.load(id).await?;
        let mut config = self.decode_config(&record).await?;
        if record.config_version() > self.version {
            return Err(invalid());
        }
        if record.config_version() < self.version {
            config = self
                .provisioner
                .migrate_config(record.config_version(), config)
                .await
                .map_err(migration_failure)?;
            let prepared = self
                .provisioner
                .prepare(config.clone())
                .await
                .map_err(migration_failure)?;
            let envelope = self.encode(id, prepared, &config).await?;
            let migrated = InstanceRecord::from_envelope(
                self.reference(id.clone()),
                self.version,
                record.revision().checked_add(1).ok_or_else(invalid)?,
                envelope,
                record.created_at(),
                now()?.max(record.updated_at().checked_add(1).ok_or_else(invalid)?),
            )?;
            config = self.decode_config(&migrated).await?;
            self.repository
                .update(migrated.clone(), record.revision())
                .await?;
            record = migrated;
        }
        Ok((record, config))
    }
}
impl HostInstanceRegistry {
    pub fn create(&self, config: InstanceConfig) -> DriverFuture<'_, InstanceInfo> {
        Box::pin(async move {
            self.schema()?;
            let id = new_instance_id()?;
            let time = now()?;
            let prepared = self.prepare(config.clone()).await?;
            let envelope = self.encode(&id, prepared, &config).await?;
            let record = InstanceRecord::from_envelope(
                self.reference(id),
                self.version,
                1,
                envelope,
                time,
                time,
            )?;
            let info = Self::info(&record);
            self.repository.insert(record).await?;
            Ok(info)
        })
    }
    pub fn get(&self, id: InstanceId) -> DriverFuture<'_, Option<InstanceInfo>> {
        Box::pin(async move {
            Ok(self
                .repository
                .get(self.reference(id))
                .await?
                .as_ref()
                .map(Self::info))
        })
    }
    pub fn list(&self, query: InstanceQuery) -> DriverFuture<'_, InstancePage> {
        Box::pin(async move {
            let page = self.repository.list(self.key.clone(), query).await?;
            Ok(InstancePage {
                instances: page.records.iter().map(Self::info).collect(),
                next_after: page.next_after,
            })
        })
    }
    pub fn remove(&self, id: InstanceId) -> DriverFuture<'_, RemoveInstanceOutcome> {
        self.repository.remove(self.reference(id))
    }
}
