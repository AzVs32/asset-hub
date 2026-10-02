use std::any::{Any, TypeId, type_name};
use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;

use crate::ContextError;
use crate::registration::REGISTRATIONS;

type SharedValue = Arc<dyn Any + Send + Sync>;

/// The automatically registered values published during startup.
pub struct Context {
    values: HashMap<TypeId, SharedValue>,
}

impl Context {
    pub(crate) fn load() -> Result<Self, ContextError> {
        let mut registrations: Vec<_> = REGISTRATIONS.iter().collect();
        registrations.sort_unstable_by_key(|item| (item.type_name)());

        let mut values = HashMap::with_capacity(registrations.len());
        for registration in registrations {
            match values.entry((registration.type_id)()) {
                Entry::Vacant(entry) => {
                    entry.insert((registration.create)()?);
                }
                Entry::Occupied(_) => {
                    return Err(ContextError::DuplicateRegistration {
                        type_name: (registration.type_name)(),
                    });
                }
            }
        }
        Ok(Self { values })
    }

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
