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
    assert_eq!(file.package.as_deref(), Some("assethub.vfs.v5"));
    let storage_info = file
        .message_type
        .iter()
        .find(|message| message.name.as_deref() == Some("StorageInfo"))
        .ok_or("StorageInfo")?;
    assert_eq!(storage_info.field.len(), 1);
    assert_eq!(storage_info.field[0].name.as_deref(), Some("chunk_size"));
    assert!(
        storage_info
            .reserved_name
            .iter()
            .any(|name| name == "capabilities")
    );
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
            Some(".assethub.vfs.v5.Request")
        );
        assert_eq!(
            method.output_type.as_deref(),
            Some(".assethub.vfs.v5.Reply")
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

#[test]
fn base_contract_negotiates_without_optional_features_but_requires_the_schema_profile() -> Result {
    let mut description = p::Description {
        major: p::PROTOCOL_MAJOR,
        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
        ..Default::default()
    };
    assert_eq!(
        p::negotiate_protocol(&description).map(|protocol| protocol.features),
        Ok(Vec::<String>::new())
    );
    description.instance_schema_profile = "unknown.schema.v1".into();
    assert_eq!(
        p::negotiate_protocol(&description),
        Err(p::IncompatiblePlugin::SchemaProfile)
    );
    Ok(())
}

#[test]
fn instance_capability_protocol_versions_are_rejected() {
    for major in [1, 2] {
        let description = p::Description {
            major,
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            ..Default::default()
        };
        assert_eq!(
            p::negotiate_protocol(&description),
            Err(p::IncompatiblePlugin::MajorVersion)
        );
    }
}

#[test]
fn optional_features_are_filtered_and_required_features_must_be_supported_and_advertised() -> Result
{
    let mut description = p::Description {
        major: p::PROTOCOL_MAJOR,
        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
        features: vec!["streams.chunked.v1".into(), "unknown.feature.v1".into()],
        required_features: vec!["streams.chunked.v1".into()],
        ..Default::default()
    };
    assert_eq!(
        p::negotiate_protocol(&description).map(|protocol| protocol.features),
        Ok(vec!["streams.chunked.v1".into()])
    );
    description
        .required_features
        .push("unknown.feature.v1".into());
    assert_eq!(
        p::negotiate_protocol(&description),
        Err(p::IncompatiblePlugin::RequiredFeature)
    );
    description.required_features = vec!["streams.chunked.v1".into()];
    description.features.clear();
    assert_eq!(
        p::negotiate_protocol(&description),
        Err(p::IncompatiblePlugin::RequiredFeature)
    );
    Ok(())
}

#[test]
fn host_negotiation_reports_only_missing_capabilities() -> Result {
    let description = p::Description {
        major: p::PROTOCOL_MAJOR,
        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
        required_host_capabilities: vec!["http".into(), "fs.read".into(), "env".into()],
        ..Default::default()
    };
    let error = p::negotiate(&description, &["http"])
        .err()
        .ok_or("missing grants accepted")?;
    assert_eq!(
        error,
        p::IncompatiblePlugin::HostCapabilities(vec!["fs.read".into(), "env".into()])
    );
    assert_eq!(
        error.to_string(),
        "ungranted host capabilities: fs.read, env"
    );
    assert!(p::negotiate(&description, &["http", "fs.read", "env"]).is_ok());
    assert!(p::negotiate_protocol(&description).is_ok());
    Ok(())
}

#[test]
fn malformed_protobuf_retains_the_original_decode_error() -> Result {
    use std::error::Error;
    for error in [
        p::decode_request(&[0xff]).err(),
        p::decode_reply(&[0xff]).err(),
    ] {
        let error = error.ok_or("invalid protobuf accepted")?;
        assert!(matches!(error, p::CodecError::Decode(_)));
        assert!(error.source().is_some());
    }
    Ok(())
}
