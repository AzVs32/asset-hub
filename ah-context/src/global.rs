use std::sync::{Arc, Mutex, OnceLock};

use crate::{Context, ContextBuilder, ContextError};

static CONTEXT: OnceLock<Context> = OnceLock::new();
static INITIALIZATION: Mutex<()> = Mutex::new(());

/// Constructs and publishes all registered values during startup.
///
/// All registrations are checked for duplicates before any constructor runs.
/// Loading is process-wide and idempotent after successful initialization.
/// Repeated or concurrent calls return the same successfully loaded context.
/// A construction error leaves the context unpublished, allowing a later retry.
/// Constructors must not call `load()` recursively. Constructor panics are
/// programming errors and are not recovered.
pub fn load() -> Result<&'static Context, ContextError> {
    if let Some(context) = CONTEXT.get() {
        return Ok(context);
    }
    let _lock = INITIALIZATION.lock().unwrap();
    if let Some(context) = CONTEXT.get() {
        return Ok(context);
    }
    let context = ContextBuilder::from_auto()?.build()?;
    Ok(CONTEXT.get_or_init(|| context))
}

/// Retrieves a shared value after startup has completed [`load`].
pub fn get<T: Send + Sync + 'static>() -> Result<Arc<T>, ContextError> {
    CONTEXT
        .get()
        .ok_or(ContextError::NotInitialized)?
        .get::<T>()
}
