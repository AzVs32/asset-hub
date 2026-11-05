use ah_vfs::domain::storage::{EntryPermissions, EntryPermissionsError, XattrName, XattrNameError};

#[test]
fn permissions_accept_exactly_mode_bits_without_entry_type_bits()
-> Result<(), Box<dyn std::error::Error>> {
    for bits in 0..=0o7777 {
        assert_eq!(EntryPermissions::new(bits)?.bits(), bits);
    }
    for bits in [0o10000, 0o100644, 1 << 31, u32::MAX] {
        assert_eq!(
            EntryPermissions::new(bits),
            Err(EntryPermissionsError::InvalidBits)
        );
    }
    Ok(())
}

#[test]
fn xattr_names_enforce_byte_boundaries_without_changing_identity()
-> Result<(), Box<dyn std::error::Error>> {
    for name in [
        "a".to_owned(),
        "a".repeat(255),
        "文".repeat(85),
        format!("{}ab", "文".repeat(84)),
    ] {
        assert_eq!(XattrName::parse(&name)?.as_str(), name);
    }
    for (name, expected) in [
        (String::new(), XattrNameError::Empty),
        ("a".repeat(XattrName::MAX_LEN + 1), XattrNameError::TooLong),
        ("文".repeat(86), XattrNameError::TooLong),
        ("user.\0name".to_owned(), XattrNameError::InvalidCharacter),
    ] {
        assert_eq!(XattrName::parse(&name), Err(expected));
    }
    for name in [
        "user.é",
        "user.e\u{301}",
        "user.名字",
        "USER.Name",
        "user.name",
    ] {
        assert_eq!(XattrName::parse(name)?.as_str(), name);
    }
    assert_ne!(
        XattrName::parse("user.é")?,
        XattrName::parse("user.e\u{301}")?
    );
    assert_ne!(
        XattrName::parse("USER.Name")?,
        XattrName::parse("user.name")?
    );
    Ok(())
}

#[test]
fn storage_value_error_conversions_default_to_request_classifications()
-> Result<(), Box<dyn std::error::Error>> {
    use ah_vfs::domain::storage::{StorageError, StorageErrorKind, StoragePath, StoragePathError};
    use std::error::Error;
    let error = StorageError::from(EntryPermissionsError::InvalidBits);
    assert_eq!(error.kind(), StorageErrorKind::InvalidRequest);
    assert_eq!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<EntryPermissionsError>()),
        Some(&EntryPermissionsError::InvalidBits)
    );
    for cause in [
        XattrNameError::Empty,
        XattrNameError::TooLong,
        XattrNameError::InvalidCharacter,
    ] {
        let error = StorageError::from(cause);
        assert_eq!(error.kind(), StorageErrorKind::InvalidRequest);
        assert_eq!(
            error
                .source()
                .and_then(|e| e.downcast_ref::<XattrNameError>()),
            Some(&cause)
        );
    }
    let error = StorageError::from(
        StoragePath::parse("../invalid")
            .err()
            .ok_or("invalid path accepted")?,
    );
    assert_eq!(error.kind(), StorageErrorKind::InvalidPath);
    assert!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<StoragePathError>())
            .is_some()
    );
    Ok(())
}
