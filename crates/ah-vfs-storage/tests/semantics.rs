use ah_vfs_storage::*;
use std::error::Error;
type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
const READS: [Operation; 7] = [
    Operation::Stat,
    Operation::List,
    Operation::Read,
    Operation::ReadLink,
    Operation::GetPermissions,
    Operation::ListXattrs,
    Operation::GetXattr,
];
const MUTATIONS: [Operation; 7] = [
    Operation::Write,
    Operation::CreateDirectory,
    Operation::Remove,
    Operation::Rename,
    Operation::SetPermissions,
    Operation::SetXattr,
    Operation::RemoveXattr,
];

#[test]
fn outcome_is_independent_of_failure_reason_and_conservative_after_dispatch() {
    for failure in [
        Failure::Rejected(Rejection::Conflict),
        Failure::Backend(BackendFailure::TimedOut),
        Failure::Backend(BackendFailure::Cancelled),
        Failure::Backend(BackendFailure::CorruptData),
        Failure::Contract(ContractViolation::InvalidResponse),
    ] {
        for operation in MUTATIONS {
            let error = StorageError::after_dispatch(operation, failure, None);
            assert_eq!(error.failure(), failure);
            assert_eq!(error.operation(), operation);
            assert_eq!(error.outcome(), MutationOutcome::Unknown);
            assert_eq!(
                StorageError::without_effects(operation, failure, None).outcome(),
                MutationOutcome::Unchanged
            );
        }
        for operation in READS {
            assert_eq!(
                StorageError::after_dispatch(operation, failure, None).outcome(),
                MutationOutcome::Unchanged
            );
        }
    }
}
#[test]
fn diagnostics_are_typed_private_and_do_not_change_failure_or_outcome() -> Result {
    fn thread_safe<T: Error + Send + Sync + 'static>() {}
    thread_safe::<StorageError>();
    let cause = StoragePath::parse("private-location/..")
        .err()
        .ok_or("invalid address")?;
    let failure = Failure::Contract(ContractViolation::InvalidEntry);
    let error = StorageError::after_dispatch(Operation::Write, failure, Some(Box::new(cause)));
    assert_eq!(error.failure(), failure);
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<PathError>())
            .is_some()
    );
    let error = StorageError::after_dispatch(Operation::Write, failure, Some(Box::new(error)));
    let nested = error
        .source()
        .and_then(|e| e.downcast_ref::<StorageError>())
        .ok_or("nested cause")?;
    assert!(
        nested
            .source()
            .and_then(|e| e.downcast_ref::<PathError>())
            .is_some()
    );
    let error = StorageError::after_dispatch(
        Operation::Write,
        failure,
        Some(Box::new(std::io::Error::other("token=private-secret"))),
    );
    let same_public_fields = StorageError::after_dispatch(Operation::Write, failure, None);
    assert_eq!(error.to_string(), same_public_fields.to_string());
    assert_eq!(format!("{error:?}"), format!("{same_public_fields:?}"));
    assert!(
        error
            .source()
            .ok_or("source")?
            .to_string()
            .contains("private-secret")
    );
    Ok(())
}
#[test]
fn malformed_responses_and_stored_corruption_are_distinct_failure_families() -> Result {
    let request = ListRequest::new(StoragePath::root(), PageSize::DEFAULT);
    let bound = request.bind_selection(DirectorySelection::from_lookup(
        StoragePath::root(),
        DirectoryIdentity::from_bytes([1; 16]),
    ))?;
    let invalid_page = DirectoryPage::new(&bound, vec![], true)
        .err()
        .ok_or("malformed page")?;
    let protocol = StorageError::after_dispatch(
        Operation::List,
        Failure::Contract(ContractViolation::InvalidPage),
        Some(Box::new(invalid_page)),
    );
    assert_eq!(
        protocol.failure(),
        Failure::Contract(ContractViolation::InvalidPage)
    );
    assert_eq!(protocol.outcome(), MutationOutcome::Unchanged);
    assert_eq!(
        protocol
            .source()
            .and_then(|e| e.downcast_ref::<DirectoryPageError>()),
        Some(&DirectoryPageError::EmptyContinuation)
    );
    let corrupt = StorageError::after_dispatch(
        Operation::Read,
        Failure::Backend(BackendFailure::CorruptData),
        None,
    );
    assert_ne!(protocol.failure(), corrupt.failure());
    Ok(())
}
#[test]
fn capability_defaults_disable_every_optional_operation() {
    let capabilities = Capabilities::default();
    for operation in READS.into_iter().chain(MUTATIONS) {
        assert!(!capabilities.supports(operation));
    }
}
#[test]
fn declarations_remain_infallible_and_immutable_when_composed() {
    let base = Capabilities::default();
    let extended = Capabilities::default()
        .with_stat(true)
        .with_list(true)
        .with_permissions(AttributeSupport::ReadWrite)
        .with_xattrs(AttributeSupport::Read);
    assert!(!base.supports(Operation::Stat));
    for operation in [
        Operation::Stat,
        Operation::List,
        Operation::GetPermissions,
        Operation::SetPermissions,
        Operation::GetXattr,
        Operation::ListXattrs,
    ] {
        assert!(extended.supports(operation));
    }
    assert!(!extended.supports(Operation::SetXattr));
    assert!(!extended.supports(Operation::Remove));
}
#[test]
fn write_admission_checks_modes_and_atomicity_before_side_effects() -> Result {
    for modes in [WriteModes::CreateNew, WriteModes::Replace, WriteModes::Both] {
        for atomic_publish in [false, true] {
            let capabilities =
                Capabilities::default().with_write(WriteSupport::staged(modes, atomic_publish));
            for mode in [WriteMode::CreateNew, WriteMode::Replace] {
                for publication in [Publication::Staged, Publication::Atomic] {
                    let request =
                        WriteRequest::new(StoragePath::parse("file")?, mode, publication)?;
                    let mode_supported = modes == WriteModes::Both
                        || matches!(
                            (modes, mode),
                            (WriteModes::CreateNew, WriteMode::CreateNew)
                                | (WriteModes::Replace, WriteMode::Replace)
                        );
                    let expected =
                        mode_supported && (publication == Publication::Staged || atomic_publish);
                    assert_eq!(capabilities.require_write(&request).is_ok(), expected);
                }
            }
        }
    }
    let capabilities = Capabilities::default();
    let request = WriteRequest::new(
        StoragePath::parse("file")?,
        WriteMode::CreateNew,
        Publication::Staged,
    )?;
    let cause = capabilities
        .require_write(&request)
        .err()
        .ok_or("unsupported write admitted")?;
    let failure = StorageError::without_effects(Operation::Write, Failure::Rejected(cause), None);
    assert_eq!(failure.failure(), Failure::Rejected(Rejection::Unsupported));
    assert_eq!(failure.outcome(), MutationOutcome::Unchanged);
    Ok(())
}
#[test]
fn read_admission_distinguishes_sequential_prefixes_from_arbitrary_ranges() -> Result {
    let prefix = ReadRequest::new(StoragePath::parse("file")?, ReadRange::new(0, Some(3))?);
    let range = ReadRequest::new(StoragePath::parse("file")?, ReadRange::new(1, Some(3))?);
    let sequential = Capabilities::default().with_read(ReadSupport::Sequential);
    assert!(sequential.require_read(&prefix).is_ok());
    assert_eq!(sequential.require_read(&range), Err(Rejection::Unsupported));
    assert!(
        Capabilities::default()
            .with_read(ReadSupport::Range)
            .require_read(&range)
            .is_ok()
    );
    assert_eq!(
        Capabilities::default().require_read(&prefix),
        Err(Rejection::Unsupported)
    );
    Ok(())
}
#[test]
fn directory_and_attribute_guarantees_carry_their_prerequisites() {
    assert!(!DirectorySupport::None.can_create());
    assert!(!DirectorySupport::None.persistent_empty());
    for create in [false, true] {
        for persistent_empty in [false, true] {
            for support in [
                DirectorySupport::hierarchical(create, persistent_empty),
                DirectorySupport::prefix(create, persistent_empty),
            ] {
                let capabilities = Capabilities::default().with_directories(support);
                assert_eq!(capabilities.supports(Operation::CreateDirectory), create);
                assert_eq!(
                    capabilities.directories().persistent_empty(),
                    persistent_empty
                );
            }
        }
    }
    for support in [
        AttributeSupport::None,
        AttributeSupport::Read,
        AttributeSupport::ReadWrite,
    ] {
        let capabilities = Capabilities::default()
            .with_xattrs(support)
            .with_permissions(support);
        assert!(!support.can_write() || support.can_read());
        assert_eq!(
            capabilities.supports(Operation::GetPermissions),
            support.can_read()
        );
        assert_eq!(
            capabilities.supports(Operation::SetXattr),
            support.can_write()
        );
    }
}
#[test]
fn rename_admission_preserves_the_requested_replacement_guarantee() -> Result {
    for (support, no_replace, replace) in [
        (RenameSupport::None, false, false),
        (RenameSupport::NoReplace, true, false),
        (RenameSupport::Replace, false, true),
        (RenameSupport::Both, true, true),
    ] {
        let capabilities = Capabilities::default().with_rename(support);
        for (mode, supported) in [
            (RenameMode::NoReplace, no_replace),
            (RenameMode::Replace, replace),
        ] {
            let request = RenameRequest::new(
                StoragePath::parse("file")?,
                StoragePath::parse("destination")?,
                mode,
            )?;
            assert_eq!(capabilities.require_rename(&request).is_ok(), supported);
        }
    }
    Ok(())
}

#[test]
fn extensible_payloads_are_built_and_matched_through_the_public_contract() -> Result {
    let EntryType::File { size, .. } = EntryType::file(Some(42)) else {
        return Err("expected a file".into());
    };
    assert_eq!(size, Some(42));
    let WriteSupport::Staged {
        modes,
        atomic_publish,
        ..
    } = WriteSupport::staged(WriteModes::Both, true)
    else {
        return Err("expected staged write support".into());
    };
    assert_eq!(modes, WriteModes::Both);
    assert!(atomic_publish);
    for support in [
        DirectorySupport::hierarchical(true, false),
        DirectorySupport::prefix(true, false),
    ] {
        match support {
            DirectorySupport::Hierarchical {
                create,
                persistent_empty,
                ..
            }
            | DirectorySupport::Prefix {
                create,
                persistent_empty,
                ..
            } => {
                assert!(create);
                assert!(!persistent_empty);
            }
            _ => return Err("expected directory support".into()),
        }
    }
    Ok(())
}

#[test]
fn attribute_requests_admit_only_their_own_capability_family() -> Result {
    use std::num::NonZeroUsize;

    let none = Capabilities::default();
    let declarations = [
        none,
        none.with_permissions(AttributeSupport::Read),
        none.with_permissions(AttributeSupport::ReadWrite),
        none.with_xattrs(AttributeSupport::Read),
        none.with_xattrs(AttributeSupport::ReadWrite),
    ];
    let name = XattrName::parse("user.note")?;
    let path = StoragePath::parse("work/link")?;
    for (action, expected, admitted) in [
        (
            AttributeAction::GetPermissions,
            Operation::GetPermissions,
            [false, true, true, false, false],
        ),
        (
            AttributeAction::SetPermissions(Permissions::new(0o7777)?),
            Operation::SetPermissions,
            [false, false, true, false, false],
        ),
        (
            AttributeAction::list_xattrs(NonZeroUsize::MIN),
            Operation::ListXattrs,
            [false, false, false, true, true],
        ),
        (
            AttributeAction::get_xattr(name.clone(), NonZeroUsize::MAX),
            Operation::GetXattr,
            [false, false, false, true, true],
        ),
        (
            AttributeAction::set_xattr(name.clone(), Vec::new(), XattrWriteMode::CreateNew),
            Operation::SetXattr,
            [false, false, false, false, true],
        ),
        (
            AttributeAction::RemoveXattr(name),
            Operation::RemoveXattr,
            [false, false, false, false, true],
        ),
    ] {
        let request = AttributeRequest::new(path.clone(), action);
        assert_eq!(request.operation(), expected);
        assert_eq!(
            declarations.map(|caps| caps.supports(request.operation())),
            admitted,
            "{expected:?}"
        );
        assert_eq!(request.path(), &path);
    }
    Ok(())
}

#[test]
fn canonical_target_checks_preserve_rejection_precedence() -> Result {
    let address = StoragePath::parse("entry")?;
    let file = EntryType::file(None);
    let directory = EntryType::Directory;
    let read = ReadRequest::new(address.clone(), ReadRange::FULL);
    let create = WriteRequest::new(address.clone(), WriteMode::CreateNew, Publication::Atomic)?;
    let replace = WriteRequest::new(address.clone(), WriteMode::Replace, Publication::Atomic)?;
    let mkdir = CreateDirectoryRequest::new(address.clone())?;
    let unlink = RemoveRequest::new(address.clone(), RemoveKind::NonDirectory)?;
    let rmdir = RemoveRequest::new(address.clone(), RemoveKind::EmptyDirectory)?;
    for (entry, read_result) in [
        (file, Ok(())),
        (directory, Err(Rejection::IsDirectory)),
        (EntryType::Symlink, Err(Rejection::NotRegularFile)),
        (EntryType::Other, Err(Rejection::NotRegularFile)),
    ] {
        assert_eq!(read.check_target(entry), read_result);
        assert_eq!(
            create.check_target(Some(entry)),
            Err(Rejection::AlreadyExists)
        );
        assert_eq!(replace.check_target(Some(entry)), read_result);
        assert_eq!(
            mkdir.check_target(Some(entry)),
            Err(Rejection::AlreadyExists)
        );
        if entry == directory {
            assert_eq!(unlink.check_target(entry), Err(Rejection::IsDirectory));
            assert!(rmdir.check_target(entry).is_ok());
        } else {
            assert!(unlink.check_target(entry).is_ok());
            assert_eq!(rmdir.check_target(entry), Err(Rejection::NotDirectory));
        }
    }
    for write in [create, replace] {
        assert!(write.check_target(None).is_ok());
    }
    assert!(mkdir.check_target(None).is_ok());

    let target = StoragePath::parse("target")?;
    let no_replace = RenameRequest::new(address.clone(), target.clone(), RenameMode::NoReplace)?;
    let rename = RenameRequest::new(address, target, RenameMode::Replace)?;
    assert_eq!(
        no_replace.decide_destination(file, RenameDestination::NonEmptyDirectory),
        Err(Rejection::AlreadyExists)
    );
    assert_eq!(
        rename.decide_destination(file, RenameDestination::NonEmptyDirectory),
        Err(Rejection::IsDirectory)
    );
    assert_eq!(
        rename.decide_destination(directory, RenameDestination::NonEmptyDirectory),
        Err(Rejection::DirectoryNotEmpty)
    );
    assert_eq!(
        rename.decide_destination(directory, RenameDestination::Occupied(file)),
        Err(Rejection::NotDirectory)
    );
    assert!(
        rename
            .decide_destination(directory, RenameDestination::Missing)
            .is_ok()
    );
    Ok(())
}
