mod action_registry;
mod builder;
mod directory_registry;
mod resource_registry;

pub(crate) use action_registry::DefaultResourceActionRegistry;
pub(crate) use builder::registries_from_catalog;
pub(crate) use directory_registry::DefaultDirectoryKindRegistry;
pub(crate) use resource_registry::DefaultResourceKindRegistry;

use action_registry::*;
use directory_registry::*;
use resource_registry::*;

#[cfg(test)]
mod tests;
