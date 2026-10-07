use std::any::{TypeId, type_name};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::error::Error;
use std::sync::Arc;

use crate::auto::{self, ASSET_CONTEXT_AUTO_REGISTRATIONS};
use crate::context::SharedValue;
use crate::{Context, ContextError};

type Initializer = Box<dyn FnOnce() -> Result<SharedValue, ContextError> + Send>;

struct Registration {
    type_id: TypeId,
    initialize: Initializer,
}

/// Registers types and constructs an independent shared context.
///
/// A new builder starts empty and does not access the global context.
/// Registration checks duplicates immediately; constructors run only in [`Self::build`].
/// Use `build()` for an independent context or [`crate::load_with`] to publish
/// the builder's values globally.
///
/// ```
/// use asset_context::ContextBuilder;
///
/// #[derive(Default)]
/// struct Counter(std::sync::atomic::AtomicUsize);
/// struct Label(String);
///
/// let label = String::from("local");
/// let mut builder = ContextBuilder::new();
/// builder.register::<Counter>()?;
/// builder.register_with(move || Ok::<_, std::io::Error>(Label(label)))?;
/// let context = builder.build()?;
/// assert_eq!(context.get::<Label>()?.0, "local");
/// # Ok::<(), asset_context::ContextError>(())
/// ```
#[derive(Default)]
pub struct ContextBuilder {
    registrations: Vec<Registration>,
    types: HashSet<TypeId>,
}

impl ContextBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Collects declarations made with `#[context]` and `register!`.
    ///
    /// Returns a duplicate-registration error before running any constructor.
    /// The builder can be extended and built without publishing a global context.
    pub fn from_auto() -> Result<Self, ContextError> {
        auto::collect(&ASSET_CONTEXT_AUTO_REGISTRATIONS)
    }

    /// Registers a type to be constructed using `Default` during [`Self::build`].
    pub fn register<T>(&mut self) -> Result<&mut Self, ContextError>
    where
        T: Default + Send + Sync + 'static,
    {
        self.register_with(|| Ok::<T, Infallible>(T::default()))
    }

    /// Registers a fallible initializer, without invoking it.
    ///
    /// The initializer may capture owned dependencies and is called once during
    /// [`Self::build`]. A rejected duplicate leaves the existing initializer intact.
    /// Its error is converted into a boxed source; strings and typed or boxed
    /// thread-safe errors are accepted without losing the original error chain.
    pub fn register_with<T, E>(
        &mut self,
        init: impl FnOnce() -> Result<T, E> + Send + 'static,
    ) -> Result<&mut Self, ContextError>
    where
        T: Send + Sync + 'static,
        E: Into<Box<dyn Error + Send + Sync>>,
    {
        let type_id = TypeId::of::<T>();
        if !self.types.insert(type_id) {
            return Err(ContextError::DuplicateRegistration {
                type_name: type_name::<T>(),
            });
        }

        let initialize: Initializer = Box::new(move || {
            let value = init().map_err(|source| ContextError::Initialization {
                type_name: type_name::<T>(),
                source: source.into(),
            })?;
            Ok(Arc::new(value))
        });
        self.registrations.push(Registration {
            type_id,
            initialize,
        });
        Ok(self)
    }

    /// Consumes the builder and constructs values in registration order.
    ///
    /// Returns a context only if every initializer succeeds. On failure, already
    /// constructed values are dropped; external side effects are not rolled back.
    /// To retry, create a new builder. Constructor panics are not recovered.
    pub fn build(self) -> Result<Context, ContextError> {
        let mut values = HashMap::with_capacity(self.registrations.len());
        for registration in self.registrations {
            values.insert(registration.type_id, (registration.initialize)()?);
        }
        Ok(Context { values })
    }
}
