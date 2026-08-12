use super::content_abi::{AvailableContent, HostContentResolver, HostContentState};
use super::frame_url::{plugin_web_asset_url, resolve_plugin_output_urls};
use super::permissions::validate_external_permissions;
use crate::config::PluginPermissionGrants;
use asset_core::domain::StorageKey;
use asset_core::port::BlobStorage;
use asset_plugin_api::manifest::PluginPermissions;
use asset_plugin_api::protocol::{
    PLUGIN_API_VERSION, PluginFrameView, PluginResourceActionOutput, PluginView,
};
use std::sync::{Arc, Mutex};

use super::policy::PluginExecutionPolicy;

#[test]
fn plugin_frame_relative_url_is_resolved_to_plugin_web_route() {
    let mut output = PluginResourceActionOutput::new(PluginView::PluginFrame(PluginFrameView {
        plugin_api: PLUGIN_API_VERSION.to_string(),
        title: Some("demo.md".to_string()),
        url: "index.html#payload=abc".to_string(),
    }));

    resolve_plugin_output_urls(&mut output, "azvs.markdown").unwrap();

    let Some(PluginView::PluginFrame(frame)) = output.view else {
        panic!("expected plugin frame");
    };
    assert_eq!(frame.url, "/plugins/azvs.markdown/index.html#payload=abc");
}

#[test]
fn plugin_frame_rejects_a_different_plugin_api() {
    let mut output = PluginResourceActionOutput::new(PluginView::PluginFrame(PluginFrameView {
        plugin_api: "asset-hub.plugin-api@4".to_string(),
        title: None,
        url: "index.html".to_string(),
    }));

    assert!(resolve_plugin_output_urls(&mut output, "azvs.markdown").is_err());
}

#[test]
fn plugin_frame_public_or_protocol_url_is_rejected() {
    assert!(plugin_web_asset_url("azvs.markdown", "/plugins/custom/index.html").is_err());
    assert!(plugin_web_asset_url("azvs.markdown", "asset://content/demo.md").is_err());
    assert!(plugin_web_asset_url("azvs.markdown", "https://example.com/view").is_err());
}

#[test]
fn plugin_frame_hash_only_url_defaults_to_index_html() {
    assert_eq!(
        plugin_web_asset_url("azvs.markdown", "#payload=abc").unwrap(),
        "/plugins/azvs.markdown/index.html#payload=abc"
    );
}

#[test]
fn external_permissions_require_matching_host_grants() {
    let permissions: PluginPermissions = serde_json::from_value(serde_json::json!({
        "allow": ["resource.read", "resource.delete", "directory.delete"],
        "network": {"hosts": ["api.example.com"]},
        "filesystem": {"read": ["/srv/plugins/input"], "write": []}
    }))
    .unwrap();

    let error = validate_external_permissions(
        "example.plugin",
        &permissions,
        &PluginPermissionGrants::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("without a matching host grant"));

    let grants = PluginPermissionGrants {
        resource_delete: true,
        directory_delete: true,
        network_hosts: vec!["api.example.com".to_string()],
        filesystem_read: vec!["/srv/plugins".into()],
        filesystem_write: Vec::new(),
    };
    validate_external_permissions("example.plugin", &permissions, &grants).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn content_handles_read_raw_bounded_ranges_and_close() {
    let root = std::env::temp_dir().join(format!(
        "asset-hub-content-handle-test-{}",
        uuid::Uuid::now_v7()
    ));
    let storage = Arc::new(crate::storage::OpenDalBlobStorage::from_local_root(&root).unwrap());
    let key = StorageKey::new("docs/demo.bin").unwrap();
    storage
        .put(&key, bytes::Bytes::from_static(b"abcdefgh"))
        .await
        .unwrap();
    let reference = "asset://content/test".to_string();
    let mut state = HostContentState::default();
    state
        .references
        .insert(reference.clone(), AvailableContent { key, size: 8 });
    let resolver = HostContentResolver {
        storage,
        state: Arc::new(Mutex::new(state)),
        runtime: tokio::runtime::Handle::current(),
        policy: Arc::new(PluginExecutionPolicy::new(128, 16, 3, 1024, 1024, 1, 32, 5).unwrap()),
    };

    tokio::task::spawn_blocking(move || {
        let handle = resolver.open(&reference).unwrap();
        assert_eq!(resolver.size(&handle).unwrap(), 8);
        assert_eq!(resolver.read(&handle, 2, 100).unwrap(), b"cde");
        assert_eq!(resolver.read(&handle, 5, 3).unwrap(), b"fgh");
        resolver.close(&handle).unwrap();
        assert!(resolver.size(&handle).is_err());
    })
    .await
    .unwrap();

    let _ = std::fs::remove_dir_all(root);
}
