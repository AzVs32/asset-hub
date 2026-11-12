use ah_vfs_storage::*;
use std::error::Error;
type Result = std::result::Result<(), Box<dyn Error>>;

#[test]
fn defaults_produce_valid_minimal_values() -> Result {
    let read = ReadRequest::new(PathExpr::parse("file")?, ReadRange::FULL);
    assert_eq!(read.range(), ReadRange::FULL);
    assert_eq!(ReadRange::new(0, None)?, ReadRange::FULL);
    let entry = Entry::new(StoragePath::parse("file")?, EntryType::file(None), None)?;
    assert_eq!(entry.entry_type(), EntryType::file(None));
    assert!(entry.modified().is_none());
    let query = ListRequest::new(ListTarget::Path(PathExpr::parse("/")?), PageSize::DEFAULT);
    assert_eq!(query.size(), PageSize::DEFAULT);
    let bound = query.bind_selection(DirectorySelection::from_lookup(
        StoragePath::root(),
        DirectoryIdentity::from_bytes([1; 16]),
    ))?;
    assert!(bound.after().is_none());
    let page = DirectoryPage::new(&bound, vec![], false)?;
    assert!(page.continuation().is_none());
    let write = WriteSupport::staged(WriteModes::Both, false);
    assert!(write.supports(WriteMode::Replace, Publication::Staged));
    assert!(!write.supports(WriteMode::Replace, Publication::Atomic));
    for support in [
        DirectorySupport::hierarchical(false, false),
        DirectorySupport::prefix(false, false),
    ] {
        assert!(!support.can_create());
        assert!(!support.persistent_empty());
    }
    Ok(())
}

#[test]
fn optional_members_accept_boundary_options_without_weakening_validation() -> Result {
    let entry = Entry::new(
        StoragePath::root(),
        EntryType::Directory,
        Some(std::time::UNIX_EPOCH),
    )?;
    assert_eq!(entry.modified(), Some(std::time::UNIX_EPOCH));
    let query = ListRequest::new(ListTarget::Path(PathExpr::parse("/")?), PageSize::DEFAULT);
    let bound = query.bind_selection(DirectorySelection::from_lookup(
        StoragePath::root(),
        DirectoryIdentity::from_bytes([1; 16]),
    ))?;
    let page = DirectoryPage::from_parts(
        &bound,
        bound.selection().address(),
        bound.selection().identity(),
        vec![],
        None,
    )?;
    assert!(page.continuation().is_none());
    let error = StorageError::after_dispatch(
        Operation::Write,
        Failure::Backend(BackendFailure::TimedOut),
        Some(Box::new(std::io::Error::other("private diagnostic"))),
    );
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert!(
        error
            .source()
            .and_then(|error| error.downcast_ref::<std::io::Error>())
            .is_some()
    );
    assert!(!format!("{error:?}").contains("private diagnostic"));
    let error = StorageError::without_effects(
        Operation::Write,
        Failure::Rejected(Rejection::Unsupported),
        None,
    );
    assert!(error.source().is_none());
    Ok(())
}

#[test]
fn generic_helpers_compose_public_constructors() -> Result {
    fn from_operand<T>(
        input: &str,
        construct: impl FnOnce(PathExpr) -> T,
    ) -> std::result::Result<T, PathExprError> {
        PathExpr::parse(input).map(construct)
    }

    let link = ReadLinkRequest::new(StoragePath::parse("work/link")?);
    assert_eq!(link.path().to_string(), "work/link");
    let remove = from_operand("/work/file", |path| {
        RemoveRequest::new(path, RemoveKind::NonDirectory)
    })??;
    assert_eq!(remove.kind(), RemoveKind::NonDirectory);
    assert_eq!(
        from_operand(".", |path| RemoveRequest::new(
            path,
            RemoveKind::NonDirectory
        ))?,
        Err(RequestPathError::ExpectedEntryName),
    );
    fn declare(configure: impl FnOnce(Capabilities) -> Capabilities) -> Capabilities {
        configure(Capabilities::default())
    }
    let capabilities = declare(|value| value.with_read_link(true));
    assert!(capabilities.supports(Operation::ReadLink));
    assert!(!capabilities.supports(Operation::Read));
    Ok(())
}

#[test]
fn external_executors_can_match_closed_grammar_and_policies() -> Result {
    fn rooted(start: LookupStart) -> bool {
        match start {
            LookupStart::InstanceRoot => true,
            LookupStart::BaseDirectory => false,
        }
    }
    fn component_text(component: &PathComponent) -> &str {
        match component {
            PathComponent::Name(name) => name.as_str(),
            PathComponent::CurrentDirectory => ".",
            PathComponent::ParentDirectory => "..",
        }
    }
    fn follows(action: SymlinkAction) -> bool {
        match action {
            SymlinkAction::FollowWithinInstance => true,
            SymlinkAction::Preserve => false,
        }
    }
    fn requires_reconciliation(outcome: MutationOutcome) -> bool {
        match outcome {
            MutationOutcome::Unchanged => false,
            MutationOutcome::Unknown => true,
        }
    }

    let operand = PathExpr::parse("/work/./../file")?;
    assert!(rooted(operand.start()));
    assert_eq!(
        operand
            .components()
            .iter()
            .map(component_text)
            .collect::<Vec<_>>(),
        ["work", ".", "..", "file"]
    );
    assert!(!follows(
        StatRequest::new(operand, SymlinkAction::Preserve)
            .lookup()
            .final_symlink()
    ));
    let error = StorageError::after_dispatch(
        Operation::Write,
        Failure::Backend(BackendFailure::TimedOut),
        None,
    );
    assert!(requires_reconciliation(error.outcome()));
    Ok(())
}

#[test]
fn external_attribute_callers_construct_and_borrow_payloads() -> Result {
    use std::num::NonZeroUsize;

    let max = NonZeroUsize::MIN;
    let name = XattrName::parse("user.label")?;
    let path = StoragePath::parse("file")?;
    let list = AttributeRequest::new(path.clone(), AttributeAction::list_xattrs(max));
    assert!(
        matches!(list.action(), AttributeAction::ListXattrs { max_bytes, .. } if *max_bytes == max)
    );
    let get = AttributeRequest::new(path.clone(), AttributeAction::get_xattr(name.clone(), max));
    assert!(
        matches!(get.action(), AttributeAction::GetXattr { name: key, max_bytes, .. } if *key == name && *max_bytes == max)
    );
    let permissions = Permissions::new(0o644)?;
    let set = AttributeRequest::new(path.clone(), AttributeAction::SetPermissions(permissions));
    assert_eq!(set.action(), &AttributeAction::SetPermissions(permissions));
    let remove = AttributeRequest::new(path, AttributeAction::RemoveXattr(name));
    assert_eq!(remove.operation(), Operation::RemoveXattr);
    Ok(())
}

#[test]
fn external_attribute_adapter_moves_the_path_and_original_buffer() -> Result {
    use std::{
        pin::pin,
        task::{Context, Poll, Waker},
    };

    struct SetXattrMessage {
        path: StoragePath,
        name: XattrName,
        data: Vec<u8>,
        mode: XattrWriteMode,
    }

    async fn prepare_message(request: AttributeRequest) -> Option<SetXattrMessage> {
        let (path, action) = request.into_parts();
        match action {
            AttributeAction::SetXattr {
                name, value, mode, ..
            } => Some(SetXattrMessage {
                path,
                name,
                data: value,
                mode,
            }),
            _ => None,
        }
    }

    fn require_owned<T: Send + 'static>(future: T) -> T {
        future
    }

    let path = StoragePath::parse("目录/file")?;
    let original_components = path.as_str().as_ptr();
    let value = vec![0, 255];
    let original_buffer = value.as_ptr();
    let name = XattrName::parse("user.label")?;
    let request = AttributeRequest::new(
        path,
        AttributeAction::set_xattr(name, value, XattrWriteMode::Replace),
    );

    assert_eq!(request.operation(), Operation::SetXattr);
    assert_eq!(request.path().to_string(), "目录/file");
    assert!(matches!(
        request.action(),
        AttributeAction::SetXattr { value, .. } if value.as_ptr() == original_buffer
    ));

    let mut context = Context::from_waker(Waker::noop());
    let Poll::Ready(Some(message)) =
        pin!(require_owned(prepare_message(request))).poll(&mut context)
    else {
        return Err("expected an owned SetXattr message".into());
    };
    assert_eq!(message.path.as_str().as_ptr(), original_components);
    assert_eq!(message.path.to_string(), "目录/file");
    assert_eq!(message.name.as_str(), "user.label");
    assert_eq!(message.mode, XattrWriteMode::Replace);
    assert_eq!(message.data, [0, 255]);
    assert_eq!(message.data.as_ptr(), original_buffer);
    Ok(())
}
