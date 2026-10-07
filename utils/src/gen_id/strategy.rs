use super::ParseIdError;

// Public only so exported macros can use this interface in downstream crates.
pub trait IdStrategy: std::str::FromStr {
    fn generate() -> Self;

    fn parse(value: &str) -> Result<Self, ParseIdError> {
        value.parse().map_err(|_| ParseIdError)
    }
}
