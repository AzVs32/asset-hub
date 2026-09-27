use std::collections::{BTreeMap, BTreeSet};

use asset_vfs::entry::EntryKind;
use asset_vfs::namespace::VirtualRelativePath;
use asset_vfs_conformance::driver::data::{
    self, TreeBuilder, TreeEntry, TreeSpec, missing_child, root_path,
};

#[derive(Debug, PartialEq, Eq)]
enum StoredEntry {
    Directory,
    File(Vec<u8>),
}

#[derive(Default)]
struct RecordingBuilder {
    entries: BTreeMap<String, StoredEntry>,
    fail_at: Option<&'static str>,
}

impl RecordingBuilder {
    fn check_parent(&self, path: &VirtualRelativePath) -> Result<(), &'static str> {
        if path.is_empty() {
            return Ok(());
        }
        let parent = path
            .as_str()
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        match self.entries.get(parent) {
            Some(StoredEntry::Directory) => Ok(()),
            _ => Err("parent directory must be created first"),
        }
    }
}

impl TreeBuilder for RecordingBuilder {
    type Error = &'static str;

    fn create_directory(&mut self, path: &VirtualRelativePath) -> Result<(), Self::Error> {
        self.check_parent(path)?;
        self.entries
            .insert(path.as_str().to_owned(), StoredEntry::Directory);
        Ok(())
    }

    fn write_file(
        &mut self,
        path: &VirtualRelativePath,
        contents: &[u8],
    ) -> Result<(), Self::Error> {
        self.check_parent(path)?;
        if self.fail_at == Some(path.as_str()) {
            return Err("injected write failure");
        }
        self.entries.insert(
            path.as_str().to_owned(),
            StoredEntry::File(contents.to_vec()),
        );
        Ok(())
    }
}

#[test]
fn unordered_description_populates_parents_and_preserves_binary_contents() {
    let tree = TreeSpec::new(&[
        TreeEntry::File {
            path: "nested/deep/binary.dat",
            contents: b"\0\xffdata",
        },
        TreeEntry::Directory("nested/deep"),
        TreeEntry::Directory("empty"),
        TreeEntry::Directory("nested"),
    ]);
    let mut builder = RecordingBuilder::default();
    tree.populate(&mut builder).unwrap();
    assert_eq!(builder.entries.get(""), Some(&StoredEntry::Directory));
    assert_eq!(builder.entries.get("empty"), Some(&StoredEntry::Directory));
    assert_eq!(
        builder.entries.get("nested/deep/binary.dat"),
        Some(&StoredEntry::File(b"\0\xffdata".to_vec()))
    );
    let root = tree.listing(&root_path());
    let names: Vec<_> = root.iter().map(|entry| entry.name().as_str()).collect();
    assert_eq!(names, ["empty", "nested"]);
    let directory = tree
        .entries()
        .iter()
        .find(|entry| entry.path().as_str() == "nested/deep")
        .unwrap()
        .path();
    let nested = tree.listing(&directory);
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].name().as_str(), "binary.dat");
    assert_eq!(nested[0].kind(), EntryKind::File);
    assert_eq!(nested[0].size(), Some(6));
}

#[test]
fn population_preserves_builder_errors_and_stops_before_later_files() {
    let tree = TreeSpec::new(&[
        TreeEntry::File {
            path: "stop.txt",
            contents: b"",
        },
        TreeEntry::File {
            path: "later.txt",
            contents: b"",
        },
    ]);
    let mut builder = RecordingBuilder {
        fail_at: Some("stop.txt"),
        ..RecordingBuilder::default()
    };
    assert_eq!(tree.populate(&mut builder), Err("injected write failure"));
    assert!(!builder.entries.contains_key("stop.txt"));
    assert!(!builder.entries.contains_key("later.txt"));
}

#[test]
fn standard_trees_have_unique_valid_paths_explicit_parents_and_absent_missing_children() {
    for tree in [
        data::BINDING_TREE,
        data::BOUND_ROOT_TREE,
        data::BOUND_OTHER_TREE,
        data::READ_TREE,
    ] {
        let mut paths = BTreeSet::new();
        let directories: BTreeSet<_> = tree
            .entries()
            .iter()
            .filter(|entry| entry.kind() == EntryKind::Directory)
            .map(|entry| entry.path())
            .collect();
        for entry in tree.entries() {
            let path = entry.path();
            assert!(paths.insert(path.clone()), "duplicate path {path}");
            if let Some((parent, _)) = path.as_str().rsplit_once('/') {
                assert!(
                    directories
                        .iter()
                        .any(|directory| directory.as_str() == parent),
                    "missing parent for {path}"
                );
            }
        }
        for directory in std::iter::once(root_path()).chain(directories) {
            assert!(
                !paths.contains(&missing_child(&directory)),
                "reserved missing child exists"
            );
        }
        let mut builder = RecordingBuilder::default();
        tree.populate(&mut builder).unwrap();
    }
}

#[test]
fn standard_data_keeps_each_required_scenario() {
    let tree = data::READ_TREE;
    assert!(tree.listing(&root_path()).len() >= 2);
    assert!(
        tree.entries()
            .iter()
            .any(|entry| matches!(entry, TreeEntry::File { contents, .. } if contents.is_empty()))
    );
    assert!(
        tree.entries()
            .iter()
            .any(|entry| matches!(entry, TreeEntry::File { contents, .. } if !contents.is_empty()))
    );
    assert!(tree.entries().iter().any(
        |entry| entry.kind() == EntryKind::Directory && tree.listing(&entry.path()).is_empty()
    ));
    assert!(tree.entries().iter().any(
        |entry| entry.kind() == EntryKind::Directory && !tree.listing(&entry.path()).is_empty()
    ));
    assert!(
        tree.entries()
            .iter()
            .any(|entry| !entry.path().as_str().is_ascii())
    );
    assert!(tree.entries().iter().any(|entry| entry.path().depth() > 1));
    assert!(
        data::BINDING_TREE
            .entries()
            .iter()
            .any(|entry| entry.kind() == EntryKind::File)
    );
    let root = data::BOUND_ROOT_TREE.listing(&root_path());
    let other = data::BOUND_OTHER_TREE.listing(&root_path());
    assert!(!root.is_empty() && !other.is_empty());
    assert!(
        root.iter()
            .all(|entry| other.iter().all(|other| entry.name() != other.name()))
    );
}
