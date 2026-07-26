use asset_core::domain::{
    SecurityAuditActor, SecurityAuditEventType, SecurityAuditOutcome, SecurityAuditSource,
};
use asset_infra::config::{
    AssetInfraConfig, BlobConfig, DatabaseConfig, LocalBlobConfig, LocalBlobSyncConfig,
    SqliteDatabaseConfig,
};
use asset_runtime::AssetRuntime;
use std::path::{Path, PathBuf};
use std::process::Command;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("asset-cli-audit-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn system_scan_records_a_cli_audit_event_in_sqlite() {
    let root = TestRoot::new();
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite: SqliteDatabaseConfig { max_connections: 1 },
            ..DatabaseConfig::default()
        },
        blob: BlobConfig {
            local: LocalBlobConfig {
                root: root.path().join("data"),
                sync: LocalBlobSyncConfig {
                    enabled: false,
                    ..LocalBlobSyncConfig::default()
                },
            },
            ..BlobConfig::default()
        },
        ..AssetInfraConfig::default()
    };
    std::fs::write(
        root.path().join("config.toml"),
        toml::to_string_pretty(&config).unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_asset"))
        .current_dir(root.path())
        .args(["system", "--scan-resource"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "asset failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let runtime = AssetRuntime::new(config).await.unwrap();
    let events = runtime
        .security_audit_repository()
        .list(10, 0)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].actor, SecurityAuditActor::Unauthenticated);
    assert_eq!(events[0].source, SecurityAuditSource::Cli);
    assert_eq!(events[0].event_type, SecurityAuditEventType::ResourceScan);
    assert_eq!(events[0].outcome, SecurityAuditOutcome::Success);
    assert_eq!(events[0].target, None);
}
