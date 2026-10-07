use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::sync::Arc;

use crate::ContextError;

pub(crate) type SharedValue = Arc<dyn Any + Send + Sync>;

/// Shared values constructed by [`crate::ContextBuilder`].
pub struct Context {
    pub(crate) values: HashMap<TypeId, SharedValue>,
}

impl Context {
    /// Returns an `Arc` pointing to this type's shared instance.
    pub fn get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, ContextError> {
        let value = self
            .values
            .get(&TypeId::of::<T>())
            .ok_or(ContextError::NotRegistered {
                type_name: type_name::<T>(),
            })?;
        Ok(Arc::clone(value).downcast::<T>().unwrap())
    }
}
