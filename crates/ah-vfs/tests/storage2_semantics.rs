use ah_vfs::domain::storage2::*;
use std::error::Error;
type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
fn target() -> Result<NonRootPath> {
    Ok(StoragePath::parse("file")?.try_into()?)
}
const READS: [Operation; 6] = [
    Operation::Stat,
    Operation::List,
    Operation::Read,
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
            let error = StorageError::after_dispatch()
                .operation(operation)
                .failure(failure)
                .build();
            assert_eq!(error.failure(), failure);
            assert_eq!(error.operation(), operation);
            assert_eq!(error.outcome(), MutationOutcome::Unknown);
            assert_eq!(
                StorageError::without_effects()
                    .operation(operation)
                    .failure(failure)
                    .build()
                    .outcome(),
                MutationOutcome::Unchanged
            );
        }
        for operation in READS {
            assert_eq!(
                StorageError::after_dispatch()
                    .operation(operation)
                    .failure(failure)
                    .build()
                    .outcome(),
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
    let error = StorageError::after_dispatch()
        .operation(Operation::Write)
        .failure(failure)
        .source(cause)
        .build();
    assert_eq!(error.failure(), failure);
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<PathError>())
            .is_some()
    );
    let error = StorageError::after_dispatch()
        .operation(Operation::Write)
        .failure(failure)
        .source(error)
        .build();
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
    let error = StorageError::after_dispatch()
        .operation(Operation::Write)
        .failure(failure)
        .source(std::io::Error::other("token=private-secret"))
        .build();
    assert!(!error.to_string().contains("private-secret"));
    assert!(!format!("{error:?}").contains("private-secret"));
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
    let invalid_page = DirectoryPage::builder()
        .request(
            &ListRequest::builder()
                .directory(StoragePath::root())
                .build()?,
        )
        .entries(vec![])
        .has_more(true)
        .build()
        .err()
        .ok_or("malformed page")?;
    let protocol = StorageError::after_dispatch()
        .operation(Operation::List)
        .failure(Failure::Contract(ContractViolation::InvalidPage))
        .source(invalid_page)
        .build();
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
    let corrupt = StorageError::after_dispatch()
        .operation(Operation::Read)
        .failure(Failure::Backend(BackendFailure::CorruptData))
        .build();
    assert_ne!(protocol.failure(), corrupt.failure());
    Ok(())
}
#[test]
fn capability_defaults_disable_every_optional_operation() {
    let capabilities = Capabilities::builder().build();
    for operation in READS.into_iter().chain(MUTATIONS) {
        assert!(!capabilities.supports(operation));
    }
}
#[test]
fn declarations_remain_infallible_and_immutable_when_composed() {
    let base = Capabilities::builder().build();
    let extended = Capabilities::builder()
        .stat(true)
        .list(true)
        .permissions(AttributeSupport::ReadWrite)
        .xattrs(AttributeSupport::Read)
        .build();
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
            let capabilities = Capabilities::builder()
                .write(
                    WriteSupport::staged()
                        .modes(modes)
                        .atomic_publish(atomic_publish)
                        .build(),
                )
                .build();
            for mode in [WriteMode::CreateNew, WriteMode::Replace] {
                for publication in [Publication::Staged, Publication::Atomic] {
                    let request = WriteRequest::builder()
                        .path(target()?)
                        .mode(mode)
                        .publication(publication)
                        .build();
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
    let capabilities = Capabilities::builder().build();
    let request = WriteRequest::builder()
        .path(target()?)
        .mode(WriteMode::CreateNew)
        .publication(Publication::Staged)
        .build();
    let cause = capabilities
        .require_write(&request)
        .err()
        .ok_or("unsupported write admitted")?;
    let failure = StorageError::without_effects()
        .operation(Operation::Write)
        .failure(Failure::Rejected(cause))
        .build();
    assert_eq!(failure.failure(), Failure::Rejected(Rejection::Unsupported));
    assert_eq!(failure.outcome(), MutationOutcome::Unchanged);
    Ok(())
}
#[test]
fn read_admission_distinguishes_sequential_prefixes_from_arbitrary_ranges() -> Result {
    let prefix = ReadRequest::builder()
        .path(StoragePath::parse("file")?)
        .range(
            ReadRange::builder()
                .offset(0)
                .maybe_length(Some(3))
                .build()?,
        )
        .build();
    let range = ReadRequest::builder()
        .path(StoragePath::parse("file")?)
        .range(
            ReadRange::builder()
                .offset(1)
                .maybe_length(Some(3))
                .build()?,
        )
        .build();
    let sequential = Capabilities::builder()
        .read(ReadSupport::Sequential)
        .build();
    assert!(sequential.require_read(&prefix).is_ok());
    assert_eq!(sequential.require_read(&range), Err(Rejection::Unsupported));
    assert!(
        Capabilities::builder()
            .read(ReadSupport::Range)
            .build()
            .require_read(&range)
            .is_ok()
    );
    assert_eq!(
        Capabilities::builder().build().require_read(&prefix),
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
                DirectorySupport::hierarchical()
                    .create(create)
                    .persistent_empty(persistent_empty)
                    .build(),
                DirectorySupport::prefix()
                    .create(create)
                    .persistent_empty(persistent_empty)
                    .build(),
            ] {
                let capabilities = Capabilities::builder().directories(support).build();
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
        let capabilities = Capabilities::builder()
            .xattrs(support)
            .permissions(support)
            .build();
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
        let capabilities = Capabilities::builder().rename(support).build();
        for (mode, supported) in [
            (RenameMode::NoReplace, no_replace),
            (RenameMode::Replace, replace),
        ] {
            let request = RenameRequest::builder()
                .source(target()?)
                .destination(target()?)
                .mode(mode)
                .build();
            assert_eq!(capabilities.require_rename(&request).is_ok(), supported);
        }
    }
    Ok(())
}

#[test]
fn directory_creation_has_one_existing_target_policy_for_each_mode() {
    for (entry_type, single, recursive) in [
        (EntryType::Directory, Err(Rejection::AlreadyExists), Ok(())),
        (
            EntryType::file().size(0).build(),
            Err(Rejection::AlreadyExists),
            Err(Rejection::NotDirectory),
        ),
        (
            EntryType::Symlink,
            Err(Rejection::AlreadyExists),
            Err(Rejection::NotDirectory),
        ),
        (
            EntryType::Other,
            Err(Rejection::AlreadyExists),
            Err(Rejection::NotDirectory),
        ),
    ] {
        assert_eq!(
            CreateDirectoryMode::Single.check_existing_target(entry_type),
            single
        );
        assert_eq!(
            CreateDirectoryMode::Recursive.check_existing_target(entry_type),
            recursive
        );
    }
}

#[test]
fn extensible_payloads_are_built_and_matched_through_the_public_contract() -> Result {
    let EntryType::File { size, .. } = EntryType::file().size(42).build() else {
        return Err("expected a file".into());
    };
    assert_eq!(size, Some(42));
    let WriteSupport::Staged {
        modes,
        atomic_publish,
        ..
    } = WriteSupport::staged()
        .modes(WriteModes::Both)
        .atomic_publish(true)
        .build()
    else {
        return Err("expected staged write support".into());
    };
    assert_eq!(modes, WriteModes::Both);
    assert!(atomic_publish);
    for support in [
        DirectorySupport::hierarchical().create(true).build(),
        DirectorySupport::prefix().create(true).build(),
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
