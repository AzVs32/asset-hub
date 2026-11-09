use ah_vfs_storage::{EntryName, PathError, StoragePath};
use std::error::Error;

#[test]
fn root_and_relative_names_preserve_their_identity() -> Result<(), Box<dyn Error>> {
    let root = StoragePath::root();
    assert_eq!(StoragePath::parse("")?, root);
    assert!(root.is_root());
    assert_eq!(root.name(), None);
    assert_eq!(root.parent(), None);
    assert_eq!(root.entry_names().count(), 0);

    let path = root
        .join(&EntryName::parse("目录")?)?
        .join(&EntryName::parse("file name.txt")?)?;
    assert_eq!(path.as_str(), "目录/file name.txt");
    assert_eq!(
        path.name().as_ref().map(EntryName::as_str),
        Some("file name.txt")
    );
    assert_eq!(path.parent(), Some(StoragePath::parse("目录")?));
    assert_eq!(StoragePath::parse("目录")?.parent(), Some(root));
    assert_eq!(
        path.entry_names().collect::<Vec<_>>(),
        [
            EntryName::parse("目录")?,
            EntryName::parse("file name.txt")?
        ]
    );
    assert_eq!(path.to_string(), "目录/file name.txt");
    assert_eq!(path.as_str().parse::<StoragePath>()?, path);
    assert_eq!(StoragePath::try_from(path.to_string())?, path);
    Ok(())
}

#[test]
fn every_input_entry_rejects_ambiguous_and_escaping_paths() {
    for input in [
        "/",
        "/absolute",
        "a/",
        "a//b",
        ".",
        "..",
        "a/./b",
        "a/../b",
        "../b",
        "C:/file",
        "C:file",
        "a:b",
        "a\\b",
        "\\\\server\\share",
        "a\0b",
    ] {
        assert!(StoragePath::parse(input).is_err(), "accepted {input:?}");
        assert!(input.parse::<StoragePath>().is_err(), "accepted {input:?}");
        assert!(StoragePath::try_from(input.to_owned()).is_err());
    }
    for name in ["", ".", "..", "/a", "a/b", "a\\b", "C:file", "a\0b"] {
        assert!(EntryName::parse(name).is_err());
    }
}

#[test]
fn length_limit_counts_utf8_bytes_and_applies_to_join() -> Result<(), Box<dyn Error>> {
    let maximum = "a".repeat(ah_vfs_storage::MAX_PATH_BYTES);
    assert_eq!(StoragePath::parse(&maximum)?.as_str(), maximum);
    assert_eq!(
        StoragePath::parse(&format!("{maximum}a")),
        Err(PathError::TooLong)
    );
    assert_eq!(
        StoragePath::parse(&"文".repeat(ah_vfs_storage::MAX_PATH_BYTES / 3 + 1)),
        Err(PathError::TooLong)
    );
    assert_eq!(
        StoragePath::parse(&maximum)?.join(&EntryName::parse("b")?),
        Err(PathError::TooLong)
    );
    Ok(())
}
