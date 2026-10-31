use ah_vfs::{
    application::dto::{
        DEFAULT_DIR_PAGE_LIMIT, DirQuery, MAX_DIR_PAGE_LIMIT, StorageDirQuery,
        validate_dir_page_limit,
    },
    domain::{StorageErrorKind, StoragePath},
};
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
            after: Some("文件 name".into()),
            limit,
        }
        .validate()?;
    }
    let limit = NonZeroUsize::new(MAX_DIR_PAGE_LIMIT.get() + 1).ok_or("zero limit")?;
    let query = StorageDirQuery {
        path: StoragePath::root(),
        after: Some("../invalid".into()),
        limit,
    };
    assert_eq!(
        query.validate().err().ok_or("expected limit error")?.kind(),
        StorageErrorKind::InvalidRequest
    );
    Ok(())
}

#[test]
fn invalid_cursors_retain_path_errors_and_full_address_bounds() -> TestResult {
    for name in ["", ".", "..", "a/b", "a\\b", "a:b", "a\0b"] {
        let query = StorageDirQuery {
            path: StoragePath::root(),
            after: Some(name.into()),
            limit: DEFAULT_DIR_PAGE_LIMIT,
        };
        let error = query.validate().err().ok_or("expected path error")?;
        assert_eq!(error.kind(), StorageErrorKind::InvalidPath);
        assert!(error.source().is_some());
    }
    let query = StorageDirQuery {
        path: StoragePath::parse(&"x".repeat(StoragePath::MAX_LEN))?,
        after: Some("child".into()),
        limit: DEFAULT_DIR_PAGE_LIMIT,
    };
    assert_eq!(
        query
            .validate()
            .err()
            .ok_or("expected path overflow")?
            .kind(),
        StorageErrorKind::InvalidPath
    );
    Ok(())
}
