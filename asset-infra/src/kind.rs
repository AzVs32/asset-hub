mod action_registry;
mod builder;
mod directory_action_registry;
mod directory_registry;
pub(crate) mod normalization;
mod resource_registry;
mod validation;

pub use action_registry::DefaultResourceActionRegistry;
pub use builder::{CapabilityCatalogs, build_capability_catalogs};
pub use directory_action_registry::DefaultDirectoryActionRegistry;
pub use directory_registry::DefaultDirectoryKindRegistry;
pub use resource_registry::DefaultResourceKindRegistry;

use action_registry::*;
use directory_registry::*;
use resource_registry::*;

#[cfg(test)]
mod tests;
