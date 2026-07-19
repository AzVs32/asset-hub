use super::*;
use crate::migration;

#[tokio::test]
async fn repository_roundtrips_http_and_cli_events_without_fabricating_an_actor() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migration::sqlite::run(&pool).await.unwrap();
    let repository = SqliteSecurityAuditRepository::new(pool);

    repository
        .record(&NewSecurityAuditEvent {
            actor: SecurityAuditActor::unauthenticated(),
            source: SecurityAuditSource::Http,
            event_type: SecurityAuditEventType::AuthLogin,
            outcome: SecurityAuditOutcome::Failure,
            target: Some("attempted-user".to_string()),
        })
        .await
        .unwrap();
    repository
        .record(&NewSecurityAuditEvent {
            actor: SecurityAuditActor::unauthenticated(),
            source: SecurityAuditSource::Cli,
            event_type: SecurityAuditEventType::MaintenanceStorageScan,
            outcome: SecurityAuditOutcome::Success,
            target: None,
        })
        .await
        .unwrap();

    let events = repository.list(10, 0).await.unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().any(|event| {
        event.event_type == SecurityAuditEventType::AuthLogin
            && event.actor == SecurityAuditActor::Unauthenticated
            && event.source == SecurityAuditSource::Http
            && event.outcome == SecurityAuditOutcome::Failure
            && event.target.as_deref() == Some("attempted-user")
    }));
    assert!(events.iter().any(|event| {
        event.event_type == SecurityAuditEventType::MaintenanceStorageScan
            && event.actor == SecurityAuditActor::Unauthenticated
            && event.source == SecurityAuditSource::Cli
            && event.outcome == SecurityAuditOutcome::Success
    }));
}
