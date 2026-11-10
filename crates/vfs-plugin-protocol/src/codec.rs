use super::*;
use prost::Message;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    InvalidData,
    Decode(prost::DecodeError),
    TooLarge,
}
impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid or oversized plugin protobuf message")
    }
}
impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(source) => Some(source),
            _ => None,
        }
    }
}
/// Default classification for malformed request input. Response encoders and
/// decoders classify the actual operation and preserve its mutation outcome separately.
impl From<CodecError> for Failure {
    fn from(error: CodecError) -> Self {
        Self::new(match error {
            CodecError::InvalidData | CodecError::Decode(_) => FailureCode::InvalidRequest,
            CodecError::TooLarge => FailureCode::TooLarge,
        })
    }
}
type Result<T> = std::result::Result<T, CodecError>;
const MAX_VALUE_DEPTH: usize = 24;
fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or(CodecError::InvalidData)
}
fn field<T>(value: &Option<T>) -> Result<&T> {
    required(value.as_ref())
}
fn bound(size: usize, data: &[u8]) -> Result<()> {
    if data.len() > MAX_DATA_BYTES || size.saturating_sub(data.len()) > MAX_CONTROL_BYTES {
        Err(CodecError::TooLarge)
    } else {
        Ok(())
    }
}

// JSON conversion belongs only at the application/configuration boundary.
impl From<JsonObject> for Object {
    fn from(value: JsonObject) -> Self {
        Self {
            fields: value
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        }
    }
}
impl<const N: usize> From<[(String, serde_json::Value); N]> for Object {
    fn from(value: [(String, serde_json::Value); N]) -> Self {
        JsonObject::from(value).into()
    }
}
impl From<serde_json::Value> for JsonValue {
    fn from(value: serde_json::Value) -> Self {
        use json_value::Kind;
        Self {
            kind: Some(match value {
                serde_json::Value::Null => Kind::NullValue(Empty {}),
                serde_json::Value::Bool(v) => Kind::Boolean(v),
                serde_json::Value::String(v) => Kind::Text(v),
                serde_json::Value::Number(v) => {
                    if let Some(v) = v.as_u64() {
                        Kind::UnsignedInteger(v)
                    } else if let Some(v) = v.as_i64() {
                        Kind::Integer(v)
                    } else {
                        Kind::Real(v.as_f64().unwrap_or(f64::NAN))
                    }
                }
                serde_json::Value::Object(v) => {
                    Kind::Object(v.into_iter().collect::<JsonObject>().into())
                }
                serde_json::Value::Array(v) => Kind::Array(Array {
                    values: v.into_iter().map(Into::into).collect(),
                }),
            }),
        }
    }
}
impl Object {
    pub fn new() -> Self {
        Self::default()
    }
}
pub fn json_object(value: Object) -> Result<JsonObject> {
    decode_object(value, 0)
}
fn decode_object(value: Object, depth: usize) -> Result<JsonObject> {
    if depth > MAX_VALUE_DEPTH {
        return Err(CodecError::InvalidData);
    }
    value
        .fields
        .into_iter()
        .map(|(key, value)| Ok((key, decode_json(value, depth + 1)?)))
        .collect()
}
fn decode_json(value: JsonValue, depth: usize) -> Result<serde_json::Value> {
    use json_value::Kind;
    if depth > MAX_VALUE_DEPTH {
        return Err(CodecError::InvalidData);
    }
    Ok(match required(value.kind)? {
        Kind::NullValue(_) => serde_json::Value::Null,
        Kind::Boolean(v) => v.into(),
        Kind::Text(v) => v.into(),
        Kind::Integer(v) => v.into(),
        Kind::UnsignedInteger(v) => v.into(),
        Kind::Real(v) => serde_json::Value::Number(
            serde_json::Number::from_f64(v).ok_or(CodecError::InvalidData)?,
        ),
        Kind::Object(v) => {
            serde_json::Value::Object(decode_object(v, depth)?.into_iter().collect())
        }
        Kind::Array(v) => serde_json::Value::Array(
            v.values
                .into_iter()
                .map(|v| decode_json(v, depth + 1))
                .collect::<Result<_>>()?,
        ),
    })
}
fn check_object(value: &Object, depth: usize) -> Result<()> {
    if depth > MAX_VALUE_DEPTH {
        return Err(CodecError::InvalidData);
    }
    for value in value.fields.values() {
        check_json(value, depth + 1)?;
    }
    Ok(())
}
fn check_json(value: &JsonValue, depth: usize) -> Result<()> {
    use json_value::Kind;
    if depth > MAX_VALUE_DEPTH {
        return Err(CodecError::InvalidData);
    }
    match field(&value.kind)? {
        Kind::Real(v) if !v.is_finite() => Err(CodecError::InvalidData),
        Kind::Object(v) => check_object(v, depth),
        Kind::Array(v) => {
            for value in &v.values {
                check_json(value, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn check_resource(value: &ResourceRef) -> Result<()> {
    match ResourceKind::try_from(value.kind) {
        Ok(ResourceKind::Storage | ResourceKind::Reader | ResourceKind::Writer) => Ok(()),
        _ => Err(CodecError::InvalidData),
    }
}
pub fn validate_request(request: &Request) -> Result<()> {
    use Operation as O;
    if request.generation.len() > 128 {
        return Err(CodecError::InvalidData);
    }
    let operation = field(&request.operation)?;
    if !request.data.is_empty()
        && !matches!(
            operation,
            O::WriterChunk(_)
                | O::Storage(StorageCall {
                    operation: Some(StorageOp::SetXattr(_)),
                    ..
                })
        )
    {
        return Err(CodecError::InvalidData);
    }
    match operation {
        O::Initialize(v) => check_object(field(&v.settings)?, 0)?,
        O::Prepare(v) | O::TestConnection(v) => check_object(field(&v.config)?, 0)?,
        O::Migrate(v) => check_object(field(&v.config)?, 0)?,
        O::Open(v) => {
            check_object(field(&v.config)?, 0)?;
        }
        O::Storage(v) => match field(&v.operation)? {
            StorageOp::Stat(request) => {
                super::decode_stat(request.clone()).map_err(|_| CodecError::InvalidData)?;
            }
            StorageOp::ReadLink(request) => {
                super::decode_lookup(request.lookup.clone())
                    .map_err(|_| CodecError::InvalidData)?;
            }
            StorageOp::SetXattr(v)
                if !matches!(
                    XattrWriteMode::try_from(v.mode),
                    Ok(XattrWriteMode::Upsert
                        | XattrWriteMode::CreateNew
                        | XattrWriteMode::Replace)
                ) =>
            {
                return Err(CodecError::InvalidData);
            }
            _ => {}
        },
        O::Release(v) => check_resource(v)?,
        _ => {}
    }
    bound(request.encoded_len(), &request.data)
}
fn check_entry(value: &Entry) -> Result<()> {
    if !matches!(
        EntryKind::try_from(value.kind),
        Ok(EntryKind::File | EntryKind::Dir | EntryKind::Symlink | EntryKind::Other)
    ) || value.modified.is_some_and(|v| v.nanos >= 1_000_000_000)
    {
        return Err(CodecError::InvalidData);
    }
    Ok(())
}

pub fn validate_reply(reply: &Reply) -> Result<()> {
    let value = field(&reply.result)?;
    if !reply.data.is_empty() && !matches!(value, Value::Bytes(_) | Value::Chunk(_)) {
        return Err(CodecError::InvalidData);
    }
    match value {
        Value::Description(v) => {
            for schema in [&v.settings_schema, &v.instance_schema]
                .into_iter()
                .flatten()
            {
                check_object(schema, 0)?;
            }
        }
        Value::Prepared(v) => {
            check_object(field(&v.config)?, 0)?;
            check_object(field(&v.details)?, 0)?;
        }
        Value::Config(v) => check_object(field(&v.config)?, 0)?,
        Value::Storage(v) => {
            field(&v.info)?;
        }
        Value::Entry(v) => check_entry(v)?,
        Value::Page(v) => {
            for value in &v.entries {
                check_entry(value)?;
            }
        }
        Value::Chunk(v) if v.eof != reply.data.is_empty() => return Err(CodecError::InvalidData),
        _ => {}
    }
    bound(reply.encoded_len(), &reply.data)
}
pub fn to_request(generation: &str, operation: &Operation, data: Vec<u8>) -> Result<Request> {
    let request = Request {
        generation: generation.into(),
        operation: Some(operation.clone()),
        data,
    };
    validate_request(&request)?;
    Ok(request)
}
pub fn from_request(request: Request) -> Result<(String, Operation, Vec<u8>)> {
    validate_request(&request)?;
    Ok((
        request.generation,
        required(request.operation)?,
        request.data,
    ))
}
pub fn encode_request(generation: &str, operation: &Operation, data: Vec<u8>) -> Result<Vec<u8>> {
    Ok(to_request(generation, operation, data)?.encode_to_vec())
}
pub fn decode_request(bytes: &[u8]) -> Result<(String, Operation, Vec<u8>)> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CodecError::TooLarge);
    }
    let request = Request::decode(bytes).map_err(CodecError::Decode)?;
    bound(bytes.len(), &request.data)?;
    from_request(request)
}
pub fn to_reply(value: std::result::Result<Reply, Failure>) -> Result<Reply> {
    let reply = value.unwrap_or_else(|error| Reply::new(Value::Failure(error)));
    validate_reply(&reply)?;
    Ok(reply)
}
pub fn from_reply(reply: Reply) -> Result<std::result::Result<Reply, Failure>> {
    validate_reply(&reply)?;
    if let Some(Value::Failure(error)) = reply.result {
        Ok(Err(error))
    } else {
        Ok(Ok(reply))
    }
}
pub fn encode_reply(value: std::result::Result<Reply, Failure>) -> Result<Vec<u8>> {
    Ok(to_reply(value)?.encode_to_vec())
}
pub fn encode_status(code: FailureCode) -> Vec<u8> {
    Reply::new(Value::Failure(Failure::new(code))).encode_to_vec()
}
pub fn decode_reply(bytes: &[u8]) -> Result<std::result::Result<Reply, Failure>> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CodecError::TooLarge);
    }
    let reply = Reply::decode(bytes).map_err(CodecError::Decode)?;
    bound(bytes.len(), &reply.data)?;
    from_reply(reply)
}
pub fn entry_encoded_len(value: &Entry) -> usize {
    value.encoded_len()
}
