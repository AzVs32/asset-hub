use ah_vfs_storage as d;
pub fn encode_failure(
    error: &d::StorageError,
) -> Result<vfs_plugin_protocol::StorageFailure, d::ContractViolation> {
    vfs_plugin_protocol::encode_storage_error(error)
}

pub fn decode_failure(
    expected: d::Operation,
    failure: vfs_plugin_protocol::StorageFailure,
) -> Result<d::StorageError, d::ContractViolation> {
    let error = vfs_plugin_protocol::decode_storage_error(failure)?;
    if error.operation() != expected {
        return Err(d::ContractViolation::InvalidResponse);
    }
    Ok(error)
}
