use ah_vfs::domain::storage2::*;
use std::error::Error;
type Result = std::result::Result<(), Box<dyn Error>>;

#[test]
fn defaults_produce_valid_minimal_values() -> Result {
    let read = ReadRequest::builder().path(StoragePath::root()).build();
    assert_eq!(read.range(), ReadRange::FULL);
    assert_eq!(ReadRange::builder().build()?, ReadRange::FULL);
    let entry = Entry::builder()
        .path(StoragePath::parse("file")?)
        .entry_type(EntryType::file().build())
        .build();
    assert_eq!(entry.entry_type(), EntryType::file().build());
    assert!(entry.modified().is_none());
    let query = ListRequest::builder()
        .directory(StoragePath::root())
        .build()?;
    assert_eq!(query.size(), PageSize::DEFAULT);
    assert!(query.cursor().is_none());
    let page = DirectoryPage::builder()
        .request(&query)
        .entries(vec![])
        .has_more(false)
        .build()?;
    assert!(page.next_after().is_none());
    let write = WriteSupport::staged().modes(WriteModes::Both).build();
    assert!(write.supports(WriteMode::Replace, Publication::Staged));
    assert!(!write.supports(WriteMode::Replace, Publication::Atomic));
    for support in [
        DirectorySupport::hierarchical().build(),
        DirectorySupport::prefix().build(),
    ] {
        assert!(!support.can_create());
        assert!(!support.persistent_empty());
    }
    Ok(())
}

#[test]
fn optional_members_accept_boundary_options_without_weakening_validation() -> Result {
    let entry = Entry::builder()
        .path(StoragePath::root())
        .entry_type(EntryType::Directory)
        .maybe_modified(Some(std::time::UNIX_EPOCH))
        .build();
    assert_eq!(entry.modified(), Some(std::time::UNIX_EPOCH));
    let query = ListRequest::builder()
        .directory(StoragePath::root())
        .maybe_after(None)
        .build()?;
    assert!(query.cursor().is_none());
    let page = DirectoryPage::from_parts()
        .request(&query)
        .entries(vec![])
        .next(None)
        .build()?;
    assert!(page.next_after().is_none());
    let error = StorageError::after_dispatch()
        .operation(Operation::Write)
        .failure(Failure::Backend(BackendFailure::TimedOut))
        .maybe_source(Some(std::io::Error::other("private diagnostic")))
        .build();
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert!(
        error
            .source()
            .and_then(|error| error.downcast_ref::<std::io::Error>())
            .is_some()
    );
    assert!(!format!("{error:?}").contains("private diagnostic"));
    let error = StorageError::without_effects()
        .operation(Operation::Write)
        .failure(Failure::Rejected(Rejection::Unsupported))
        .maybe_source(None::<std::io::Error>)
        .build();
    assert!(error.source().is_none());
    Ok(())
}

#[test]
fn builder_types_and_typestate_traits_are_available_to_external_callers() -> Result {
    fn read_builder() -> ReadRequestBuilder {
        ReadRequest::builder()
    }
    fn finish<S: capabilities_builder::IsComplete>(
        builder: CapabilitiesBuilder<S>,
    ) -> Capabilities {
        builder.build()
    }
    let read = read_builder().path(StoragePath::parse("file")?).build();
    assert_eq!(read.path().as_str(), "file");
    assert!(finish(Capabilities::builder().stat(true)).supports(Operation::Stat));
    Ok(())
}
