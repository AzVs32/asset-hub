/// Invalid built-in identifier input, including malformed text, an unsupported
/// UUID version/variant or a numeric overflow.
///
/// This deliberately exposes validity rather than backend-specific diagnostics.
/// It never retains the rejected input or a backend error. The message is for
/// human diagnostics, not a machine-readable error code. Callers should match
/// this type rather than inspect its display text.
///
/// Custom values keep their own `FromStr::Err` when using `#[id(parse)]`; this
/// type is not imposed on domain-specific parsing or validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid identifier")]
pub struct InvalidIdError;
