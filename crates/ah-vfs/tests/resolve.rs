//! Exercise the VFS resolver against an isolated namespace that never follows links.
use ah_vfs::resolve::{
    LookupPort, MAX_SYMLINK_EXPANSIONS, Resolver, SymlinkTraversal, prepare_link_target,
};
use ah_vfs_storage::*;
use std::{collections::BTreeMap, error::Error, sync::Arc};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

enum Node {
    Directory,
    File,
    Link(LinkTarget),
}

impl Node {
    fn link(target: &str) -> Self {
        Self::Link(LinkTarget::new(target.as_bytes().to_vec()))
    }

    fn kind(&self) -> EntryType {
        match self {
            Self::Directory => EntryType::Directory,
            Self::File => EntryType::file(None),
            Self::Link(_) => EntryType::Symlink,
        }
    }
}

struct Namespace {
    generation: Arc<()>,
    nodes: BTreeMap<StoragePath, Arc<Node>>,
    competing_creation: Option<StoragePath>,
    fail_final_verification: bool,
}

#[derive(Clone)]
struct Binding {
    generation: Arc<()>,
    entry: Entry,
    node: Arc<Node>,
}

#[test]
fn listed_native_spellings_reopen_the_same_exact_slots() -> TestResult {
    let mut namespace = Namespace::new([
        ("docs", Node::Directory),
        ("docs/Report", Node::File),
        ("docs/report", Node::Directory),
        ("docs/é", Node::File),
        ("docs/e\u{301}", Node::Directory),
    ])?;
    let root = StoragePath::root();
    let request = ListRequest::new(
        ListTarget::Path(PathExpr::parse("/docs")?),
        PageSize::DEFAULT,
    );
    let directory = namespace.select(
        request.lookup().ok_or("initial list lookup")?,
        &root,
        IntermediateSymlinks::FollowWithinInstance,
    )?;
    let mut names = [
        "report".as_bytes(),
        "é".as_bytes(),
        "Report".as_bytes(),
        "e\u{301}".as_bytes(),
    ]
    .map(EntryName::from_backend_bytes)
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    let entries = names
        .iter()
        .map(|name| -> TestResult<Entry> {
            let child = directory.join(name)?;
            let node = namespace.nodes.get(&child).ok_or("native child")?;
            Ok(Entry::new(child, node.kind(), None)?)
        })
        .collect::<TestResult<Vec<_>>>()?;
    let bound = request.bind_selection(DirectorySelection::from_lookup(
        directory,
        DirectoryIdentity::from_bytes([1; 16]),
    ))?;
    let page = DirectoryPage::new(&bound, entries, false)?;
    let cwd = StoragePath::parse("docs/report")?;
    for (entry, expected) in
        page.entries()
            .iter()
            .zip(["docs/Report", "docs/e\u{301}", "docs/report", "docs/é"])
    {
        let stat = StatRequest::new(entry.path().to_rooted_expr(), SymlinkAction::Preserve);
        let selected = namespace.select(
            stat.lookup(),
            &cwd,
            IntermediateSymlinks::FollowWithinInstance,
        )?;
        assert_eq!(selected.as_str(), expected);
        assert_eq!(&selected, entry.path());
    }
    Ok(())
}

impl Namespace {
    fn new(entries: impl IntoIterator<Item = (&'static str, Node)>) -> TestResult<Self> {
        let mut nodes = BTreeMap::from([(StoragePath::root(), Arc::new(Node::Directory))]);
        for (path, node) in entries {
            nodes.insert(StoragePath::parse(path)?, Arc::new(node));
        }
        Ok(Self {
            generation: Arc::new(()),
            nodes,
            competing_creation: None,
            fail_final_verification: false,
        })
    }

    fn binding(&self, path: StoragePath, op: Operation) -> Result<Binding, StorageError> {
        let node = self
            .nodes
            .get(&path)
            .ok_or_else(|| rejected(op, Rejection::NotFound))?;
        let entry = Entry::new(path, node.kind(), None).map_err(|error| {
            StorageError::without_effects(
                op,
                Failure::Contract(ContractViolation::InvalidEntry),
                Some(Box::new(error)),
            )
        })?;
        Ok(Binding {
            generation: self.generation.clone(),
            entry,
            node: node.clone(),
        })
    }

    fn verify(&self, binding: &Binding, op: Operation) -> Result<(), StorageError> {
        if !Arc::ptr_eq(&self.generation, &binding.generation) {
            return Err(StorageError::without_effects(
                op,
                Failure::Backend(BackendFailure::Stale),
                None,
            ));
        }
        if self
            .nodes
            .get(binding.entry.path())
            .is_none_or(|node| !Arc::ptr_eq(node, &binding.node))
        {
            return Err(rejected(op, Rejection::Conflict));
        }
        Ok(())
    }

    fn remove_atomically(
        &mut self,
        binding: &Binding,
        before_effect: impl FnOnce(&mut Self),
    ) -> Result<(), StorageError> {
        let op = Operation::Remove;
        self.verify(binding, op)?;
        before_effect(self);
        self.verify(binding, op)?;
        self.nodes.remove(binding.entry.path());
        Ok(())
    }

    fn directories(&self, cwd: &StoragePath, op: Operation) -> Result<Vec<Binding>, StorageError> {
        let mut chain = Vec::new();
        let mut path = Some(cwd.clone());
        while let Some(address) = path {
            path = address.parent();
            chain.push(self.binding(address, op)?);
        }
        chain.reverse();
        Ok(chain)
    }

    fn select(
        &mut self,
        lookup: PathLookup<'_>,
        cwd: &StoragePath,
        links: IntermediateSymlinks,
    ) -> Result<StoragePath, Failure> {
        let op = Operation::Stat;
        let directories = self.directories(cwd, op).map_err(|error| error.failure())?;
        let selected = Resolver::new(op, links)
            .walk(
                &mut Fixture {
                    namespace: self,
                    op,
                },
                lookup,
                directories,
            )
            .and_then(|selected| selected.require_entry(op))
            .map_err(|error| error.failure())?;
        lookup
            .check_existing_target(selected.entry.path(), selected.entry.entry_type())
            .map_err(Failure::Rejected)?;
        Ok(selected.entry.path().clone())
    }

    fn mkdir_recursive(
        &mut self,
        request: &CreateDirectoryRequest,
    ) -> Result<StoragePath, StorageError> {
        let op = Operation::CreateDirectory;
        let directories = self.directories(&StoragePath::root(), op)?;
        Resolver::new(op, IntermediateSymlinks::default())
            .walk(
                &mut Fixture {
                    namespace: self,
                    op,
                },
                request.lookup(),
                directories,
            )
            .and_then(|selected| selected.require_entry(op))
            .map(|selected| selected.entry.path().clone())
    }

    fn rename_decision(
        &mut self,
        request: &RenameRequest,
        cwd: &StoragePath,
    ) -> Result<RenameDecision, Failure> {
        let op = Operation::Rename;
        let (source, destination) = request.lookups();
        let directories = self.directories(cwd, op).map_err(|error| error.failure())?;
        let mut resolver = Resolver::new(op, IntermediateSymlinks::default());
        let mut backend = Fixture {
            namespace: self,
            op,
        };
        let (parent, name) = resolver
            .parent(&mut backend, source, directories.clone())
            .map_err(|error| error.failure())?;
        let source_address = parent
            .entry
            .path()
            .join(&name)
            .map_err(|_| Failure::Rejected(Rejection::PathTooLong))?;
        let source_entry = backend
            .observe(&parent, source_address)
            .map_err(|error| error.failure())?;
        let kind = source_entry.entry.entry_type();
        source
            .check_existing_target(source_entry.entry.path(), kind)
            .map_err(Failure::Rejected)?;

        let (parent, name) = resolver
            .parent(&mut backend, destination, directories)
            .map_err(|error| error.failure())?;
        let address = parent
            .entry
            .path()
            .join(&name)
            .map_err(|_| Failure::Rejected(Rejection::PathTooLong))?;
        let observation = match self.nodes.get(&address).map(|node| node.kind()) {
            None => RenameDestination::Missing,
            Some(_) if source_entry.entry.path() == &address => RenameDestination::SameSlot,
            Some(EntryType::Directory)
                if self
                    .nodes
                    .keys()
                    .any(|child| child.parent().as_ref() == Some(&address)) =>
            {
                RenameDestination::NonEmptyDirectory
            }
            Some(kind) => RenameDestination::Occupied(kind),
        };
        request
            .decide_destination(&address, kind, observation)
            .map_err(Failure::Rejected)
    }
}

struct Fixture<'a> {
    namespace: &'a mut Namespace,
    op: Operation,
}

impl Fixture<'_> {
    fn observe(&self, parent: &Binding, address: StoragePath) -> Result<Binding, StorageError> {
        self.namespace.verify(parent, self.op)?;
        self.namespace.binding(address, self.op)
    }
}

impl LookupPort for Fixture<'_> {
    type Binding = Binding;

    fn entry(binding: &Binding) -> &Entry {
        &binding.entry
    }

    async fn observe_child(
        &mut self,
        parent: &Binding,
        address: StoragePath,
        _name: &EntryName,
    ) -> Result<Binding, StorageError> {
        self.observe(parent, address)
    }

    async fn read_bound_link(&self, entry: &Binding) -> Result<LinkTarget, StorageError> {
        match entry.node.as_ref() {
            Node::Link(link) => Ok(link.clone()),
            _ => Err(rejected(self.op, Rejection::NotSymlink)),
        }
    }

    async fn create_child(
        &mut self,
        parent: &Binding,
        address: StoragePath,
        _name: &EntryName,
    ) -> Result<Binding, StorageError> {
        self.namespace.verify(parent, self.op)?;
        let std::collections::btree_map::Entry::Vacant(slot) =
            self.namespace.nodes.entry(address.clone())
        else {
            return Err(rejected(self.op, Rejection::AlreadyExists));
        };
        slot.insert(Arc::new(Node::Directory));
        if self.namespace.competing_creation.as_ref() == Some(&address) {
            return Err(rejected(self.op, Rejection::AlreadyExists));
        }
        self.namespace.binding(address, self.op)
    }

    async fn verify_observations(&self) -> Result<(), StorageError> {
        if self.namespace.fail_final_verification {
            return Err(rejected(self.op, Rejection::Conflict));
        }
        Ok(())
    }
}

fn rejected(op: Operation, reason: Rejection) -> StorageError {
    StorageError::without_effects(op, Failure::Rejected(reason), None)
}

#[test]
fn final_slot_atomic_rejects_same_type_replacements_before_the_effect() -> TestResult {
    for (before, after) in [
        (Node::Directory, Node::Directory),
        (Node::File, Node::File),
        (Node::link("before"), Node::link("after")),
    ] {
        let mut namespace = Namespace::new([("target", before)])?;
        let path = StoragePath::parse("target")?;
        let stat = StatRequest::new(PathExpr::parse("/target")?, SymlinkAction::Preserve);
        let op = Operation::Remove;
        let directories = namespace.directories(&StoragePath::root(), op)?;
        let bound = Resolver::new(op, IntermediateSymlinks::default())
            .walk(
                &mut Fixture {
                    namespace: &mut namespace,
                    op,
                },
                stat.lookup(),
                directories,
            )?
            .require_entry(op)?;
        let replacement = Arc::new(after);
        let error = namespace
            .remove_atomically(&bound, |namespace| {
                namespace.nodes.insert(path.clone(), replacement.clone());
            })
            .err()
            .ok_or("a changed slot was removed")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::Conflict));
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
        assert!(
            namespace
                .nodes
                .get(&path)
                .is_some_and(|node| Arc::ptr_eq(node, &replacement))
        );
    }
    Ok(())
}

#[test]
fn an_observation_from_a_retired_generation_is_stale() -> TestResult {
    let mut namespace = Namespace::new([("target", Node::File)])?;
    let path = StoragePath::parse("target")?;
    let bound = namespace.binding(path.clone(), Operation::Remove)?;
    namespace.generation = Arc::new(());
    let error = namespace
        .remove_atomically(&bound, |_| {})
        .err()
        .ok_or("stale binding was accepted")?;
    assert_eq!(error.failure(), Failure::Backend(BackendFailure::Stale));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    assert!(namespace.nodes.contains_key(&path));
    Ok(())
}

#[test]
fn parent_navigation_does_not_skip_failed_prefixes_and_stays_inside_root() -> TestResult {
    let mut fs = Namespace::new([
        ("target", Node::File),
        ("file", Node::File),
        ("dir", Node::Directory),
        ("dir/back", Node::link("../../target")),
    ])?;
    let root = StoragePath::root();
    for (path, expected) in [
        (
            "missing/../target",
            Err(Failure::Rejected(Rejection::NotFound)),
        ),
        (
            "file/../target",
            Err(Failure::Rejected(Rejection::NotDirectory)),
        ),
        ("dir/../target", Ok(StoragePath::parse("target")?)),
        ("/../../target", Ok(StoragePath::parse("target")?)),
        ("dir/back", Ok(StoragePath::parse("target")?)),
    ] {
        let read = ReadRequest::new(PathExpr::parse(path)?, ReadRange::FULL);
        assert_eq!(
            fs.select(read.lookup(), &root, IntermediateSymlinks::default()),
            expected,
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn expanded_links_keep_intermediate_final_and_trailing_slash_rules() -> TestResult {
    let mut fs = Namespace::new([
        ("dir", Node::Directory),
        ("dir/file", Node::File),
        ("alias", Node::link("dir")),
        ("end", Node::link("dir/file")),
        ("nested", Node::link("alias/file")),
        ("target_slash", Node::link("dir/file/")),
    ])?;
    let root = StoragePath::root();
    let follow = SymlinkAction::FollowWithinInstance;
    let preserve = SymlinkAction::Preserve;
    for (path, final_link, expected) in [
        (
            "alias/file",
            follow,
            Err(Failure::Rejected(Rejection::SymlinkNotAllowed)),
        ),
        (
            "nested",
            follow,
            Err(Failure::Rejected(Rejection::SymlinkNotAllowed)),
        ),
        ("end", follow, Ok(StoragePath::parse("dir/file")?)),
        ("end", preserve, Ok(StoragePath::parse("end")?)),
        ("alias/", follow, Ok(StoragePath::parse("dir")?)),
        (
            "alias/",
            preserve,
            Err(Failure::Rejected(Rejection::NotDirectory)),
        ),
        (
            "end/",
            follow,
            Err(Failure::Rejected(Rejection::NotDirectory)),
        ),
        (
            "target_slash",
            follow,
            Err(Failure::Rejected(Rejection::NotDirectory)),
        ),
    ] {
        let stat = StatRequest::new(PathExpr::parse(path)?, final_link);
        assert_eq!(
            fs.select(stat.lookup(), &root, IntermediateSymlinks::Reject),
            expected,
            "{path} {final_link:?}"
        );
    }
    let stat = StatRequest::new(PathExpr::parse("nested")?, follow);
    assert_eq!(
        fs.select(stat.lookup(), &root, IntermediateSymlinks::default())?,
        StoragePath::parse("dir/file")?,
    );
    Ok(())
}

#[test]
fn cycles_exhaust_the_budget_and_dangling_links_cannot_create_their_targets() -> TestResult {
    let mut fs = Namespace::new([
        ("first", Node::link("second")),
        ("second", Node::link("/first")),
        ("dangling", Node::link("missing")),
    ])?;
    let root = StoragePath::root();
    let links = IntermediateSymlinks::default();
    for (path, rejection) in [
        ("first", Rejection::TooManySymlinks),
        ("dangling", Rejection::NotFound),
    ] {
        let stat = StatRequest::new(PathExpr::parse(path)?, SymlinkAction::FollowWithinInstance);
        assert_eq!(
            fs.select(stat.lookup(), &root, links),
            Err(Failure::Rejected(rejection))
        );
        let read_link = StatRequest::new(PathExpr::parse(path)?, SymlinkAction::Preserve);
        assert_eq!(
            fs.select(read_link.lookup(), &root, links)?,
            StoragePath::parse(path)?
        );
    }

    for path in ["dangling", "dangling/new"] {
        let request =
            CreateDirectoryRequest::new(PathExpr::parse(path)?, CreateDirectoryMode::Recursive);
        let error = fs
            .mkdir_recursive(&request)
            .err()
            .ok_or("dangling link must fail")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::NotFound));
        assert_eq!(error.outcome(), MutationOutcome::Unchanged);
        assert!(!fs.nodes.contains_key(&StoragePath::parse("missing")?));
    }
    Ok(())
}

#[test]
fn recursive_creation_reports_known_partial_effects_separately_from_failure() -> TestResult {
    let mut fs = Namespace::new([("file", Node::File)])?;
    let request = CreateDirectoryRequest::new(
        PathExpr::parse("created/../file/child")?,
        CreateDirectoryMode::Recursive,
    );
    for outcome in [MutationOutcome::Unknown, MutationOutcome::Unchanged] {
        let error = fs
            .mkdir_recursive(&request)
            .err()
            .ok_or("file is not a directory")?;
        assert_eq!(error.operation(), Operation::CreateDirectory);
        assert_eq!(error.failure(), Failure::Rejected(Rejection::NotDirectory));
        assert_eq!(error.outcome(), outcome);
        assert!(matches!(
            fs.nodes
                .get(&StoragePath::parse("created")?)
                .map(Arc::as_ref),
            Some(Node::Directory)
        ));
        assert!(!fs.nodes.contains_key(&StoragePath::parse("file/child")?));
    }
    Ok(())
}

#[test]
fn recursive_creation_does_not_claim_a_competing_operations_effect() -> TestResult {
    let mut fs = Namespace::new([("file", Node::File)])?;
    let parent = StoragePath::parse("competing")?;
    fs.competing_creation = Some(parent.clone());
    let request = CreateDirectoryRequest::new(
        PathExpr::parse("competing/../file/child")?,
        CreateDirectoryMode::Recursive,
    );

    let error = fs
        .mkdir_recursive(&request)
        .err()
        .ok_or("expected lookup failure")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::NotDirectory));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    assert!(fs.nodes.contains_key(&parent));
    Ok(())
}

#[test]
fn recursive_creation_includes_effects_when_final_verification_fails() -> TestResult {
    let mut fs = Namespace::new([])?;
    fs.fail_final_verification = true;
    let request = CreateDirectoryRequest::new(
        PathExpr::parse("parent/child")?,
        CreateDirectoryMode::Recursive,
    );

    let error = fs
        .mkdir_recursive(&request)
        .err()
        .ok_or("expected verification failure")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::Conflict));
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert!(fs.nodes.contains_key(&StoragePath::parse("parent/child")?));
    Ok(())
}

#[test]
fn rename_checks_resolved_slots_before_replacement_emptiness() -> TestResult {
    let mut fs = Namespace::new([
        ("work", Node::Directory),
        ("work/source", Node::Directory),
        ("work/source/child", Node::File),
        ("work/alias", Node::link(".")),
        ("work/busy", Node::Directory),
        ("work/busy/child", Node::File),
        ("work/empty", Node::Directory),
    ])?;
    let cwd = StoragePath::parse("work")?;
    for (destination, mode, expected) in [
        (
            "alias/./source/",
            RenameMode::Replace,
            Ok(RenameDecision::Unchanged),
        ),
        (
            "alias/./source/",
            RenameMode::NoReplace,
            Err(Failure::Rejected(Rejection::AlreadyExists)),
        ),
        (
            "busy/",
            RenameMode::Replace,
            Err(Failure::Rejected(Rejection::DirectoryNotEmpty)),
        ),
        (
            "busy/",
            RenameMode::NoReplace,
            Err(Failure::Rejected(Rejection::AlreadyExists)),
        ),
        ("empty/", RenameMode::Replace, Ok(RenameDecision::Move)),
    ] {
        let rename = RenameRequest::new(
            PathExpr::parse("/work//source")?,
            PathExpr::parse(destination)?,
            mode,
        )?;
        assert_eq!(
            fs.rename_decision(&rename, &cwd),
            expected,
            "{destination} {mode:?}"
        );
    }
    let missing = RenameRequest::new(
        PathExpr::parse("missing")?,
        PathExpr::parse("alias/missing")?,
        RenameMode::Replace,
    )?;
    assert_eq!(
        fs.rename_decision(&missing, &cwd),
        Err(Failure::Rejected(Rejection::NotFound))
    );
    Ok(())
}

#[test]
fn directory_creation_has_one_existing_target_policy_for_each_mode() -> TestResult {
    let address = StoragePath::parse("dir")?;
    let operand = PathExpr::parse("dir")?;
    let single_request = CreateDirectoryRequest::new(operand.clone(), CreateDirectoryMode::Single);
    let recursive_request = CreateDirectoryRequest::new(operand, CreateDirectoryMode::Recursive);
    assert_eq!(
        single_request.lookup().final_symlink(),
        SymlinkAction::Preserve
    );
    assert_eq!(
        recursive_request.lookup().final_symlink(),
        SymlinkAction::FollowWithinInstance
    );
    for (entry_type, single, recursive) in [
        (EntryType::Directory, Err(Rejection::AlreadyExists), Ok(())),
        (
            EntryType::file(Some(0)),
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
            single_request
                .lookup()
                .check_existing_target(&address, entry_type),
            single
        );
        assert_eq!(
            recursive_request
                .lookup()
                .check_existing_target(&address, entry_type),
            recursive
        );
    }
    Ok(())
}

#[test]
fn file_and_entry_operations_use_different_final_link_policies() -> TestResult {
    let path = PathExpr::parse("link")?;
    let link = StoragePath::parse("work/link")?;
    let target = StoragePath::parse("work/target")?;
    let file = EntryType::file(None);
    let read = ReadRequest::new(path.clone(), ReadRange::FULL);
    let link_lookup = StatRequest::new(path.clone(), SymlinkAction::Preserve);
    let read_link = ReadLinkRequest::new(link.clone());
    let remove = RemoveRequest::new(path, RemoveKind::NonDirectory)?;

    assert_eq!(
        read.lookup().final_symlink(),
        SymlinkAction::FollowWithinInstance
    );
    assert_eq!(read.lookup().check_existing_target(&target, file), Ok(()));
    assert_eq!(
        link_lookup.lookup().final_symlink(),
        SymlinkAction::Preserve
    );
    assert_eq!(read_link.check_target(EntryType::Symlink), Ok(()));
    assert_eq!(read_link.check_target(file), Err(Rejection::NotSymlink));
    assert_eq!(remove.lookup().final_symlink(), SymlinkAction::Preserve);
    assert_eq!(
        remove
            .lookup()
            .check_existing_target(&link, EntryType::Symlink),
        Ok(())
    );

    let slash = RemoveRequest::new(PathExpr::parse("link/")?, RemoveKind::NonDirectory)?;
    assert_eq!(
        slash
            .lookup()
            .check_existing_target(&link, EntryType::Symlink),
        Err(Rejection::NotDirectory)
    );
    Ok(())
}

#[test]
fn destination_rules_apply_to_absent_and_existing_entries() -> TestResult {
    let file = EntryType::file(None);
    let address = StoragePath::parse("work/dest")?;
    let create = WriteRequest::new(
        PathExpr::parse("dest")?,
        WriteMode::CreateNew,
        Publication::Atomic,
    );
    assert_eq!(create.check_new_destination(&address), Ok(()));
    assert_eq!(
        create.lookup().check_existing_target(&address, file),
        Err(Rejection::AlreadyExists)
    );

    let write = WriteRequest::new(
        PathExpr::parse("dest/")?,
        WriteMode::CreateNew,
        Publication::Atomic,
    );
    assert_eq!(
        write.check_new_destination(&address),
        Err(Rejection::NotDirectory)
    );
    let rename = RenameRequest::new(
        PathExpr::parse("src")?,
        PathExpr::parse("dest/")?,
        RenameMode::Replace,
    )?;
    assert_eq!(
        rename.decide_destination(&address, EntryType::Directory, RenameDestination::Missing),
        Ok(RenameDecision::Move)
    );
    assert_eq!(
        rename.decide_destination(&address, file, RenameDestination::Missing),
        Err(Rejection::NotDirectory)
    );
    assert_eq!(
        rename.decide_destination(&address, file, RenameDestination::SameSlot),
        Err(Rejection::NotDirectory)
    );
    assert_eq!(
        rename.decide_destination(
            &address,
            file,
            RenameDestination::Occupied(EntryType::Directory)
        ),
        Err(Rejection::IsDirectory)
    );
    Ok(())
}

#[test]
fn effective_suffix_controls_link_policy_even_after_expansion() -> TestResult {
    let path = PathExpr::parse("/a//link/")?;
    let stat = StatRequest::new(path.clone(), SymlinkAction::Preserve);
    let read = ReadRequest::new(path, ReadRange::FULL);
    let parent = StoragePath::parse("a")?;
    let link_address = parent.join(&EntryName::parse("link")?)?;
    let mut links = SymlinkTraversal::new(IntermediateSymlinks::Reject);
    assert_eq!(
        links.encounter(stat.lookup(), &[]),
        Ok(SymlinkAction::Preserve)
    );
    assert_eq!(
        stat.lookup()
            .check_existing_target(&link_address, EntryType::Symlink),
        Err(Rejection::NotDirectory)
    );

    assert_eq!(
        links.encounter(read.lookup(), &[]),
        Ok(SymlinkAction::FollowWithinInstance)
    );
    let target = prepare_link_target(
        &LinkTarget::new(b"/.././second//last/".to_vec()),
        &parent,
        &[],
    )?;
    assert_eq!(target.start(), LookupStart::InstanceRoot);
    assert_eq!(
        target.components()[..2],
        [
            PathComponent::ParentDirectory,
            PathComponent::CurrentDirectory
        ]
    );
    assert!(target.requires_directory());
    assert_eq!(
        links.encounter(read.lookup(), &target.components()[3..]),
        Err(Rejection::SymlinkNotAllowed)
    );
    assert_eq!(
        links.encounter(read.lookup(), &[]),
        Ok(SymlinkAction::FollowWithinInstance)
    );
    assert_eq!(
        read.lookup()
            .check_existing_target(&StoragePath::root(), EntryType::Directory),
        Err(Rejection::IsDirectory)
    );
    Ok(())
}

#[test]
fn rename_shares_an_expansion_budget_across_its_two_operands() -> TestResult {
    let rename = RenameRequest::new(
        PathExpr::parse("prefix/source")?,
        PathExpr::parse("/alias/destination")?,
        RenameMode::NoReplace,
    )?;
    let (source, destination) = rename.lookups();
    let mut links = SymlinkTraversal::default();
    let source_suffix = &source.path().components()[1..];
    let destination_suffix = &destination.path().components()[1..];
    for _ in 1..MAX_SYMLINK_EXPANSIONS {
        assert_eq!(
            links.encounter(source, source_suffix),
            Ok(SymlinkAction::FollowWithinInstance)
        );
    }
    assert_eq!(
        links.encounter(destination, destination_suffix),
        Ok(SymlinkAction::FollowWithinInstance)
    );
    assert_eq!(
        links.encounter(destination, destination_suffix),
        Err(Rejection::TooManySymlinks)
    );
    assert_eq!(
        links.encounter(destination, &[]),
        Ok(SymlinkAction::Preserve)
    );
    Ok(())
}

#[test]
fn link_target_bytes_are_lossless_and_following_checks_expanded_length() -> TestResult {
    let raw = b"a//\xff/../";
    let root = StoragePath::root();
    let unreadable = LinkTarget::new(raw.to_vec());
    assert_eq!(unreadable.as_bytes(), raw);
    assert_eq!(
        prepare_link_target(&unreadable, &root, &[]),
        Err(Failure::Backend(BackendFailure::UnrepresentableLinkTarget)),
    );
    assert_eq!(unreadable.into_bytes(), raw);
    for raw in [b"".as_slice(), b"name\0", b"a:b", b"a\\b", b"C:\\docs"] {
        assert_eq!(
            prepare_link_target(&LinkTarget::new(raw.to_vec()), &root, &[]),
            Err(Failure::Backend(BackendFailure::UnrepresentableLinkTarget))
        );
    }
    let raw = "/Report//../e\u{301}/";
    let link = LinkTarget::new(raw.as_bytes().to_vec());
    assert_eq!(link.as_bytes(), raw.as_bytes());
    assert_eq!(
        prepare_link_target(&link, &root, &[])?.to_string(),
        "/Report/../e\u{301}/"
    );

    let directory = StoragePath::parse(&"d".repeat(MAX_PATH_BYTES - 4))?;
    let target = LinkTarget::new(b"a/".to_vec());
    let suffix = PathExpr::parse("b")?;
    let prepared = prepare_link_target(&target, &directory, suffix.components())?;
    assert_eq!(prepared, PathExpr::parse("a/")?);
    assert_eq!(
        prepared.check_directory_requirement(false),
        Err(Rejection::NotDirectory)
    );
    assert_eq!(prepared.check_directory_requirement(true), Ok(()));
    let long_suffix = PathExpr::parse("b/c")?;
    let too_long = Err(Failure::Rejected(Rejection::PathTooLong));
    assert_eq!(
        prepare_link_target(&target, &directory, long_suffix.components()),
        too_long
    );
    let full = StoragePath::parse(&"d".repeat(MAX_PATH_BYTES))?;
    assert_eq!(
        prepare_link_target(&LinkTarget::new(b"..".to_vec()), &full, &[]),
        too_long
    );
    assert_eq!(
        prepare_link_target(
            &LinkTarget::new(b"/a/".to_vec()),
            &full,
            long_suffix.components()
        )?,
        PathExpr::parse("/a/")?
    );
    assert_eq!(
        prepare_link_target(
            &LinkTarget::new(vec![b'/'; MAX_PATH_EXPR_BYTES + 1]),
            &root,
            &[]
        ),
        too_long
    );
    Ok(())
}

#[test]
fn creation_policy_distinguishes_operand_names_from_dangling_targets() -> TestResult {
    let path = PathExpr::parse("prefix/../new/")?;
    let address = StoragePath::parse("prefix")?;
    let single = CreateDirectoryRequest::new(path.clone(), CreateDirectoryMode::Single);
    let recursive = CreateDirectoryRequest::new(path, CreateDirectoryMode::Recursive);
    for (request, intermediate) in [(&single, Err(Rejection::NotFound)), (&recursive, Ok(()))] {
        let lookup = request.lookup();
        assert_eq!(
            lookup.check_missing_name(&address, MissingName::OperandIntermediate),
            intermediate
        );
        assert_eq!(
            lookup.check_missing_name(&address, MissingName::OperandFinal),
            Ok(())
        );
        assert_eq!(
            lookup.check_missing_name(&address, MissingName::LinkTarget),
            Err(Rejection::NotFound)
        );
    }

    let read = ReadRequest::new(PathExpr::parse("prefix/../new/")?, ReadRange::FULL);
    assert_eq!(
        read.lookup()
            .check_missing_name(&address, MissingName::OperandIntermediate),
        Err(Rejection::NotFound)
    );
    assert_eq!(
        read.lookup()
            .check_missing_name(&address, MissingName::OperandFinal),
        Err(Rejection::NotFound)
    );
    Ok(())
}
