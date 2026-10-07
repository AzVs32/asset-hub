use super::ParseIdError;

// Public only so exported macros can use this interface in downstream crates.
pub trait IdStrategy: std::str::FromStr {
    fn generate() -> Self;

    /// Normalizes backend parsing failures into the local validation contract.
    /// Neither the input nor a backend error is retained in `ParseIdError`.
    fn parse(value: &str) -> Result<Self, ParseIdError> {
        value.parse().map_err(|_| ParseIdError)
    }
}
