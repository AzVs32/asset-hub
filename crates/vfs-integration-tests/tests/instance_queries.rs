mod common;

use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        dto::{InstanceInfo, InstanceQuery},
        port::Driver,
    },
    domain::{DriverError, InstanceId, StorageErrorKind},
};
use common::*;
use std::{error::Error, num::NonZeroUsize, sync::Arc};
use vfs_driver_memory::MemoryDriver;

struct Fixture {
    driver: Arc<dyn Driver>,
    pool: sqlx::SqlitePool,
    table: &'static str,
    root: tempfile::TempDir,
}

impl Fixture {
    async fn new(memory: bool) -> TestResult<Self> {
        let pool = pool().await?;
        let root = tempfile::tempdir()?;
        let (driver, table): (Arc<dyn Driver>, _) = if memory {
            (
                Arc::new(MemoryDriver::from_pool(pool.clone()).await?),
                "memory_instances",
            )
        } else {
            (
                Arc::new(vfs_driver_local::LocalDriver::from_pool(pool.clone()).await?),
                "local_instances",
            )
        };
        let mut catalog = DriverCatalog::new();
        ah_vfs::testing::register_driver(&mut catalog, driver.clone())?;
        let driver = catalog
            .get(driver.info().key())
            .ok_or("driver missing")?
            .clone();
        Ok(Self {
            driver,
            pool,
            table,
            root,
        })
    }

    async fn insert(&self, id: &str) -> TestResult {
        if self.table == "memory_instances" {
            sqlx::query("INSERT INTO memory_instances VALUES (?, 128, 'KiB')")
                .bind(id)
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("INSERT INTO local_instances VALUES (?, ?)")
                .bind(id)
                .bind(
                    std::fs::canonicalize(self.root.path())?
                        .to_str()
                        .ok_or("UTF-8")?,
                )
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn seed(&self) -> TestResult<Vec<String>> {
        let mut ids: Vec<String> = ["z", "A", "a", "é", "e\u{301}", "文档", "with'quote", "000"]
            .into_iter()
            .map(String::from)
            .collect();
        for id in &ids {
            self.insert(id).await?;
        }
        ids.sort();
        Ok(ids)
    }
}

fn assert_storage_error<T>(result: Result<T, DriverError>, kind: StorageErrorKind) -> TestResult {
    let failure = result.err().ok_or("expected a domain failure")?;
    let DriverError::Storage(error) = failure else {
        return Err("wrong failure category".into());
    };
    assert_eq!(error.kind(), kind);
    assert!(error.source().is_some());
    Ok(())
}

#[test]
fn query_bounds_are_validated_before_any_driver_call() -> TestResult {
    let default = InstanceQuery::default();
    assert_eq!(default.limit().get(), 100);
    assert!(default.after().is_none());
    for size in [1, InstanceQuery::MAX_LIMIT] {
        assert_eq!(
            InstanceQuery::new(NonZeroUsize::new(size).ok_or("limit")?)?
                .limit()
                .get(),
            size
        );
    }
    let failure =
        InstanceQuery::new(NonZeroUsize::new(InstanceQuery::MAX_LIMIT + 1).ok_or("limit")?)
            .err()
            .ok_or("expected invalid query")?;
    assert!(
        matches!(failure, DriverError::Storage(error) if error.kind() == StorageErrorKind::InvalidRequest)
    );
    Ok(())
}

#[tokio::test]
async fn get_and_list_are_available_through_catalog_trait_objects() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        assert!(
            f.driver
                .instances()
                .registry()?
                .get(InstanceId::parse("missing")?)
                .await?
                .is_none()
        );
        let empty = f
            .driver
            .instances()
            .registry()?
            .list(InstanceQuery::default())
            .await?;
        assert!(empty.instances.is_empty());
        assert!(empty.next_after.is_none());
        f.insert("instance").await?;
        let info = f
            .driver
            .instances()
            .registry()?
            .get(InstanceId::parse("instance")?)
            .await?
            .ok_or("instance missing")?;
        assert_eq!(info.id.as_str(), "instance");
        if memory {
            assert_eq!(info.details.len(), 2);
            assert_eq!(info.details["capacity_size"].as_u64(), Some(128));
            assert_eq!(info.details["capacity_unit"].as_str(), Some("KiB"));
        } else {
            assert_eq!(info.details.len(), 1);
            assert_eq!(
                info.details["root_path"].as_str(),
                std::fs::canonicalize(f.root.path())?.to_str()
            );
        }
        let page = f
            .driver
            .instances()
            .registry()?
            .list(InstanceQuery::default())
            .await?;
        assert_eq!(page.instances, vec![info]);
        assert!(page.next_after.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn pages_use_exclusive_binary_id_boundaries_without_gaps_or_duplicates() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        let expected = f.seed().await?;
        for size in [1, 2, 3, 1000] {
            let mut query = InstanceQuery::new(NonZeroUsize::new(size).ok_or("limit")?)?;
            let mut actual = Vec::new();
            loop {
                let page = f.driver.instances().registry()?.list(query).await?;
                assert!(page.instances.len() <= size);
                for info in &page.instances {
                    assert_eq!(
                        f.driver
                            .instances()
                            .registry()?
                            .get(info.id.clone())
                            .await?,
                        Some(info.clone())
                    );
                    actual.push(info.id.as_str().to_owned());
                }
                match page.next_after {
                    Some(after) => {
                        assert_eq!(page.instances.last().map(|info| &info.id), Some(&after));
                        assert!(actual.len() < expected.len());
                        query = InstanceQuery::new(NonZeroUsize::new(size).ok_or("limit")?)?
                            .with_after(after);
                    }
                    None => break,
                }
            }
            assert_eq!(actual, expected);
        }
    }
    Ok(())
}

#[tokio::test]
async fn cursor_can_be_missing_deleted_or_contain_quotes() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        let expected = f.seed().await?;
        let cursor = "a";
        let sql = if memory {
            "DELETE FROM memory_instances WHERE id = ?"
        } else {
            "DELETE FROM local_instances WHERE id = ?"
        };
        sqlx::query(sql).bind(cursor).execute(&f.pool).await?;
        for after in [cursor, "not-an-existing-id", "with'quote", "zzz"] {
            let page = f
                .driver
                .instances()
                .registry()?
                .list(InstanceQuery::default().with_after(InstanceId::parse(after)?))
                .await?;
            let actual: Vec<_> = page.instances.iter().map(|info| info.id.as_str()).collect();
            let expected: Vec<_> = expected
                .iter()
                .map(String::as_str)
                .filter(|id| *id != cursor && *id > after)
                .collect();
            assert_eq!(actual, expected);
            assert!(page.next_after.is_none());
        }
    }
    Ok(())
}

#[tokio::test]
async fn queries_do_not_open_an_unavailable_local_root() -> TestResult {
    let f = Fixture::new(false).await?;
    f.insert("instance").await?;
    let expected: InstanceInfo = f
        .driver
        .instances()
        .registry()?
        .get(InstanceId::parse("instance")?)
        .await?
        .ok_or("instance")?;
    std::fs::remove_dir(f.root.path())?;
    assert_eq!(
        f.driver
            .instances()
            .registry()?
            .get(expected.id.clone())
            .await?,
        Some(expected.clone())
    );
    assert_eq!(
        f.driver
            .instances()
            .registry()?
            .list(InstanceQuery::default())
            .await?
            .instances,
        vec![expected.clone()]
    );
    assert!(
        matches!(f.driver.open(expected.id.into()).await, Err(DriverError::Storage(error)) if error.kind() == StorageErrorKind::NotFound)
    );
    Ok(())
}

#[tokio::test]
async fn persistence_failures_are_errors_instead_of_missing_or_empty_results() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        f.insert("instance").await?;
        f.pool.close().await;
        assert_storage_error(
            f.driver
                .instances()
                .registry()?
                .get(InstanceId::parse("instance")?)
                .await,
            StorageErrorKind::Unavailable,
        )?;
        assert_storage_error(
            f.driver
                .instances()
                .registry()?
                .list(InstanceQuery::default())
                .await,
            StorageErrorKind::Unavailable,
        )?;
    }
    Ok(())
}

#[tokio::test]
async fn corrupt_fetched_records_fail_queries_without_deleting_or_skipping_them() -> TestResult {
    for memory in [false, true] {
        let f = Fixture::new(memory).await?;
        f.insert("instance").await?;
        if memory {
            sqlx::query("UPDATE memory_instances SET capacity_size = ?, capacity_unit = 'TiB'")
                .bind(i64::MAX)
                .execute(&f.pool)
                .await?;
        } else {
            sqlx::query("UPDATE local_instances SET root_path = 'relative'")
                .execute(&f.pool)
                .await?;
        }
        assert_storage_error(
            f.driver
                .instances()
                .registry()?
                .get(InstanceId::parse("instance")?)
                .await,
            StorageErrorKind::InvalidData,
        )?;
        assert_storage_error(
            f.driver
                .instances()
                .registry()?
                .list(InstanceQuery::default())
                .await,
            StorageErrorKind::InvalidData,
        )?;
        let sql = if memory {
            "SELECT COUNT(*) FROM memory_instances"
        } else {
            "SELECT COUNT(*) FROM local_instances"
        };
        let count: i64 = sqlx::query_scalar(sql).fetch_one(&f.pool).await?;
        assert_eq!(count, 1);
    }
    Ok(())
}

#[tokio::test]
async fn instance_ids_are_scoped_to_the_queried_driver() -> TestResult {
    let local = Fixture::new(false).await?;
    let memory = Fixture::new(true).await?;
    local.insert("same-id").await?;
    memory.insert("same-id").await?;
    let id = InstanceId::parse("same-id")?;
    let local_info = local
        .driver
        .instances()
        .registry()?
        .get(id.clone())
        .await?
        .ok_or("local")?;
    let memory_info = memory
        .driver
        .instances()
        .registry()?
        .get(id)
        .await?
        .ok_or("memory")?;
    assert_eq!(local_info.id, memory_info.id);
    assert_ne!(local_info.details, memory_info.details);
    assert_ne!(local.driver.info().key(), memory.driver.info().key());
    Ok(())
}
