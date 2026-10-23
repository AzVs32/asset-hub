//! Generate UUID v7 values and wrap them in a distinct MountId type.
//!
//! UuidV7 rejects other UUID versions and variants and displays a hyphenated UUID.
//! Generation creates a new identity; parsing restores an existing one.
//!
//! Run from the ah-id source root (the directory containing Cargo.toml):
//! ```sh
//! cargo run --example uuid_v7 --features uuid_v7
//! ```
//! To run only the built-in value portion without deriving an ID:
//! ```sh
//! cargo run --example uuid_v7 --no-default-features --features uuid_v7
//! ```

#[cfg(feature = "derive")]
#[derive(ah_id::Id, Debug, Clone, Copy, PartialEq, Eq)]
#[id(generate, display, parse)]
struct MountId(ah_id::UuidV7);

fn main() -> Result<(), ah_id::InvalidIdError> {
    // Built-in values work without the derive feature.
    let value = ah_id::UuidV7::new();
    let restored: ah_id::UuidV7 = value.to_string().parse()?;
    assert_eq!(value, restored);

    #[cfg(feature = "derive")]
    {
        // Generate creates a fresh identity; parsing restores an existing one.
        let created = MountId::new();
        let saved = created.to_string();
        let restored: MountId = saved.parse()?;
        assert_eq!(created, restored);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn generate_and_restore_are_distinct_operations() -> Result<(), ah_id::InvalidIdError> {
        super::main()
    }
}
