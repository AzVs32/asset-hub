use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ah_context::ContextError;

static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

struct Retryable(u32);

ah_context::register!(Retryable, init = initialize);

fn initialize() -> Result<Retryable, std::io::Error> {
    if ATTEMPTS.fetch_add(1, Ordering::SeqCst) == 0 {
        Err(std::io::Error::other("not ready"))
    } else {
        Ok(Retryable(42))
    }
}

#[test]
fn constructor_errors_preserve_the_source_without_publishing_partial_state() {
    let error = match ah_context::load() {
        Ok(_) => panic!("the first initialization must fail"),
        Err(error) => error,
    };
    assert!(
        matches!(error, ContextError::Initialization { type_name, .. } if type_name.ends_with("::Retryable"))
    );
    assert_eq!(error.source().unwrap().to_string(), "not ready");
    assert!(matches!(
        ah_context::get::<Retryable>(),
        Err(ContextError::NotInitialized)
    ));

    ah_context::load().unwrap();
    let value = ah_context::get::<Retryable>().unwrap();
    assert_eq!(value.0, 42);
    assert!(Arc::ptr_eq(
        &value,
        &ah_context::get::<Retryable>().unwrap()
    ));
    assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 2);
}
