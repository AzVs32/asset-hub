use super::*;
use asset_plugin_api::protocol::{
    PluginActionFailure, PluginDiagnostic, PluginDiagnosticSeverity, diagnostic::codes,
};

#[test]
fn plugin_diagnostic_codes_map_to_stable_http_statuses() {
    assert_eq!(
        plugin_status(asset_plugin_api::protocol::diagnostic::codes::INVALID_INPUT),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        plugin_status(asset_plugin_api::protocol::diagnostic::codes::PERMISSION_DENIED),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        plugin_status(asset_plugin_api::protocol::diagnostic::codes::CONTENT_LIMIT_EXCEEDED),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        plugin_status(asset_plugin_api::protocol::diagnostic::codes::TIMEOUT),
        StatusCode::GATEWAY_TIMEOUT
    );
}

#[test]
fn plugin_failure_preserves_additional_diagnostics() {
    let mut failure =
        PluginActionFailure::new(PluginDiagnostic::error(codes::INVALID_INPUT, "invalid"));
    failure.diagnostics.push(PluginDiagnostic {
        code: "plugin.input_hint".to_string(),
        message: "Provide a value".to_string(),
        severity: PluginDiagnosticSeverity::Info,
        retryable: false,
        details: None,
    });

    let error = HttpError::from(CoreError::plugin_failure(
        "example.plugin",
        "example.action",
        failure,
    ));
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.diagnostics.len(), 1);
    assert_eq!(error.diagnostics[0].code, "plugin.input_hint");
}
