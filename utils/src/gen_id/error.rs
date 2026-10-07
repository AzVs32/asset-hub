use thiserror::Error;

/// The input does not represent an identifier accepted by the selected strategy.
///
/// This is a local validation error. It does not store the input or expose a
/// backend parser error. All invalid representations have the same error
/// contract; the operation using the identifier decides its business meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid identifier")]
pub struct ParseIdError;
