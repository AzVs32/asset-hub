use ah_context::{ContextError, context};

#[context()]
#[derive(Default)]
struct Duplicate;

ah_context::register!(Duplicate);

#[test]
fn duplicate_declarations_cannot_publish_a_context() {
    assert!(matches!(
        ah_context::load(),
        Err(ContextError::DuplicateRegistration { .. })
    ));
    assert!(matches!(
        ah_context::get::<Duplicate>(),
        Err(ContextError::NotInitialized)
    ));
}
