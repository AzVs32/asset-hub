use std::sync::{Arc, Mutex, OnceLock};

use crate::{Context, ContextBuilder, ContextError};

static GLOBAL: GlobalContext = GlobalContext::new();

struct GlobalContext {
    context: OnceLock<Context>,
    initialization: Mutex<()>,
}

impl GlobalContext {
    const fn new() -> Self {
        Self {
            context: OnceLock::new(),
            initialization: Mutex::new(()),
        }
    }

    fn load(&self) -> Result<&Context, ContextError> {
        if let Some(context) = self.context.get() {
            return Ok(context);
        }
        let _lock = self.initialization.lock().unwrap();
        if let Some(context) = self.context.get() {
            return Ok(context);
        }
        self.publish(ContextBuilder::from_auto()?)
    }

    fn load_with(&self, builder: ContextBuilder) -> Result<&Context, ContextError> {
        if self.context.get().is_some() {
            return Err(ContextError::AlreadyInitialized);
        }
        let _lock = self.initialization.lock().unwrap();
        if self.context.get().is_some() {
            return Err(ContextError::AlreadyInitialized);
        }
        self.publish(builder)
    }

    // Callers hold the initialization lock and have checked that no context exists.
    fn publish(&self, builder: ContextBuilder) -> Result<&Context, ContextError> {
        let context = builder.build()?;
        Ok(self.context.get_or_init(|| context))
    }
}

/// Constructs and publishes all registered values during startup.
///
/// All registrations are checked for duplicates before any constructor runs.
/// Loading is process-wide and idempotent after successful initialization.
/// Repeated or concurrent calls return the same successfully loaded context.
/// A construction error leaves the context unpublished, allowing a later retry.
/// Constructors must not call either global loader recursively. Constructor panics are
/// programming errors and are not recovered.
pub fn load() -> Result<&'static Context, ContextError> {
    GLOBAL.load()
}

/// Constructs and publishes a custom builder as the process-wide context.
///
/// Consumes exactly the supplied registrations. Use [`ContextBuilder::from_auto`]
/// to include automatic declarations, then add initializers capturing loaded
/// configuration or other owned dependencies.
///
/// Unlike [`load`], this function rejects an existing context rather than
/// discarding the supplied registrations. Concurrent calls to either function
/// share one initialization lock. A rejected builder's initializers never run.
/// Construction errors leave the global context unpublished, allowing a retry
/// with a new builder; already built values are dropped and external side effects
/// are not rolled back. Initializers must not recursively call either loader.
/// Constructor panics are not recovered.
///
/// # Errors
///
/// Returns [`ContextError::AlreadyInitialized`] if either loader has already
/// published a global context, or [`ContextError::Initialization`] if an
/// initializer fails.
pub fn load_with(builder: ContextBuilder) -> Result<&'static Context, ContextError> {
    GLOBAL.load_with(builder)
}

/// Retrieves a shared value after startup has completed [`load`] or [`load_with`].
pub fn get<T: Send + Sync + 'static>() -> Result<Arc<T>, ContextError> {
    GLOBAL
        .context
        .get()
        .ok_or(ContextError::NotInitialized)?
        .get::<T>()
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::error::Error;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    use super::GlobalContext;
    use crate::{Context, ContextBuilder, ContextError};

    struct Value(usize);

    fn builder(value: usize) -> ContextBuilder {
        let mut builder = ContextBuilder::new();
        builder
            .register_with(move || Ok::<_, Infallible>(Value(value)))
            .unwrap();
        builder
    }

    #[test]
    fn failed_custom_load_leaves_the_context_unpublished_and_allows_a_retry() {
        let global = GlobalContext::new();
        let mut failed = ContextBuilder::new();
        failed
            .register_with(|| Err::<Value, _>("not ready"))
            .unwrap();

        let error = global.load_with(failed).err().unwrap();
        assert!(matches!(&error, ContextError::Initialization { .. }));
        assert_eq!(error.source().unwrap().to_string(), "not ready");
        assert!(global.context.get().is_none());

        let context = global.load_with(builder(42)).unwrap();
        assert_eq!(context.get::<Value>().unwrap().0, 42);
        assert!(std::ptr::eq(context, global.load().unwrap()));
    }

    #[test]
    fn either_loader_rejects_a_later_custom_builder_without_running_it() {
        for automatic_first in [false, true] {
            let global = GlobalContext::new();
            let context = if automatic_first {
                global.load().unwrap()
            } else {
                global.load_with(builder(42)).unwrap()
            };
            let calls = Arc::new(AtomicUsize::new(0));
            let count = Arc::clone(&calls);
            let mut rejected = ContextBuilder::new();
            rejected
                .register_with(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, Infallible>(Value(99))
                })
                .unwrap();

            assert!(matches!(
                global.load_with(rejected),
                Err(ContextError::AlreadyInitialized)
            ));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert!(std::ptr::eq(context, global.load().unwrap()));
            if !automatic_first {
                assert_eq!(context.get::<Value>().unwrap().0, 42);
            }
        }
    }

    #[test]
    fn concurrent_custom_and_automatic_loads_publish_one_context() {
        for include_automatic in [false, true] {
            let global = Arc::new(GlobalContext::new());
            let calls = Arc::new(AtomicUsize::new(0));
            let barrier = Arc::new(Barrier::new(16));
            let threads: Vec<_> = (0..16)
                .map(|index| {
                    let global = Arc::clone(&global);
                    let calls = Arc::clone(&calls);
                    let barrier = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        let automatic = include_automatic && index % 2 == 0;
                        let mut candidate = ContextBuilder::new();
                        if !automatic {
                            candidate
                                .register_with(move || {
                                    calls.fetch_add(1, Ordering::SeqCst);
                                    Ok::<_, Infallible>(Value(index))
                                })
                                .unwrap();
                        }
                        barrier.wait();
                        let result = if automatic {
                            global.load()
                        } else {
                            global.load_with(candidate)
                        };
                        (
                            automatic,
                            result.map(|context| context as *const Context as usize),
                        )
                    })
                })
                .collect();

            let results: Vec<_> = threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect();
            let published = global.load().unwrap() as *const Context as usize;
            let mut custom_successes = 0;
            for (automatic, result) in results {
                match result {
                    Ok(address) => {
                        assert_eq!(address, published);
                        if !automatic {
                            custom_successes += 1;
                        }
                    }
                    Err(ContextError::AlreadyInitialized) => assert!(!automatic),
                    Err(error) => panic!("unexpected load error: {error}"),
                }
            }
            assert_eq!(calls.load(Ordering::SeqCst), custom_successes);
            if include_automatic {
                assert!(custom_successes <= 1);
            } else {
                assert_eq!(custom_successes, 1);
            }
        }
    }
}
