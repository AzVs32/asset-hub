use super::*;
use crate::sqlite::{SqliteDatabase, SqliteUploadSessionRepository};
use asset_core::domain::{Checksum, DirectoryId, IdempotencyKey, IdempotencyRecord, UploadSession};
use asset_core::port::{IdempotencyAcquire, IdempotencyRepository, UploadSessionRepository};
use asset_core::service::{IdempotencyOutcome, IdempotencyService};
use chrono::{Duration, Utc};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::test]
async fn sqlite_leases_preserve_replay_and_reject_active_or_different_requests() {
    let repository = repository("idempotency-active").await;
    let service = IdempotencyService::new(repository);
    let key = key("active");

    let execution_id = acquire(&service, &key, "hash-a").await;
    assert!(matches!(
        service.begin(&key, "hash-a").await.unwrap(),
        IdempotencyOutcome::AlreadyInProgress
    ));
    assert!(matches!(
        service.begin(&key, "hash-b").await.unwrap(),
        IdempotencyOutcome::ConflictDifferentRequest
    ));

    service
        .complete(&key, execution_id, serde_json::json!({ "result": 1 }))
        .await
        .unwrap();
    assert!(matches!(
        service.begin(&key, "hash-a").await.unwrap(),
        IdempotencyOutcome::Replay(result) if result == serde_json::json!({ "result": 1 })
    ));
    assert!(matches!(
        service.begin(&key, "hash-b").await.unwrap(),
        IdempotencyOutcome::ConflictDifferentRequest
    ));
}

#[tokio::test]
async fn sqlite_expired_lease_can_be_taken_over_but_stale_owner_cannot_mutate_it() {
    let repository = repository("idempotency-takeover").await;
    let key = key("takeover");
    let expired = IdempotencyRecord::new(
        key.clone(),
        "hash".to_string(),
        Utc::now() - Duration::seconds(1),
    );
    let old_execution_id = expired.execution_id();
    assert!(matches!(
        repository.acquire(&expired).await.unwrap(),
        IdempotencyAcquire::Acquired
    ));

    let service = IdempotencyService::new(repository.clone());
    let new_execution_id = acquire(&service, &key, "hash").await;
    assert_ne!(old_execution_id, new_execution_id);
    assert!(matches!(
        service
            .complete(&key, old_execution_id, serde_json::json!({ "stale": true }))
            .await,
        Err(CoreError::LostIdempotencyLease { .. })
    ));
    assert!(matches!(
        service.abandon(&key, old_execution_id).await,
        Err(CoreError::LostIdempotencyLease { .. })
    ));

    service
        .complete(&key, new_execution_id, serde_json::json!({ "fresh": true }))
        .await
        .unwrap();
    assert!(matches!(
        service.begin(&key, "hash").await.unwrap(),
        IdempotencyOutcome::Replay(result) if result == serde_json::json!({ "fresh": true })
    ));
}

#[tokio::test]
async fn sqlite_conditional_takeover_grants_only_one_expired_lease_owner() {
    let repository = repository("idempotency-race").await;
    let key = key("race");
    let expired = IdempotencyRecord::new(
        key.clone(),
        "hash".to_string(),
        Utc::now() - Duration::seconds(1),
    );
    repository.acquire(&expired).await.unwrap();

    let first = IdempotencyRecord::new(
        key.clone(),
        "hash".to_string(),
        Utc::now() + Duration::minutes(1),
    );
    let second = IdempotencyRecord::new(key, "hash".to_string(), Utc::now() + Duration::minutes(1));
    let (left, right) = tokio::join!(repository.acquire(&first), repository.acquire(&second));
    let acquired = [left.unwrap(), right.unwrap()]
        .into_iter()
        .filter(|outcome| matches!(outcome, IdempotencyAcquire::Acquired))
        .count();
    assert_eq!(acquired, 1);
}

#[tokio::test]
async fn upload_creation_key_recovers_the_durable_session_after_lease_takeover() {
    let path = unique_temp_path("idempotency-upload-link").join("asset-hub.sqlite");
    let database = SqliteDatabase::connect(&path, 1).await.unwrap();
    let uploads = SqliteUploadSessionRepository::new(database.pool().clone());
    let key = key("upload-link");
    let session = UploadSession::new(
        "document.txt",
        DirectoryId::root(),
        Some("text/plain".to_string()),
        0,
        Checksum::sha256("0".repeat(64)).unwrap(),
    )
    .unwrap();

    uploads
        .save_with_idempotency_key(&session, &key)
        .await
        .unwrap();
    assert_eq!(
        uploads
            .find_by_idempotency_key(&key)
            .await
            .unwrap()
            .unwrap()
            .id(),
        session.id()
    );
}

async fn acquire(
    service: &IdempotencyService,
    key: &IdempotencyKey,
    hash: &str,
) -> asset_core::domain::IdempotencyExecutionId {
    match service.begin(key, hash).await.unwrap() {
        IdempotencyOutcome::Acquired { execution_id } => execution_id,
        _ => panic!("expected idempotency execution lease"),
    }
}

async fn repository(name: &str) -> Arc<SqliteIdempotencyRepository> {
    let path = unique_temp_path(name).join("asset-hub.sqlite");
    let database = SqliteDatabase::connect(&path, 4).await.unwrap();
    Arc::new(SqliteIdempotencyRepository::new(database.pool().clone()))
}

fn key(name: &str) -> IdempotencyKey {
    IdempotencyKey::new(name).unwrap()
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
}
