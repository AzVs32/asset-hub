use super::*;
use asset_core::domain::{ResourceAction, ResourceId};
use asset_core::port::ResourceActionOutput;
use asset_plugin_api::protocol::{
    PluginActionOutput, PluginDiagnostic, PluginDiagnosticSeverity, PluginView, TextView,
};
use serde_json::json;

#[test]
fn action_output_response_preserves_plugin_diagnostics() {
    let mut plugin_output = PluginActionOutput::new(PluginView::Text(TextView {
        text: "done".to_string(),
    }));
    plugin_output.diagnostics.push(PluginDiagnostic {
        code: "plugin.normalized_input".to_string(),
        message: "Input was normalized".to_string(),
        severity: PluginDiagnosticSeverity::Warning,
        retryable: false,
        details: Some(json!({"field": "title"})),
    });
    let output = ResourceActionOutput::new(
        ResourceId::new(),
        ResourceAction::from("example.inspect"),
        plugin_output,
    );

    let value = serde_json::to_value(ResourceActionOutputResponse::from(&output)).unwrap();
    assert_eq!(value["diagnostics"][0]["code"], "plugin.normalized_input");
    assert_eq!(value["diagnostics"][0]["severity"], "warning");
    assert_eq!(value["diagnostics"][0]["details"]["field"], "title");
}
