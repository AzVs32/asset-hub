use super::ParseIdError;
use super::strategy::IdStrategy;

pub use ulid::Ulid as ULID;

impl IdStrategy for ULID {
    fn generate() -> Self {
        Self::generate()
    }

    fn parse(value: &str) -> Result<Self, ParseIdError> {
        // A 26-character Base32 value must fit in 128 bits. The upstream
        // parser truncates overflowing bits, so validate the first digit.
        if !matches!(value.as_bytes().first(), Some(b'0'..=b'7')) {
            return Err(ParseIdError);
        }
        value.parse().map_err(|_| ParseIdError)
    }
}
