use ah_vfs::domain::{InstanceId, InstanceIdError};

#[test]
fn instance_id_inputs_preserve_identity_and_share_validation() -> Result<(), InstanceIdError> {
    for input in [
        "42",
        "0042",
        "01900000-0000-7000-8000-000000000001",
        "opaque:AbC",
        "实例-01",
    ] {
        let id = InstanceId::parse(input)?;
        assert_eq!(id.as_str(), input);
        assert_eq!(input.parse::<InstanceId>()?, id);
        assert_eq!(InstanceId::try_from(input.to_owned())?, id);
    }
    assert_ne!(
        InstanceId::parse("opaque:abc")?,
        InstanceId::parse("opaque:AbC")?
    );
    assert_ne!(InstanceId::parse("42")?, InstanceId::parse("0042")?);

    assert_eq!(InstanceId::parse(""), Err(InstanceIdError::Empty));
    for input in ["", " ", " 42", "42 ", "a\tb", "a\nb", "a\0b", "a\u{2003}b"] {
        let expected = InstanceId::parse(input);
        assert!(expected.is_err(), "accepted {input:?}");
        if !input.is_empty() {
            assert_eq!(expected, Err(InstanceIdError::InvalidCharacter));
        }
        assert_eq!(input.parse::<InstanceId>(), expected);
        assert_eq!(InstanceId::try_from(input.to_owned()), expected);
    }
    Ok(())
}

#[test]
fn generated_ids_are_valid_uuid_v7_values_and_round_trip() -> Result<(), Box<dyn std::error::Error>>
{
    let id: InstanceId = InstanceId::generate();
    let uuid: ah_id::UuidV7 = id.as_str().parse()?;
    assert_eq!(uuid.to_string(), id.as_str());
    assert_eq!(InstanceId::parse(id.as_str())?, id);
    assert_eq!(InstanceId::try_from(id.to_string())?, id);
    assert_ne!(InstanceId::generate(), id);
    Ok(())
}
