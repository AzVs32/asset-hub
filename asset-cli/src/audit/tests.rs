use super::*;
use asset_core::domain::{SecurityAuditEvent, SecurityAuditSource};
use std::sync::Mutex;

#[derive(Default)]
struct RecordingRepository {
    events: Mutex<Vec<NewSecurityAuditEvent>>,
    reject_writes: bool,
}

#[async_trait::async_trait]
impl SecurityAuditRepository for RecordingRepository {
    async fn record(&self, event: &NewSecurityAuditEvent) -> Result<(), CoreError> {
        if self.reject_writes {
            return Err(CoreError::repository(
                "security_audit.test",
                std::io::Error::other("audit unavailable"),
            ));
        }
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }

    async fn list(&self, _limit: u32, _offset: u64) -> Result<Vec<SecurityAuditEvent>, CoreError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn records_cli_source_target_and_operation_outcome() {
    let repository = RecordingRepository::default();

    let value = audited(
        &repository,
        SecurityAuditEventType::AuthUserStatus,
        Some(" alice "),
        async { Ok(42) },
    )
    .await
    .unwrap();
    assert_eq!(value, 42);

    let error = audited::<()>(
        &repository,
        SecurityAuditEventType::ResourceScan,
        None,
        async { Err(CoreError::configuration("scan failed")) },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("scan failed"));

    let events = repository.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].source, SecurityAuditSource::Cli);
    assert_eq!(events[0].outcome, SecurityAuditOutcome::Success);
    assert_eq!(events[0].target.as_deref(), Some("alice"));
    assert_eq!(events[1].event_type, SecurityAuditEventType::ResourceScan);
    assert_eq!(events[1].outcome, SecurityAuditOutcome::Failure);
    assert_eq!(events[1].target, None);
}

#[tokio::test]
async fn audit_write_failure_does_not_replace_business_result() {
    let repository = RecordingRepository {
        reject_writes: true,
        ..RecordingRepository::default()
    };

    let result = audited(
        &repository,
        SecurityAuditEventType::ResourceScan,
        None,
        async { Ok("completed") },
    )
    .await;

    assert_eq!(result.unwrap(), "completed");
}
