mod builder;
mod directory_registry;
mod resource_registry;

pub use builder::{KindCatalogs, build_kind_catalogs};
pub use directory_registry::DefaultDirectoryKindRegistry;
pub use resource_registry::DefaultResourceKindRegistry;

use directory_registry::*;
