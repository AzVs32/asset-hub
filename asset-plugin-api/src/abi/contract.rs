use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AbiValueType {
    String,
    U64,
    Bytes,
    Unit,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AbiParameter {
    pub name: &'static str,
    pub value_type: AbiValueType,
    pub schema: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostFunctionSpec {
    pub name: &'static str,
    pub parameters: &'static [AbiParameter],
    pub result: AbiValueType,
    pub result_schema: Option<&'static str>,
    pub lifecycle: &'static str,
}

pub const CONTENT_OPEN: HostFunctionSpec = HostFunctionSpec {
    name: "asset_hub_content_open",
    parameters: &[AbiParameter {
        name: "reference",
        value_type: AbiValueType::String,
        schema: None,
    }],
    result: AbiValueType::String,
    result_schema: None,
    lifecycle: "Returns a call-scoped handle that must be closed before the action returns.",
};

pub const CONTENT_SIZE: HostFunctionSpec = HostFunctionSpec {
    name: "asset_hub_content_size",
    parameters: &[AbiParameter {
        name: "handle",
        value_type: AbiValueType::String,
        schema: None,
    }],
    result: AbiValueType::U64,
    result_schema: None,
    lifecycle: "The handle must have been returned by content_open in the current action call.",
};

pub const CONTENT_READ: HostFunctionSpec = HostFunctionSpec {
    name: "asset_hub_content_read",
    parameters: &[
        AbiParameter {
            name: "handle",
            value_type: AbiValueType::String,
            schema: None,
        },
        AbiParameter {
            name: "offset",
            value_type: AbiValueType::U64,
            schema: None,
        },
        AbiParameter {
            name: "length",
            value_type: AbiValueType::U64,
            schema: None,
        },
    ],
    result: AbiValueType::Bytes,
    result_schema: None,
    lifecycle: "Reads at most the Host policy limit and remaining content bytes.",
};

pub const CONTENT_CLOSE: HostFunctionSpec = HostFunctionSpec {
    name: "asset_hub_content_close",
    parameters: &[AbiParameter {
        name: "handle",
        value_type: AbiValueType::String,
        schema: None,
    }],
    result: AbiValueType::Unit,
    result_schema: None,
    lifecycle: "Invalidates the call-scoped handle.",
};

pub const DIRECTORY_LIST_CHILDREN: HostFunctionSpec = HostFunctionSpec {
    name: "asset_hub_directory_list_children",
    parameters: &[AbiParameter {
        name: "request",
        value_type: AbiValueType::Json,
        schema: Some("DirectoryPageRequest"),
    }],
    result: AbiValueType::Json,
    result_schema: Some("PluginDirectoryPage"),
    lifecycle: "The request reference is valid only during the current Directory Action call.",
};

pub const DIRECTORY_LIST_RESOURCES: HostFunctionSpec = HostFunctionSpec {
    name: "asset_hub_directory_list_resources",
    parameters: &[AbiParameter {
        name: "request",
        value_type: AbiValueType::Json,
        schema: Some("DirectoryPageRequest"),
    }],
    result: AbiValueType::Json,
    result_schema: Some("PluginDirectoryResourcePage"),
    lifecycle: "The request reference and returned content references are call-scoped.",
};

pub const HOST_FUNCTIONS: &[HostFunctionSpec] = &[
    CONTENT_OPEN,
    CONTENT_SIZE,
    CONTENT_READ,
    CONTENT_CLOSE,
    DIRECTORY_LIST_CHILDREN,
    DIRECTORY_LIST_RESOURCES,
];
