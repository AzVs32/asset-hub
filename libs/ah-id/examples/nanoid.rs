//! Generate and restore a 21-character URL-safe NanoID, directly or through TokenId.
//!
//! `generate` adds new() and Generate; display and parse delegate to the value's
//! Display and FromStr. The value owns the text format and validation; parsing
//! preserves InvalidIdError. Generation does not check storage for duplicate IDs.
//! Standard derives are chosen explicitly by the consumer.
//!
//! Run from the ah-id source root (the directory containing Cargo.toml):
//! ```sh
//! cargo run --example nanoid --features nanoid
//! ```
//! To run only the built-in value portion without deriving an ID:
//! ```sh
//! cargo run --example nanoid --no-default-features --features nanoid
//! ```

#[cfg(feature = "derive")]
#[derive(ah_id::Id, Debug, Clone, PartialEq, Eq)]
#[id(generate, display, parse)]
struct TokenId(ah_id::NanoId);

fn main() -> Result<(), ah_id::InvalidIdError> {
    // Built-in values work without the derive feature.
    let value = ah_id::NanoId::new();
    let restored: ah_id::NanoId = value.to_string().parse()?;
    assert_eq!(value, restored);

    #[cfg(feature = "derive")]
    {
        let created = TokenId::new();
        let restored: TokenId = created.to_string().parse()?;
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
