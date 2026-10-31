//! Metadata profile `assethub.config.annotations.v1`, independent of any driver.
//!
//! Config values are objects. The host interprets x-secret/x-immutable only at
//! the root, properties, items and additionalProperties locations. References
//! and annotations hidden in combinators/patterns are unsupported. Other JSON
//! Schema validation remains authoritative inside the driver/provisioner.
use super::dto::InstanceConfigSchema;
use crate::domain::{DriverError, StorageError, StorageErrorKind};
use serde_json::Value;

fn failure(kind: StorageErrorKind) -> DriverError {
    StorageError::new(kind).into()
}
fn invalid() -> DriverError {
    failure(StorageErrorKind::InvalidData)
}

/// Check the shared metadata profile before initializing a driver. This does
/// not validate instance values or perform driver-specific schema evaluation.
pub fn validate_instance_config_schema(schema: &InstanceConfigSchema) -> Result<(), DriverError> {
    validate_annotations(&Value::Object(schema.clone().into_iter().collect()))
}

pub(crate) fn has_annotation(value: &Value, name: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.get(name) == Some(&Value::Bool(true))
                || map.values().any(|v| has_annotation(v, name))
        }
        Value::Array(values) => values.iter().any(|v| has_annotation(v, name)),
        _ => false,
    }
}

fn has_reference(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            ["$ref", "$dynamicRef", "$recursiveRef"].iter().any(|key| map.contains_key(*key))
                || map.values().any(has_reference)
        }
        Value::Array(values) => values.iter().any(has_reference),
        _ => false,
    }
}

// Fail closed for annotations hidden in combinators, references or patterns whose
// instance locations cannot be determined by this deliberately bounded walker.
pub(crate) fn validate_annotations(schema: &Value) -> Result<(), DriverError> {
    if !schema.is_object() && !schema.is_boolean() {
        return Err(invalid());
    }
    if let Some(object) = schema.as_object() {
        for (key, value) in object {
            match key.as_str() {
                "properties" => {
                    let properties = value.as_object().ok_or_else(invalid)?;
                    for child in properties.values() {
                        validate_annotations(child)?;
                    }
                }
                "items" | "additionalProperties" => {
                    if value.is_object() {
                        validate_annotations(value)?;
                    } else if !value.is_boolean() {
                        return Err(failure(StorageErrorKind::UnsupportedOperation));
                    }
                }
                "$ref" | "$dynamicRef" | "$recursiveRef" => {
                    return Err(failure(StorageErrorKind::UnsupportedOperation));
                }
                "x-secret" | "x-immutable" => {
                    if !value.is_boolean() {
                        return Err(invalid());
                    }
                }
                _ if has_reference(value)
                    || has_annotation(value, "x-secret")
                    || has_annotation(value, "x-immutable") => {
                    return Err(failure(StorageErrorKind::UnsupportedOperation));
                }
                _ => {}
            }
        }
    }
    Ok(())
}
