//! Asset Hub plugin protocol.
//!
//! Plugin authors should be able to depend on this crate to understand and use:
//! - manifest structure and permission declarations,
//! - action handler input payloads,
//! - action handler output payloads,
//! - shared view response types.

pub mod action;
pub mod manifest;
pub mod request;
pub mod view;

pub use action::{
    ResourceAction, ResourceActionAccess, ResourceActionDefinition, ResourceActionWhen,
};
pub use manifest::{
    ActionAppliesTo, ActionExecutor, ActionOutputContract, ActionRequirements, ActionUi,
    ContentDelivery, MANIFEST_VERSION, ManifestActionAccess, PluginCapabilities, PluginManifest,
    PluginMetadata, PluginPermissions, PluginRuntime, ReadWritePermission,
    ResourceActionCapability, ResourceKindCapability,
};
pub use request::{
    PluginActionRequest, PluginChecksum, PluginContentBytes, PluginResource, PluginResourceContent,
};
pub use view::{
    BinaryUrlView, FormView, HtmlView, JsonView, MarkdownView, MediaView, PluginActionOutput,
    PluginContentEncoding, PluginView, TableColumn, TableView, TextView,
};
