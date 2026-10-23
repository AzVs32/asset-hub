/// Create a fresh, valid value without requiring an existing identity.
///
/// This is independent of display, parsing and copying. Implementations must
/// produce values satisfying their type's invariants. Stateful or fallible
/// allocation belongs in an application-owned generator or constructor.
pub trait Generate: Sized {
    /// Generate a fresh value satisfying this type's invariants.
    fn generate() -> Self;
}
