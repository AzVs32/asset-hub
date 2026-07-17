mod action_registry;
mod builder;
mod hierarchy;

pub(crate) use action_registry::DefaultResourceActionRegistry;
pub(crate) use builder::registries_from_catalog;
pub(crate) use hierarchy::DefaultResourceKindRegistry;

use action_registry::*;
use hierarchy::*;

#[cfg(test)]
mod tests;
