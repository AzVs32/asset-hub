//! Name admission for an adapter exposing an exact logical namespace.
use ah_vfs_storage::*;
use std::error::Error;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

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
