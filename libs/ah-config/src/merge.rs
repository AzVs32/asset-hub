use figment::value::Value;
use std::{
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex, RwLock},
};

/// Type information used to remove obsolete enum defaults before ordinary merging.
pub trait DefaultMerge {
    fn prepare_defaults(defaults: &mut Value, supplied: &Value);
}

/// Select type-aware handling when a field has a configuration derive.
pub struct MergeProbe<T>(pub PhantomData<fn() -> T>);

pub trait PrepareDefaults {
    fn prepare(self, defaults: &mut Value, supplied: &Value);
}

impl<T: DefaultMerge> PrepareDefaults for &MergeProbe<T> {
    fn prepare(self, defaults: &mut Value, supplied: &Value) {
        T::prepare_defaults(defaults, supplied);
    }
}

// The extra reference is the fallback selected for ordinary Serde field types.
impl<T> PrepareDefaults for &&MergeProbe<T> {
    fn prepare(self, _: &mut Value, _: &Value) {}
}

macro_rules! transparent {
    ($($wrapper:ident),* $(,)?) => {
        $(impl<T: DefaultMerge> DefaultMerge for $wrapper<T> {
            fn prepare_defaults(defaults: &mut Value, supplied: &Value) {
                T::prepare_defaults(defaults, supplied);
            }
        })*
    };
}

// These wrappers serialize as their inner value. Merge preparation never acquires locks.
transparent!(Option, Box, Rc, Arc, Mutex, RwLock);

fn prepare_map<T: DefaultMerge>(defaults: &mut Value, supplied: &Value) {
    if let (Value::Dict(_, defaults), Value::Dict(_, supplied)) = (defaults, supplied) {
        for (key, supplied) in supplied {
            if let Some(defaults) = defaults.get_mut(key) {
                T::prepare_defaults(defaults, supplied);
            }
        }
    }
}

impl<K, T: DefaultMerge> DefaultMerge for BTreeMap<K, T> {
    fn prepare_defaults(defaults: &mut Value, supplied: &Value) {
        prepare_map::<T>(defaults, supplied);
    }
}

impl<K, T: DefaultMerge, S> DefaultMerge for HashMap<K, T, S> {
    fn prepare_defaults(defaults: &mut Value, supplied: &Value) {
        prepare_map::<T>(defaults, supplied);
    }
}
