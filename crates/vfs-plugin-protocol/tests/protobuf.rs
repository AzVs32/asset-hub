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
            lookup: Some(p::Lookup {
                operand: "/file".into(),
                intermediate_links: p::IntermediateLink::FollowIntermediate.into(),
                ..Default::default()
            }),
            offset: u64::MAX,
            length: Some(0),
        })),
    };
    let (_, operation, _) = p::decode_request(&independent.encode_to_vec())?;
    assert_eq!(
        operation,
        Op::Read(p::encode_read(
            1,
            &ah_vfs_storage::ReadRequest::new(
                ah_vfs_storage::PathExpr::parse("/file")?,
                ah_vfs_storage::ReadRange::new(u64::MAX, Some(0))?
            ),
            &ah_vfs_storage::StoragePath::root(),
            ah_vfs_storage::IntermediateSymlinks::FollowWithinInstance
        ))
    );
    // Encoded independently by protoc --encode from the schema's text format.
    let protoc = [
        10, 1, 103, 82, 26, 8, 1, 18, 9, 10, 5, 47, 102, 105, 108, 101, 24, 1, 24, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 1, 32, 0,
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
    assert_eq!(file.package.as_deref(), Some("assethub.vfs.v13"));
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
    assert_eq!(request.field.len(), 20);
    for name in [
        "begin_lookup",
        "observe_child",
        "read_observed_link",
        "verify_lookup",
    ] {
        assert!(
            request
                .field
                .iter()
                .any(|field| field.name.as_deref() == Some(name))
        );
    }
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
    for field in [
        "description",
        "storage",
        "entry",
        "failure",
        "opened_lookup",
        "observed_entry",
    ] {
        assert!(reply.field.iter().any(|f| f.name.as_deref() == Some(field)));
    }
    for method in &file.service.first().ok_or("Plugin")?.method {
        assert_eq!(
            method.input_type.as_deref(),
            Some(".assethub.vfs.v13.Request")
        );
        assert_eq!(
            method.output_type.as_deref(),
            Some(".assethub.vfs.v13.Reply")
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

#[test]
fn read_rejects_an_overflowing_range_before_dispatch() -> Result {
    let request = wire::Request {
        operation: Some(Op::Read(wire::Read {
            handle: 1,
            lookup: Some(p::Lookup {
                operand: "../file".into(),
                working_directory: "docs".into(),
                intermediate_links: p::IntermediateLink::RejectIntermediate.into(),
            }),
            offset: u64::MAX,
            length: Some(1),
        })),
        ..Default::default()
    };
    assert!(matches!(
        p::decode_request(&request.encode_to_vec()),
        Err(p::CodecError::InvalidData)
    ));
    Ok(())
}

#[test]
fn read_preserves_absent_and_explicit_lengths() -> Result {
    use ah_vfs_storage as d;

    let path = d::PathExpr::parse("../file/")?;
    let base = d::StoragePath::parse("docs")?;
    let links = d::IntermediateSymlinks::Reject;
    for length in [None, Some(0), Some(42)] {
        let request = d::ReadRequest::new(path.clone(), d::ReadRange::new(3, length)?);
        let operation = Op::Read(p::encode_read(1, &request, &base, links));
        let bytes = p::encode_request("g", &operation, vec![])?;
        let (_, Op::Read(read), _) = p::decode_request(&bytes)? else {
            return Err("missing read request".into());
        };
        assert_eq!(p::decode_read(read)?, (request, base.clone(), links));
    }
    Ok(())
}

#[test]
fn write_round_trips_binding_and_rejects_implicit_modes() -> Result {
    use ah_vfs_storage as d;
    let request = d::WriteRequest::new(
        d::PathExpr::parse("../out")?,
        d::WriteMode::Replace,
        d::Publication::Atomic,
    );
    let base = d::StoragePath::parse("work/sub")?;
    let links = d::IntermediateSymlinks::Reject;
    let wire = p::encode_write(7, &request, &base, links);
    assert_eq!(wire.handle, 7);
    assert_eq!(p::decode_write(wire.clone())?, (request, base, links));
    for invalid in [
        p::Write {
            mode: 0,
            ..wire.clone()
        },
        p::Write {
            publication: i32::MAX,
            ..wire.clone()
        },
    ] {
        let operation = Op::Write(invalid);
        assert!(matches!(
            p::encode_request("generation", &operation, vec![]),
            Err(p::CodecError::InvalidData)
        ));
    }
    Ok(())
}

#[test]
fn create_directory_preserves_ordered_lookup_and_rejects_invalid_modes() -> Result {
    use ah_vfs_storage as d;
    let request = d::CreateDirectoryRequest::new(
        d::PathExpr::parse("../missing/")?,
        d::CreateDirectoryMode::Recursive,
    );
    let base = d::StoragePath::parse("work/sub")?;
    let links = d::IntermediateSymlinks::Reject;
    let wire = p::encode_create_directory(&request, &base, links);
    assert_eq!(
        p::decode_create_directory(wire.clone())?,
        (request, base, links)
    );
    for mode in [0, i32::MAX] {
        let op = Op::Storage(p::StorageCall {
            handle: 1,
            operation: Some(p::StorageOp::CreateDirectory(p::CreateDirectory {
                mode,
                ..wire.clone()
            })),
        });
        assert!(matches!(
            p::encode_request("generation", &op, vec![]),
            Err(p::CodecError::InvalidData)
        ));
    }
    Ok(())
}

#[test]
fn mutations_round_trip_shared_bindings_and_reject_non_entry_operands() -> Result {
    use ah_vfs_storage as d;
    let base = d::StoragePath::parse("work/sub")?;
    let links = d::IntermediateSymlinks::Reject;
    let remove = d::RemoveRequest::new(
        d::PathExpr::parse("../directory/")?,
        d::RemoveKind::EmptyDirectory,
    )?;
    let wire = p::encode_remove(&remove, &base, links);
    assert_eq!(
        p::decode_remove(wire.clone())?,
        (remove, base.clone(), links)
    );
    assert!(p::decode_remove(p::Remove { kind: 0, ..wire }).is_err());
    let rename = d::RenameRequest::new(
        d::PathExpr::parse("../source/")?,
        d::PathExpr::parse("/target/")?,
        d::RenameMode::Replace,
    )?;
    let wire = p::encode_rename(&rename, &base, links);
    assert_eq!(p::decode_rename(wire.clone())?, (rename, base, links));
    for invalid in [
        p::Rename {
            mode: 0,
            ..wire.clone()
        },
        p::Rename {
            destination: "/".into(),
            ..wire
        },
    ] {
        assert!(matches!(
            p::encode_request(
                "generation",
                &Op::Storage(p::StorageCall {
                    handle: 1,
                    operation: Some(p::StorageOp::Rename(invalid)),
                }),
                vec![]
            ),
            Err(p::CodecError::InvalidData)
        ));
    }
    Ok(())
}

#[test]
fn attribute_requests_preserve_lookup_and_move_binary_values_without_copying() -> Result {
    use ah_vfs_storage as d;
    use d::AttributeAction as A;
    use std::num::NonZeroUsize;

    let base = d::StoragePath::parse("work/sub")?;
    let links = d::IntermediateSymlinks::Reject;
    let path = d::PathExpr::parse("../entry/")?;
    let name = d::XattrName::parse("user.key")?;
    let max = NonZeroUsize::new(1024).ok_or("limit")?;
    for action in [
        A::GetPermissions,
        A::SetPermissions(d::Permissions::new(0o754)?),
        A::list_xattrs(max),
        A::get_xattr(name.clone(), max),
        A::set_xattr(name.clone(), vec![], d::XattrWriteMode::Upsert),
        A::set_xattr(name.clone(), vec![0, 255, 0], d::XattrWriteMode::Replace),
        A::RemoveXattr(name.clone()),
    ] {
        let original = d::AttributeRequest::new(path.clone(), action);
        let copy = original.clone();
        let pointer = match original.action() {
            A::SetXattr { value, .. } => Some(value.as_ptr()),
            _ => None,
        };
        let (wire, data) = p::encode_attribute(original, &base, links)?;
        if let Some(pointer) = pointer {
            assert_eq!(data.as_ptr(), pointer);
        }
        let (decoded, decoded_base, decoded_links) = p::decode_attribute(wire, data)?;
        if let A::SetXattr { value, .. } = decoded.action() {
            assert_eq!(Some(value.as_ptr()), pointer);
        }
        assert_eq!(
            (decoded, decoded_base, decoded_links),
            (copy, base.clone(), links)
        );
    }

    let lookup = Some(p::encode_lookup(&path, &base, links));
    for wire in [
        p::StorageOp::SetPermissions(p::SetPermissions {
            lookup: lookup.clone(),
            mode: 0o100644,
        }),
        p::StorageOp::GetXattr(p::GetXattr {
            lookup: lookup.clone(),
            name: "user.key".into(),
            max_bytes: 0,
        }),
        p::StorageOp::SetXattr(p::SetXattr {
            lookup: lookup.clone(),
            name: "user.key".into(),
            mode: 0,
        }),
        p::StorageOp::RemoveXattr(p::RemoveXattr {
            lookup: lookup.clone(),
            name: "user.\0bad".into(),
        }),
        p::StorageOp::ListXattrs(p::ListXattrs {
            lookup,
            max_bytes: (p::MAX_ATTRIBUTE_NAMES_BYTES + 1).try_into()?,
        }),
    ] {
        assert!(matches!(
            p::encode_request(
                "generation",
                &Op::Storage(p::StorageCall {
                    handle: 1,
                    operation: Some(wire),
                }),
                vec![]
            ),
            Err(p::CodecError::InvalidData)
        ));
    }
    let request = d::AttributeRequest::new(
        path.clone(),
        A::set_xattr(
            name,
            vec![0; p::MAX_DATA_BYTES + 1],
            d::XattrWriteMode::CreateNew,
        ),
    );
    assert!(matches!(
        p::encode_attribute(request, &base, links),
        Err(p::AttributeValueError::Limit)
    ));
    let request = d::AttributeRequest::new(path, A::list_xattrs(NonZeroUsize::MAX));
    let (wire, data) = p::encode_attribute(request, &base, links)?;
    let (bounded, _, _) = p::decode_attribute(wire, data)?;
    assert!(matches!(bounded.action(), A::ListXattrs { max_bytes, .. }
        if max_bytes.get() == p::MAX_ATTRIBUTE_NAMES_BYTES));
    Ok(())
}

#[test]
fn attribute_replies_apply_limits_before_decoding_and_preserve_empty_values() -> Result {
    use ah_vfs_storage::{AttributeExpectation as E, AttributeResult as R, XattrList, XattrName};
    use std::num::NonZeroUsize;
    let limit = NonZeroUsize::new(20).ok_or("limit")?;
    let names = XattrList::new(
        vec![XattrName::parse("user.a")?, XattrName::parse("user.é")?],
        limit,
    )?;
    let reply = p::encode_attribute_result(E::Names(limit), R::Names(names.clone()))?;
    assert_eq!(
        p::decode_attribute_result(E::Names(limit), reply)?,
        R::Names(names)
    );
    let empty = XattrList::new(vec![], limit)?;
    let reply = p::encode_attribute_result(E::Names(limit), R::Names(empty.clone()))?;
    assert_eq!(
        p::decode_attribute_result(E::Names(limit), reply)?,
        R::Names(empty)
    );
    let reply = p::encode_attribute_result(E::Value(limit), R::Value(vec![]))?;
    assert_eq!(
        p::decode_attribute_result(E::Value(limit), reply)?,
        R::Value(vec![])
    );
    for payload in [
        b"user.a\0user.a\0".as_slice(),
        b"user.b\0user.a\0",
        b"\xff\0",
        b"a",
        b"\0",
        &[b'a'; 21],
    ] {
        let reply = p::Reply {
            result: Some(p::Value::Names(p::Empty {})),
            data: payload.to_vec(),
        };
        assert!(p::decode_attribute_result(E::Names(limit), reply).is_err());
    }
    let reply = p::Reply {
        result: Some(p::Value::Bytes(p::Empty {})),
        data: vec![7; 21],
    };
    assert!(p::decode_attribute_result(E::Value(limit), reply).is_err());
    let reply = p::Reply {
        result: Some(p::Value::Names(p::Empty {})),
        data: vec![0; p::MAX_ATTRIBUTE_NAMES_BYTES + 1],
    };
    assert!(matches!(
        p::validate_reply(&reply),
        Err(p::CodecError::TooLarge)
    ));
    assert!(p::decode_attribute_result(E::Names(NonZeroUsize::MAX), reply).is_err());
    Ok(())
}

#[test]
fn listing_validates_limits_and_cursor_before_enumeration() -> Result {
    use ah_vfs_storage as d;
    let request = d::ListRequest::new(
        d::ListTarget::Path(d::StoragePath::root().to_rooted_expr()),
        d::PageSize::new(1)?,
    );
    let selection = d::DirectorySelection::from_lookup(
        d::StoragePath::root(),
        d::DirectoryIdentity::from_bytes([1; 16]),
    );
    let bound = request.bind_selection(selection)?;
    let list = p::encode_list(&bound);
    for invalid in [
        p::List {
            size: 0,
            ..list.clone()
        },
        p::List {
            size: d::PageSize::MAX as u32 + 1,
            ..list.clone()
        },
        p::List {
            after: Some("../bad".into()),
            ..list.clone()
        },
        p::List {
            selection: None,
            ..list.clone()
        },
    ] {
        let op = Op::Storage(p::StorageCall {
            handle: 1,
            operation: Some(p::StorageOp::List(invalid)),
        });
        assert!(matches!(
            p::encode_request("generation", &op, vec![]),
            Err(p::CodecError::InvalidData)
        ));
    }
    let page = p::Page {
        reported: list.selection,
        completion: Some(p::page::Completion::After("a".into())),
    };
    assert_eq!(
        p::decode_page(&bound, page, &[]),
        Err(d::ContractViolation::InvalidPage)
    );
    Ok(())
}

#[test]
fn entry_timestamps_preserve_absence_epoch_and_signed_subseconds() -> Result {
    use ah_vfs_storage as d;
    use std::time::{Duration, UNIX_EPOCH};

    for modified in [
        None,
        Some(p::Timestamp {
            seconds: 0,
            nanos: 0,
        }),
        Some(p::Timestamp {
            seconds: -1,
            nanos: 500_000_000,
        }),
        Some(p::Timestamp {
            seconds: 42,
            nanos: 123_456_700,
        }),
    ] {
        let wire = p::Entry {
            path: "file".into(),
            kind: p::EntryKind::File.into(),
            size: Some(0),
            modified,
        };
        let reply = p::Reply {
            result: Some(p::Value::Entry(wire.clone())),
            data: vec![],
        };
        let bytes = p::encode_reply(Ok(reply))?;
        let Some(p::Value::Entry(received)) = p::decode_reply(&bytes)??.result else {
            return Err("missing entry reply".into());
        };
        let entry = p::decode_entry(received)?;
        assert_eq!(entry.entry_type(), d::EntryType::file(Some(0)));
        assert_eq!(p::encode_entry(&entry)?, wire);
    }

    assert!(matches!(
        p::decode_time(0, 1_000_000_000),
        Err(p::EntryValueError::Timestamp)
    ));
    for ((seconds, nanos), host_time) in [
        (
            (i64::MIN, 0),
            UNIX_EPOCH.checked_sub(Duration::from_secs(1 << 63)),
        ),
        (
            (i64::MAX, 0),
            UNIX_EPOCH.checked_add(Duration::from_secs((1 << 63) - 1)),
        ),
        ((0, 1), UNIX_EPOCH.checked_add(Duration::from_nanos(1))),
    ] {
        let expected = host_time
            .filter(|time| matches!(p::encode_time(*time), Ok(wire) if wire == (seconds, nanos)));
        match expected {
            Some(time) => assert_eq!(p::decode_time(seconds, nanos)?, time),
            None => assert!(matches!(
                p::decode_time(seconds, nanos),
                Err(p::EntryValueError::Timestamp)
            )),
        }
    }
    if let Some(time) = UNIX_EPOCH.checked_add(Duration::from_secs(1 << 63)) {
        assert!(matches!(
            p::encode_time(time),
            Err(p::EntryValueError::Timestamp)
        ));
    }
    Ok(())
}

#[test]
fn page_replies_require_the_selected_directory_and_explicit_completion() -> Result {
    use ah_vfs_storage as d;
    use p::page::Completion;

    let request = d::ListRequest::new(
        d::ListTarget::Path(d::PathExpr::parse("/alias/")?),
        d::PageSize::new(1)?,
    );
    let bound = request.bind_selection(d::DirectorySelection::from_lookup(
        d::StoragePath::parse("docs")?,
        d::DirectoryIdentity::from_bytes([7; 16]),
    ))?;
    let entry = d::Entry::new(
        d::StoragePath::parse("docs/a")?,
        d::EntryType::Directory,
        None,
    )?;
    let original = d::DirectoryPage::new(&bound, vec![entry], true)?;
    let bytes = p::encode_reply(Ok(p::encode_page(original.clone())?))?;
    let reply = p::decode_reply(&bytes)??;
    let Some(p::Value::Page(page)) = reply.result else {
        return Err("missing page reply".into());
    };
    assert_eq!(p::decode_page(&bound, page.clone(), &reply.data)?, original);

    let empty = p::Page {
        completion: Some(Completion::End(p::Empty {})),
        ..page.clone()
    };
    assert!(
        p::decode_page(&bound, empty.clone(), &[])?
            .entries()
            .is_empty()
    );
    let invalid = Err(d::ContractViolation::InvalidPage);
    for (address, identity) in [
        ("alias", vec![7; 16]),
        ("docs", vec![8; 16]),
        ("docs", vec![7; 15]),
    ] {
        let forged = p::Page {
            reported: Some(p::DirectorySelection {
                address: address.into(),
                identity,
            }),
            ..empty.clone()
        };
        assert_eq!(p::decode_page(&bound, forged, &[]), invalid);
    }
    for completion in [None, Some(Completion::After("b".into()))] {
        let malformed = p::Page {
            completion,
            ..page.clone()
        };
        assert_eq!(p::decode_page(&bound, malformed, &reply.data), invalid);
    }
    assert_eq!(
        p::decode_page(&bound, page.clone(), &reply.data.repeat(2)),
        invalid
    );
    let unrelated = p::Entry {
        path: "elsewhere/a".into(),
        kind: p::EntryKind::Dir.into(),
        ..Default::default()
    };
    let mut data = Vec::new();
    unrelated.encode_length_delimited(&mut data)?;
    assert_eq!(p::decode_page(&bound, page, &data), invalid);
    Ok(())
}
