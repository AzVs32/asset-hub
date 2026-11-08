//! Exercise the executor-facing rules in a single, isolated namespace.
//! This fixture supplies observations and walk order. The domain API decides
//! link policy, missing-name admission, target shape and rename outcomes.
use ah_vfs_storage::*;
use std::{collections::BTreeMap, error::Error};

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
    nodes: BTreeMap<StoragePath, Node>,
}

struct Selected {
    address: StoragePath,
    kind: Option<EntryType>,
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

#[test]
fn native_equivalence_does_not_authorize_a_different_spelling() -> TestResult {
    // Model one case-preserving native slot with ASCII-insensitive lookup.
    // The adapter checks its authoritative spelling before using that slot.
    let native = b"Report";
    let stored = EntryName::from_backend_bytes(native)?;
    let access = |requested: &EntryName, may_create: bool| {
        if requested == &stored {
            Ok(())
        } else if may_create && requested.as_str().as_bytes().eq_ignore_ascii_case(native) {
            Err(Rejection::Conflict)
        } else {
            Err(Rejection::NotFound)
        }
    };
    assert_eq!(access(&EntryName::parse("Report")?, false), Ok(()));
    assert_eq!(
        access(&EntryName::parse("report")?, false),
        Err(Rejection::NotFound)
    );
    assert_eq!(
        access(&EntryName::parse("report")?, true),
        Err(Rejection::Conflict)
    );
    Ok(())
}

#[test]
fn native_name_limits_and_logical_address_limits_have_distinct_failures() -> TestResult {
    // Model a backend with a 255-byte component limit and a reserved name.
    let validate_slot = |parent: &StoragePath, name: &EntryName| {
        let child = parent.join(name).map_err(|_| Rejection::PathTooLong)?;
        match name.as_str() {
            text if text.len() > 255 => Err(Rejection::NameTooLong),
            "reserved" => Err(Rejection::UnsupportedName),
            _ => Ok(child),
        }
    };
    let root = StoragePath::root();
    let max_native = "文".repeat(85);
    let name = EntryName::parse(&max_native)?;
    assert_eq!(validate_slot(&root, &name)?.as_str(), max_native);
    let longer = EntryName::parse(&format!("{max_native}a"))?;
    assert_eq!(validate_slot(&root, &longer), Err(Rejection::NameTooLong));
    assert_eq!(
        validate_slot(&root, &EntryName::parse("reserved")?),
        Err(Rejection::UnsupportedName)
    );

    let segment = "a".repeat(255);
    let parent = StoragePath::parse(&[segment.as_str(); 15].join("/"))?;
    assert_eq!(
        validate_slot(&parent, &name)?.as_str().len(),
        MAX_PATH_BYTES - 1
    );
    let parent = parent.join(&EntryName::parse("a")?)?;
    assert_eq!(validate_slot(&parent, &name), Err(Rejection::PathTooLong));
    assert_eq!(validate_slot(&parent, &longer), Err(Rejection::PathTooLong));
    Ok(())
}

impl Namespace {
    fn new(entries: impl IntoIterator<Item = (&'static str, Node)>) -> TestResult<Self> {
        let mut nodes = BTreeMap::from([(StoragePath::root(), Node::Directory)]);
        for (path, node) in entries {
            nodes.insert(StoragePath::parse(path)?, node);
        }
        Ok(Self { nodes })
    }

    fn walk(
        &mut self,
        lookup: PathLookup<'_>,
        cwd: &StoragePath,
        links: &mut SymlinkTraversal,
        create_directories: bool,
    ) -> Result<Selected, Failure> {
        let mut walk = Walk {
            nodes: &mut self.nodes,
            lookup,
            links,
            create_directories,
        };
        walk.segment(lookup.path(), cwd, &[], false)
    }

    fn select(
        &mut self,
        lookup: PathLookup<'_>,
        cwd: &StoragePath,
        links: IntermediateSymlinks,
    ) -> Result<StoragePath, Failure> {
        let selected = self.walk(lookup, cwd, &mut SymlinkTraversal::new(links), false)?;
        let kind = selected
            .kind
            .ok_or(Failure::Rejected(Rejection::NotFound))?;
        lookup
            .check_existing_target(&selected.address, kind)
            .map_err(Failure::Rejected)?;
        Ok(selected.address)
    }

    fn mkdir_recursive(
        &mut self,
        request: &CreateDirectoryRequest,
    ) -> Result<StoragePath, StorageError> {
        let count_before = self.nodes.len();
        let lookup = request.lookup();
        let result = self
            .walk(
                lookup,
                &StoragePath::root(),
                &mut SymlinkTraversal::default(),
                true,
            )
            .and_then(|selected| {
                let kind = selected
                    .kind
                    .ok_or(Failure::Rejected(Rejection::NotFound))?;
                lookup
                    .check_existing_target(&selected.address, kind)
                    .map_err(Failure::Rejected)?;
                Ok(selected.address)
            });
        result.map_err(|failure| {
            // Only this fixture mutates this map, and mkdir can only insert.
            if self.nodes.len() == count_before {
                StorageError::without_effects(Operation::CreateDirectory, failure, None)
            } else {
                StorageError::after_dispatch(Operation::CreateDirectory, failure, None)
            }
        })
    }

    fn rename_decision(
        &mut self,
        request: &RenameRequest,
        cwd: &StoragePath,
    ) -> Result<RenameDecision, Failure> {
        let (source, destination) = request.lookups();
        let mut links = SymlinkTraversal::default();
        let src = self.walk(source, cwd, &mut links, false)?;
        let kind = src.kind.ok_or(Failure::Rejected(Rejection::NotFound))?;
        source
            .check_existing_target(&src.address, kind)
            .map_err(Failure::Rejected)?;
        let dst = self.walk(destination, cwd, &mut links, false)?;

        // Addresses uniquely identify slots in this fixture. Aliases have
        // already been walked, and the namespace cannot change concurrently.
        let observation = match dst.kind {
            None => RenameDestination::Missing,
            Some(_) if src.address == dst.address => RenameDestination::SameSlot,
            Some(EntryType::Directory)
                if self
                    .nodes
                    .keys()
                    .any(|child| child.parent().as_ref() == Some(&dst.address)) =>
            {
                RenameDestination::NonEmptyDirectory
            }
            Some(kind) => RenameDestination::Occupied(kind),
        };
        request
            .decide_destination(&dst.address, kind, observation)
            .map_err(Failure::Rejected)
    }
}

struct Walk<'a, 'request> {
    nodes: &'a mut BTreeMap<StoragePath, Node>,
    lookup: PathLookup<'request>,
    links: &'a mut SymlinkTraversal,
    create_directories: bool,
}

impl Walk<'_, '_> {
    fn segment(
        &mut self,
        path: &PathExpr,
        base: &StoragePath,
        suffix: &[PathComponent],
        in_target: bool,
    ) -> Result<Selected, Failure> {
        let mut address = match path.start() {
            LookupStart::InstanceRoot => StoragePath::root(),
            LookupStart::BaseDirectory => base.clone(),
        };
        for (i, component) in path.components().iter().enumerate() {
            if !matches!(self.nodes.get(&address), Some(Node::Directory)) {
                return Err(Failure::Rejected(Rejection::NotDirectory));
            }
            match component {
                PathComponent::CurrentDirectory => {}
                PathComponent::ParentDirectory => {
                    address = address.parent().unwrap_or_else(StoragePath::root);
                }
                PathComponent::Name(name) => {
                    let child = address
                        .join(name)
                        .map_err(|_| Failure::Rejected(Rejection::PathTooLong))?;
                    let remaining = &path.components()[i + 1..];
                    match self.nodes.get(&child) {
                        Some(Node::Link(target)) => {
                            let pending: Vec<_> = remaining.iter().chain(suffix).cloned().collect();
                            match self
                                .links
                                .encounter(self.lookup, &pending)
                                .map_err(Failure::Rejected)?
                            {
                                SymlinkAction::Preserve => address = child,
                                SymlinkAction::FollowWithinInstance => {
                                    let segment = target.prepare_lookup(&address, &pending)?;
                                    address =
                                        self.segment(&segment, &address, &pending, true)?.address;
                                }
                            }
                        }
                        Some(_) => address = child,
                        None => {
                            let origin = if in_target {
                                MissingName::LinkTarget
                            } else if remaining.is_empty() && suffix.is_empty() {
                                MissingName::OperandFinal
                            } else {
                                MissingName::OperandIntermediate
                            };
                            self.lookup
                                .check_missing_name(&child, origin)
                                .map_err(Failure::Rejected)?;
                            if !self.create_directories {
                                return Ok(Selected {
                                    address: child,
                                    kind: None,
                                });
                            }
                            self.nodes.insert(child.clone(), Node::Directory);
                            address = child;
                        }
                    }
                }
            }
        }
        let kind = self.nodes.get(&address).map(Node::kind);
        path.check_directory_requirement(kind == Some(EntryType::Directory))
            .map_err(Failure::Rejected)?;
        Ok(Selected { address, kind })
    }
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
        let read_link = ReadLinkRequest::new(PathExpr::parse(path)?);
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
            fs.nodes.get(&StoragePath::parse("created")?),
            Some(Node::Directory)
        ));
        assert!(!fs.nodes.contains_key(&StoragePath::parse("file/child")?));
    }
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
