//! Browser Frame RPC response contract after Host effect application.

use super::{PluginDiagnostic, PluginView};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameArgumentSpec {
    pub name: &'static str,
    pub schema: &'static str,
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameMethodSpec {
    pub name: &'static str,
    pub arguments: &'static [FrameArgumentSpec],
    pub result_schema: &'static str,
}

pub const RESOURCE_FRAME_METHODS: &[FrameMethodSpec] = &[
    FrameMethodSpec {
        name: "executeResourceAction",
        arguments: &[
            FrameArgumentSpec {
                name: "action",
                schema: "string",
                optional: false,
            },
            FrameArgumentSpec {
                name: "input",
                schema: "JsonObject",
                optional: true,
            },
        ],
        result_schema: "ResourceFrameActionOutput",
    },
    FrameMethodSpec {
        name: "replaceResourceText",
        arguments: &[FrameArgumentSpec {
            name: "text",
            schema: "string",
            optional: false,
        }],
        result_schema: "void",
    },
];

pub const DIRECTORY_FRAME_METHODS: &[FrameMethodSpec] = &[
    FrameMethodSpec {
        name: "executeDirectoryAction",
        arguments: &[
            FrameArgumentSpec {
                name: "action",
                schema: "string",
                optional: false,
            },
            FrameArgumentSpec {
                name: "input",
                schema: "JsonObject",
                optional: true,
            },
        ],
        result_schema: "DirectoryFrameActionOutput",
    },
    FrameMethodSpec {
        name: "viewResource",
        arguments: &[
            FrameArgumentSpec {
                name: "resourceId",
                schema: "string",
                optional: false,
            },
            FrameArgumentSpec {
                name: "input",
                schema: "JsonObject",
                optional: true,
            },
        ],
        result_schema: "ResourceFrameActionOutput",
    },
    FrameMethodSpec {
        name: "refreshDirectory",
        arguments: &[],
        result_schema: "void",
    },
    FrameMethodSpec {
        name: "navigateToDirectory",
        arguments: &[FrameArgumentSpec {
            name: "path",
            schema: "string",
            optional: false,
        }],
        result_schema: "void",
    },
    FrameMethodSpec {
        name: "editResource",
        arguments: &[FrameArgumentSpec {
            name: "resourceId",
            schema: "string",
            optional: false,
        }],
        result_schema: "void",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceFrameEffectKind {
    ReplaceContent,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryFrameEffectKind {
    Update,
    CreateChild,
    CreateTree,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceFrameActionOutput {
    pub resource_id: String,
    pub action: String,
    pub view: Option<PluginView>,
    pub effects: Vec<ResourceFrameEffectKind>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryFrameActionOutput {
    pub directory_id: String,
    pub action: String,
    pub view: Option<PluginView>,
    pub effects: Vec<DirectoryFrameEffectKind>,
    pub diagnostics: Vec<PluginDiagnostic>,
}
