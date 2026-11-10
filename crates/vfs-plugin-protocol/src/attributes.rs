//! Transfer attribute control fields separately from owned binary data.
use super::{Empty, MAX_ATTRIBUTE_NAMES_BYTES, MAX_DATA_BYTES, Reply, StorageOp, Value};
use ah_vfs_storage as d;
use std::num::NonZeroUsize;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AttributeValueError {
    #[error("not an attribute operation")]
    Operation,
    #[error("invalid attribute lookup")]
    Lookup(#[from] super::LookupValueError),
    #[error("invalid permission bits")]
    Permissions(#[from] d::PermissionsError),
    #[error("invalid attribute name")]
    Name(#[from] d::XattrNameError),
    #[error("attribute name list is not UTF-8")]
    NameEncoding(#[from] std::str::Utf8Error),
    #[error("invalid attribute name list")]
    NameList(#[from] d::XattrListError),
    #[error("invalid attribute write mode")]
    Mode,
    #[error("attribute limit is zero or exceeds the transport bound")]
    Limit,
    #[error("invalid attribute data")]
    Data,
}

pub fn encode_attribute(
    request: d::AttributeRequest,
    base: &d::StoragePath,
    links: d::IntermediateSymlinks,
) -> Result<(StorageOp, Vec<u8>), AttributeValueError> {
    use d::AttributeAction as A;
    let (operand, action) = request.into_parts();
    let lookup = super::encode_lookup(&operand, base, links);
    let op = match action {
        A::GetPermissions => StorageOp::GetPermissions(lookup),
        A::SetPermissions(mode) => StorageOp::SetPermissions(super::SetPermissions {
            lookup: Some(lookup),
            mode: mode.bits(),
        }),
        A::ListXattrs { max_bytes, .. } => StorageOp::ListXattrs(super::ListXattrs {
            lookup: Some(lookup),
            max_bytes: encode_limit(max_bytes, MAX_ATTRIBUTE_NAMES_BYTES)?,
        }),
        A::GetXattr {
            name, max_bytes, ..
        } => StorageOp::GetXattr(super::GetXattr {
            lookup: Some(lookup),
            name: name.as_str().into(),
            max_bytes: encode_limit(max_bytes, MAX_DATA_BYTES)?,
        }),
        A::SetXattr {
            name, value, mode, ..
        } => {
            if value.len() > MAX_DATA_BYTES {
                return Err(AttributeValueError::Limit);
            }
            let mode = match mode {
                d::XattrWriteMode::Upsert => super::XattrWriteMode::Upsert,
                d::XattrWriteMode::CreateNew => super::XattrWriteMode::CreateNew,
                d::XattrWriteMode::Replace => super::XattrWriteMode::Replace,
            };
            return Ok((
                StorageOp::SetXattr(super::SetXattr {
                    lookup: Some(lookup),
                    name: name.as_str().into(),
                    mode: mode.into(),
                }),
                value,
            ));
        }
        A::RemoveXattr(name) => StorageOp::RemoveXattr(super::RemoveXattr {
            lookup: Some(lookup),
            name: name.as_str().into(),
        }),
        _ => return Err(AttributeValueError::Operation),
    };
    Ok((op, vec![]))
}

pub fn decode_attribute(
    op: StorageOp,
    data: Vec<u8>,
) -> Result<(d::AttributeRequest, d::StoragePath, d::IntermediateSymlinks), AttributeValueError> {
    use d::AttributeAction as A;
    if data.len() > MAX_DATA_BYTES || !data.is_empty() && !matches!(op, StorageOp::SetXattr(_)) {
        return Err(AttributeValueError::Data);
    }
    let (lookup, action) = match op {
        StorageOp::GetPermissions(lookup) => (Some(lookup), A::GetPermissions),
        StorageOp::SetPermissions(v) => (v.lookup, A::SetPermissions(d::Permissions::new(v.mode)?)),
        StorageOp::ListXattrs(v) => (
            v.lookup,
            A::list_xattrs(decode_limit(v.max_bytes, MAX_ATTRIBUTE_NAMES_BYTES)?),
        ),
        StorageOp::GetXattr(v) => (
            v.lookup,
            A::get_xattr(
                d::XattrName::parse(&v.name)?,
                decode_limit(v.max_bytes, MAX_DATA_BYTES)?,
            ),
        ),
        StorageOp::SetXattr(v) => {
            let mode = match super::XattrWriteMode::try_from(v.mode) {
                Ok(super::XattrWriteMode::Upsert) => d::XattrWriteMode::Upsert,
                Ok(super::XattrWriteMode::CreateNew) => d::XattrWriteMode::CreateNew,
                Ok(super::XattrWriteMode::Replace) => d::XattrWriteMode::Replace,
                _ => return Err(AttributeValueError::Mode),
            };
            (
                v.lookup,
                A::set_xattr(d::XattrName::parse(&v.name)?, data, mode),
            )
        }
        StorageOp::RemoveXattr(v) => (v.lookup, A::RemoveXattr(d::XattrName::parse(&v.name)?)),
        _ => return Err(AttributeValueError::Operation),
    };
    let (operand, base, links) = super::decode_lookup(lookup)?;
    Ok((d::AttributeRequest::new(operand, action), base, links))
}

// Negotiate the effective per-call response limit down to the transport bound.
// The backend still returns the complete result or LimitExceeded.
fn encode_limit(limit: NonZeroUsize, cap: usize) -> Result<u32, AttributeValueError> {
    u32::try_from(limit.get().min(cap)).map_err(|_| AttributeValueError::Limit)
}

fn decode_limit(limit: u32, cap: usize) -> Result<NonZeroUsize, AttributeValueError> {
    usize::try_from(limit)
        .ok()
        .filter(|size| *size <= cap)
        .and_then(NonZeroUsize::new)
        .ok_or(AttributeValueError::Limit)
}

pub fn encode_attribute_result(
    expected: d::AttributeExpectation,
    result: d::AttributeResult,
) -> Result<Reply, d::ContractViolation> {
    use d::AttributeResult as A;
    expected.validate(&result)?;
    let invalid = d::ContractViolation::InvalidAttributes;
    let (value, data) = match result {
        A::Permissions(mode) => (
            Value::Permissions(super::Permissions { mode: mode.bits() }),
            vec![],
        ),
        A::Names(names) => {
            let size = names.encoded_len();
            if size > MAX_ATTRIBUTE_NAMES_BYTES {
                return Err(invalid);
            }
            let mut bytes = Vec::with_capacity(size);
            for name in names.into_names() {
                bytes.extend_from_slice(name.as_str().as_bytes());
                bytes.push(0);
            }
            (Value::Names(Empty {}), bytes)
        }
        A::Value(bytes) if bytes.len() <= MAX_DATA_BYTES => (Value::Bytes(Empty {}), bytes),
        A::Value(_) => return Err(invalid),
        A::Unit => (Value::Unit(Empty {}), vec![]),
    };
    Ok(Reply {
        result: Some(value),
        data,
    })
}

pub fn decode_attribute_result(
    expected: d::AttributeExpectation,
    reply: Reply,
) -> Result<d::AttributeResult, AttributeValueError> {
    use d::{AttributeExpectation as E, AttributeResult as A};
    let invalid = AttributeValueError::Data;
    match (expected, reply.result) {
        (E::Permissions, Some(Value::Permissions(value))) if reply.data.is_empty() => {
            d::Permissions::new(value.mode)
                .map(A::Permissions)
                .map_err(AttributeValueError::from)
        }
        (E::Unit, Some(Value::Unit(_))) if reply.data.is_empty() => Ok(A::Unit),
        (E::Value(limit), Some(Value::Bytes(_)))
            if reply.data.len() <= limit.get().min(MAX_DATA_BYTES) =>
        {
            Ok(A::Value(reply.data))
        }
        (E::Names(limit), Some(Value::Names(_)))
            if reply.data.len() <= limit.get().min(MAX_ATTRIBUTE_NAMES_BYTES) =>
        {
            let names = std::str::from_utf8(&reply.data).map_err(AttributeValueError::from)?;
            if !names.is_empty() && !names.ends_with('\0') {
                return Err(invalid);
            }
            let mut decoded = Vec::new();
            let mut previous = "";
            for name in names.split_terminator('\0') {
                if !name.is_empty() && name <= previous {
                    return Err(d::XattrListError::NotOrdered.into());
                }
                decoded.push(d::XattrName::parse(name).map_err(AttributeValueError::from)?);
                previous = name;
            }
            d::XattrList::new(decoded, limit)
                .map(A::Names)
                .map_err(AttributeValueError::from)
        }
        _ => Err(invalid),
    }
}
