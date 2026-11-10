use ah_vfs_storage as d;
pub fn encode_entry(entry: d::Entry) -> Result<vfs_plugin_protocol::Entry, d::ContractViolation> {
    vfs_plugin_protocol::encode_entry(&entry).map_err(|_| d::ContractViolation::InvalidEntry)
}
pub fn decode_entry(entry: vfs_plugin_protocol::Entry) -> Result<d::Entry, d::ContractViolation> {
    vfs_plugin_protocol::decode_entry(entry).map_err(|_| d::ContractViolation::InvalidEntry)
}

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
