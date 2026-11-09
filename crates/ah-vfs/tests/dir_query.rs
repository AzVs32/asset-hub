use ah_vfs::application::dto::DirQuery;
use ah_vfs::domain::storage::{
    DEFAULT_DIR_PAGE_LIMIT, MAX_DIR_PAGE_LIMIT, StorageDirQuery, validate_dir_page_limit,
};
use ah_vfs_storage::{EntryName, StoragePath};
use ah_vfs_storage::{Failure, Rejection};
use std::{error::Error, num::NonZeroUsize};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn namespace_and_storage_queries_share_defaults_and_bounds() -> TestResult {
    assert_eq!(DEFAULT_DIR_PAGE_LIMIT.get(), 100);
    assert_eq!(MAX_DIR_PAGE_LIMIT.get(), 1000);
    assert_eq!(DirQuery::default().limit, DEFAULT_DIR_PAGE_LIMIT);
    assert_eq!(
        StorageDirQuery::new(StoragePath::root()).limit,
        DEFAULT_DIR_PAGE_LIMIT
    );
    for limit in [
        NonZeroUsize::MIN,
        DEFAULT_DIR_PAGE_LIMIT,
        MAX_DIR_PAGE_LIMIT,
    ] {
        validate_dir_page_limit(limit)?;
        StorageDirQuery {
            path: StoragePath::root(),
            after: Some(EntryName::parse("文件 name")?),
            limit,
        }
        .validate()?;
    }
    let limit = NonZeroUsize::new(MAX_DIR_PAGE_LIMIT.get() + 1).ok_or("zero limit")?;
    let query = StorageDirQuery {
        path: StoragePath::root(),
        after: None,
        limit,
    };
    assert_eq!(
        query
            .validate()
            .err()
            .ok_or("expected limit error")?
            .failure(),
        Failure::Rejected(Rejection::LimitExceeded)
    );
    Ok(())
}

#[test]
fn invalid_cursors_retain_path_errors_and_full_address_bounds() -> TestResult {
    for name in ["", ".", "..", "a/b", "a\\b", "a:b", "a\0b"] {
        assert!(EntryName::parse(name).is_err());
    }
    let query = StorageDirQuery {
        path: StoragePath::parse(&"x".repeat(ah_vfs_storage::MAX_PATH_BYTES))?,
        after: Some(EntryName::parse("child")?),
        limit: DEFAULT_DIR_PAGE_LIMIT,
    };
    assert_eq!(
        query
            .validate()
            .err()
            .ok_or("expected path overflow")?
            .failure(),
        Failure::Rejected(Rejection::PathTooLong)
    );
    Ok(())
}
