mod action_registry;
mod builder;
mod directory;
mod hierarchy;

pub(crate) use action_registry::DefaultResourceActionRegistry;
pub(crate) use builder::registries_from_catalog;
pub(crate) use directory::DefaultDirectoryKindRegistry;
pub(crate) use hierarchy::DefaultResourceKindRegistry;

use action_registry::*;
use directory::*;
use hierarchy::*;

#[cfg(test)]
mod tests;
