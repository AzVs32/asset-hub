//! Generate ULID values and wrap them in a distinct EventId type.
//!
//! Ulid displays 26 canonical Crockford Base32 characters. Its parser rejects
//! inputs that overflow 128 bits; parsing errors use InvalidIdError.
//!
//! Run from the ah-id source root (the directory containing Cargo.toml):
//! ```sh
//! cargo run --example ulid --features ulid
//! ```
//! To run only the built-in value portion without deriving an ID:
//! ```sh
//! cargo run --example ulid --no-default-features --features ulid
//! ```

#[cfg(feature = "derive")]
#[derive(ah_id::Id, Debug, Clone, Copy, PartialEq, Eq)]
#[id(generate, display, parse)]
struct EventId(ah_id::Ulid);

fn main() -> Result<(), ah_id::InvalidIdError> {
    // Built-in values work without the derive feature.
    let value = ah_id::Ulid::new();
    let restored: ah_id::Ulid = value.to_string().parse()?;
    assert_eq!(value, restored);

    #[cfg(feature = "derive")]
    {
        let created = EventId::new();
        let restored: EventId = created.to_string().parse()?;
        assert_eq!(created, restored);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn generate_and_restore() -> Result<(), ah_id::InvalidIdError> {
        super::main()
    }
}
