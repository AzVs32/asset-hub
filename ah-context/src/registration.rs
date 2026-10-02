use std::any::{Any, TypeId};
use std::sync::Arc;

use crate::ContextError;

#[doc(hidden)]
pub struct Registration {
    pub type_id: fn() -> TypeId,
    pub type_name: fn() -> &'static str,
    pub create: fn() -> Result<Arc<dyn Any + Send + Sync>, ContextError>,
}

#[linkme::distributed_slice]
pub static REGISTRATIONS: [Registration];
