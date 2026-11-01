use prost::Message;
use vfs_plugin_protocol::{self as p, Operation as Op, wire};
type Result = std::result::Result<(), Box<dyn std::error::Error>>;
fn config() -> p::Object {
    p::Object::from([
        ("unsigned".into(), u64::MAX.into()),
        ("signed".into(), i64::MIN.into()),
        ("fraction".into(), serde_json::json!(1.25)),
        (
            "nested".into(),
            serde_json::json!({"values": [null, true, "文本"]}),
        ),
    ])
}

#[test]
fn generated_bindings_preserve_integer_precision_and_field_presence() -> Result {
    let encoded = p::to_request(
        "g",
        &Op::Prepare(p::Config {
            config: Some(config()),
        }),
        vec![],
    )?;
    let Some(wire::request::Operation::Prepare(value)) = encoded.operation else {
        return Err("prepare".into());
    };
    let value = value.config.ok_or("config")?;
    assert_eq!(
        value.fields["unsigned"].kind,
        Some(wire::json_value::Kind::UnsignedInteger(u64::MAX))
    );
    assert_eq!(
        value.fields["signed"].kind,
        Some(wire::json_value::Kind::Integer(i64::MIN))
    );
    let config = p::json_object(value)?;
    assert_eq!(config["unsigned"].as_u64(), Some(u64::MAX));
    assert_eq!(config["signed"].as_i64(), Some(i64::MIN));
    assert_eq!(config["fraction"].as_f64(), Some(1.25));
    assert_eq!(
        config["nested"],
        serde_json::json!({"values": [null, true, "文本"]})
    );
    let independent = wire::Request {
        generation: "g".into(),
        data: vec![],
        operation: Some(wire::request::Operation::Read(wire::Read {
            handle: 1,
            path: "file".into(),
            offset: u64::MAX,
            length: Some(0),
        })),
    };
    let (_, operation, _) = p::decode_request(&independent.encode_to_vec())?;
    assert_eq!(
        operation,
        Op::Read(p::Read {
            handle: 1,
            path: "file".into(),
            offset: u64::MAX,
            length: Some(0)
        })
    );
    // Encoded independently by protoc --encode from the schema's text format.
    let protoc = [
        10, 1, 103, 82, 21, 8, 1, 18, 4, 102, 105, 108, 101, 24, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 1, 32, 0,
    ];
    assert_eq!(independent.encode_to_vec(), protoc);
    Ok(())
}
#[test]
fn missing_alternatives_unknown_enums_and_invalid_numbers_are_rejected() -> Result {
    assert!(p::decode_request(&[]).is_err());
    assert!(p::decode_reply(&[]).is_err());
    assert!(p::decode_request(br#"{"operation":"describe"}"#).is_err());
    assert!(p::decode_request(&[0xfa, 0x7f, 0]).is_err());
    assert!(
        p::from_request(wire::Request {
            operation: Some(wire::request::Operation::Initialize(wire::Initialize {
                settings: None
            })),
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        p::from_reply(wire::Reply {
            result: Some(wire::reply::Result::Storage(wire::Storage {
                handle: 1,
                info: None
            })),
            ..Default::default()
        })
        .is_err()
    );
    for kind in [0, i32::MAX] {
        assert!(
            p::from_request(wire::Request {
                generation: "g".into(),
                data: vec![],
                operation: Some(wire::request::Operation::Release(wire::ResourceRef {
                    kind,
                    handle: 1
                }))
            })
            .is_err()
        );
        assert!(
            p::from_reply(wire::Reply {
                data: vec![],
                result: Some(wire::reply::Result::Entry(wire::Entry {
                    kind,
                    ..Default::default()
                }))
            })
            .is_err()
        );
    }
    for value in [
        wire::JsonValue { kind: None },
        wire::JsonValue {
            kind: Some(wire::json_value::Kind::Real(f64::NAN)),
        },
    ] {
        let request = wire::Request {
            generation: "g".into(),
            data: vec![],
            operation: Some(wire::request::Operation::Initialize(wire::Initialize {
                settings: Some(wire::Object {
                    fields: std::collections::BTreeMap::from([("bad".into(), value)]),
                }),
            })),
        };
        assert!(p::from_request(request).is_err());
    }
    let unknown = wire::Reply {
        data: vec![],
        result: Some(wire::reply::Result::Failure(wire::Failure {
            code: i32::MAX,
            ..Default::default()
        })),
    };
    assert_eq!(
        p::from_reply(unknown)?.err().ok_or("failure")?.code,
        i32::MAX
    );
    Ok(())
}
#[test]
fn bounds_apply_to_control_data_and_nested_configuration() {
    let oversized = p::Object::from([("text".into(), "x".repeat(p::MAX_CONTROL_BYTES).into())]);
    assert_eq!(
        p::to_request(
            "g",
            &Op::Prepare(p::Config {
                config: Some(oversized)
            }),
            vec![]
        ),
        Err(p::CodecError::TooLarge)
    );
    assert_eq!(
        p::to_request(
            "g",
            &Op::WriterChunk(p::Handle { handle: 1 }),
            vec![0; p::MAX_DATA_BYTES + 1]
        ),
        Err(p::CodecError::TooLarge)
    );
    assert_eq!(
        p::to_request("g", &Op::Describe(p::Empty {}), vec![1]),
        Err(p::CodecError::InvalidData)
    );
    assert!(p::decode_request(&vec![0; p::MAX_FRAME_BYTES + 1]).is_err());
    let mut nested = serde_json::Value::Null;
    for _ in 0..30 {
        nested = serde_json::json!({"child":nested});
    }
    assert!(
        p::to_request(
            "g",
            &Op::Prepare(p::Config {
                config: Some(p::Object::from([("nested".into(), nested)]))
            }),
            vec![]
        )
        .is_err()
    );
}
#[test]
fn exported_descriptor_contains_the_complete_language_independent_contract() -> Result {
    let descriptor = prost_types::FileDescriptorSet::decode(p::FILE_DESCRIPTOR_SET)?;
    let file = descriptor.file.first().ok_or("schema")?;
    assert_eq!(file.package.as_deref(), Some("assethub.vfs.v2"));
    let request = file
        .message_type
        .iter()
        .find(|m| m.name.as_deref() == Some("Request"))
        .ok_or("Request")?;
    assert_eq!(request.oneof_decl.len(), 1);
    assert_eq!(request.field.len(), 16);
    assert!(
        request
            .field
            .iter()
            .all(|f| f.name.as_deref() != Some("control"))
    );
    let reply = file
        .message_type
        .iter()
        .find(|m| m.name.as_deref() == Some("Reply"))
        .ok_or("Reply")?;
    assert_eq!(reply.oneof_decl.len(), 1);
    for field in ["description", "storage", "entry", "failure"] {
        assert!(reply.field.iter().any(|f| f.name.as_deref() == Some(field)));
    }
    for method in &file.service.first().ok_or("Plugin")?.method {
        assert_eq!(
            method.input_type.as_deref(),
            Some(".assethub.vfs.v2.Request")
        );
        assert_eq!(
            method.output_type.as_deref(),
            Some(".assethub.vfs.v2.Reply")
        );
    }
    Ok(())
}

#[test]
fn open_requires_an_explicit_configuration_object() -> Result {
    for config in [None, Some(p::Object::new())] {
        let request = p::Request {
            generation: "g".into(),
            data: vec![],
            operation: Some(Op::Open(p::Open {
                id: "one".into(),
                config: config.clone(),
            })),
        };
        assert_eq!(p::from_request(request).is_ok(), config.is_some());
    }
    Ok(())
}
