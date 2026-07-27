mod action_registry;
mod builder;
mod directory_action_registry;
mod directory_registry;
mod resource_registry;

pub(crate) use action_registry::DefaultResourceActionRegistry;
pub(crate) use builder::{directory_action_registry_from_catalog, registries_from_catalog};
pub(crate) use directory_action_registry::DefaultDirectoryActionRegistry;
pub(crate) use directory_registry::DefaultDirectoryKindRegistry;
pub(crate) use resource_registry::DefaultResourceKindRegistry;

use action_registry::*;
use directory_registry::*;
use resource_registry::*;

#[cfg(test)]
mod tests;
