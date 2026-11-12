use ah_vfs::resolve::{
    ResolveFailure,
    operand::{
        CreateDirectoryMode, CreateDirectoryOperand, OperandError, RemoveOperand, RenameOperand,
        StatOperand, WriteOperand,
    },
    path::{LookupStart, PathComponent, PathExpr, RootedPath},
    policy::SymlinkAction,
};
use ah_vfs_storage::{
    BackendFailure, Entry, EntryType, Failure, MAX_PATH_BYTES, MutationOutcome, Operation,
    Publication, RemoveKind, RenameMode, StorageError, StoragePath, WriteMode,
};
type Result = std::result::Result<(), Box<dyn std::error::Error>>;

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
        StatOperand::new(operand, SymlinkAction::Preserve)
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
fn selected_entry_addresses_become_exact_instance_rooted_request_operands() -> Result {
    let maximum = "x".repeat(MAX_PATH_BYTES);
    for text in ["", "file", "docs/Report", "文档/é/e\u{301}", &maximum] {
        let address = StoragePath::parse(text)?;
        let entry_type = if address.is_root() {
            EntryType::Directory
        } else {
            EntryType::file(None)
        };
        let selected = Entry::new(address, entry_type, None)?;
        let request = StatOperand::new(selected.path().to_rooted_expr(), SymlinkAction::Preserve);

        let operand = request.lookup().path();
        assert_eq!(operand.start(), LookupStart::InstanceRoot);
        assert_eq!(operand.requires_directory(), selected.path().is_root());
        let rendered = operand.to_string();
        assert_eq!(rendered.strip_prefix('/'), Some(text));
        assert_eq!(&PathExpr::parse(&rendered)?, operand);
    }
    Ok(())
}

#[test]
fn operation_requests_protect_the_selected_root_and_validate_entry_operands() -> Result {
    let root = StoragePath::root();
    let write = WriteOperand::new(
        PathExpr::parse("alias")?,
        WriteMode::CreateNew,
        Publication::Atomic,
    );
    assert_eq!(
        write.lookup().check_address(&root),
        Err(ResolveFailure::RootProtected)
    );

    let rename = RenameOperand::new(
        PathExpr::parse("a")?,
        PathExpr::parse("b")?,
        RenameMode::NoReplace,
    )?;
    assert_eq!(
        rename.lookups().0.check_address(&root),
        Err(ResolveFailure::RootProtected)
    );
    assert_eq!(
        rename.lookups().1.check_address(&root),
        Err(ResolveFailure::RootProtected)
    );
    let remove = RemoveOperand::new(PathExpr::parse("link")?, RemoveKind::NonDirectory)?;
    assert_eq!(
        remove.lookup().check_address(&root),
        Err(ResolveFailure::RootProtected)
    );
    for operand in ["/", ".", "..", "a/../"] {
        assert_eq!(
            RemoveOperand::new(PathExpr::parse(operand)?, RemoveKind::EmptyDirectory),
            Err(OperandError::ExpectedEntryName),
        );
        assert_eq!(
            RenameOperand::new(
                PathExpr::parse("a")?,
                PathExpr::parse(operand)?,
                RenameMode::NoReplace
            ),
            Err(OperandError::ExpectedEntryName),
        );
        assert_eq!(
            RenameOperand::new(
                PathExpr::parse(operand)?,
                PathExpr::parse("b")?,
                RenameMode::Replace
            ),
            Err(OperandError::ExpectedEntryName),
        );
    }
    let maximum = PathExpr::parse(&format!("/{}/", "a".repeat(MAX_PATH_BYTES)))?;
    assert_eq!(
        RemoveOperand::new(maximum.clone(), RemoveKind::EmptyDirectory)?.path(),
        &maximum
    );
    assert_eq!(
        RenameOperand::new(PathExpr::parse("a")?, maximum.clone(), RenameMode::Replace)?
            .destination(),
        &maximum
    );

    let recursive =
        CreateDirectoryOperand::new(PathExpr::parse("/")?, CreateDirectoryMode::Recursive);
    assert_eq!(
        recursive
            .lookup()
            .check_existing_target(&root, EntryType::Directory),
        Ok(())
    );
    Ok(())
}
