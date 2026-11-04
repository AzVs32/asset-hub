use ah_vfs::domain::{
    DriverInstance, DriverKey, InstanceId, Mount, MountId, VirtualPath, VirtualPathError,
};

#[test]
fn all_path_input_interfaces_apply_the_same_rules() -> Result<(), VirtualPathError> {
    for input in ["/", "////", "/文档/保存", "//文档///保存/"] {
        let expected = VirtualPath::parse(input)?;
        assert_eq!(input.parse::<VirtualPath>()?, expected);
        assert_eq!(VirtualPath::try_from(input.to_owned())?, expected);
    }

    for input in [
        "", "relative", "C:\\data", "/a\\b", "/a\0b", "/./a", "/a/../b",
    ] {
        let expected = VirtualPath::parse(input);
        assert!(expected.is_err(), "accepted {input:?}");
        assert_eq!(input.parse::<VirtualPath>(), expected);
        assert_eq!(VirtualPath::try_from(input.to_owned()), expected);
    }

    let oversized = format!("/{}", "中".repeat(VirtualPath::MAX_LEN));
    assert_eq!(
        VirtualPath::parse(&oversized),
        Err(VirtualPathError::TooLong)
    );
    assert_eq!(
        oversized.parse::<VirtualPath>(),
        Err(VirtualPathError::TooLong)
    );
    assert_eq!(
        VirtualPath::try_from(oversized),
        Err(VirtualPathError::TooLong)
    );
    Ok(())
}

#[test]
fn mount_builder_preserves_the_validated_path_and_instance()
-> Result<(), Box<dyn std::error::Error>> {
    let path = VirtualPath::parse("//文档///保存/")?;
    let instance = DriverInstance::new(DriverKey::parse("acme.s3")?, InstanceId::parse("42")?);
    let mount = Mount::builder()
        .id(MountId::new())
        .virtual_path(path)
        .driver_instance(instance.clone())
        .enabled(false)
        .build();

    assert_eq!(mount.virtual_path().as_str(), "/文档/保存");
    assert!(!mount.enabled());
    assert_eq!(mount.driver_instance(), &instance);
    Ok(())
}

#[test]
fn restored_mount_keeps_its_identity() -> Result<(), Box<dyn std::error::Error>> {
    let original = MountId::new();
    let persisted = original.to_string();
    let restored = MountId::try_from(persisted)?;
    let instance = DriverInstance::new(DriverKey::parse("acme.s3")?, InstanceId::parse("42")?);
    let restored_instance = DriverInstance::new(
        instance.driver_key().to_string().parse()?,
        instance.instance_id().to_string().parse()?,
    );
    let mount = Mount::builder()
        .id(restored)
        .virtual_path(VirtualPath::root())
        .driver_instance(restored_instance)
        .enabled(true)
        .build();
    assert_eq!(mount.id(), original);
    assert_eq!(mount.driver_instance(), &instance);
    for invalid in [
        "documents",
        "00000000-0000-0000-0000-000000000000",
        "67e55044-10b1-426f-9247-bb680e5fe0c8",
    ] {
        assert!(invalid.parse::<MountId>().is_err());
        assert!(MountId::try_from(invalid.to_owned()).is_err());
    }
    Ok(())
}

#[test]
fn instance_reference_identity_includes_driver_and_instance()
-> Result<(), Box<dyn std::error::Error>> {
    let reference = DriverInstance::new(DriverKey::parse("acme.s3")?, InstanceId::parse("42")?);
    assert_eq!(
        reference,
        DriverInstance::new(DriverKey::parse("acme.s3")?, InstanceId::parse("42")?)
    );
    assert_ne!(
        reference,
        DriverInstance::new(DriverKey::parse("acme.memory")?, InstanceId::parse("42")?)
    );
    assert_ne!(
        reference,
        DriverInstance::new(DriverKey::parse("acme.s3")?, InstanceId::parse("43")?)
    );
    Ok(())
}
