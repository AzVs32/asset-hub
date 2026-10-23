//! Identify an order line by a NanoID order ID and a positive line number.
//!
//! The application assigns line numbers within an order, so OrderLineId has no
//! generator. Generate is optional and suits parameterless, infallible creation;
//! fallible, configured or stateful allocation belongs in application services.
//!
//! OrderLineId validates its key through TryFrom and exposes it through AsRef.
//! Eq and Hash include both fields. Storage restores the fields separately and
//! reruns validation; no combined Display or FromStr format is required.
//! The storage adapter owns column mapping and the database enforces uniqueness.
//!
//! Run from the ah-id source root (the directory containing Cargo.toml):
//! ```sh
//! cargo run --example order_line --features nanoid
//! ```

// Orders have independently generated IDs; a line is identified by its order ID and line number.
mod ah_example_order {
    #[derive(ah_id::Id, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[id(generate, display, parse)]
    pub struct OrderId(ah_id::NanoId);

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct OrderLineKey {
        pub order_id: OrderId,
        pub line_number: u32,
    }
}

mod ah_example_order_line_id {
    use super::ah_example_order::OrderLineKey;

    // The application assigns line numbers within each order, so the composite ID has no generator.
    // Derive Eq and Hash to use the entire composite identity as a set or cache key.
    #[derive(ah_id::Id, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[id(as_ref)]
    pub struct OrderLineId(OrderLineKey);

    #[derive(Debug, PartialEq, Eq, thiserror::Error)]
    #[error("line number must be greater than zero")]
    pub struct InvalidLineNumber;

    impl TryFrom<OrderLineKey> for OrderLineId {
        type Error = InvalidLineNumber;

        fn try_from(key: OrderLineKey) -> Result<Self, Self::Error> {
            if key.line_number == 0 {
                return Err(InvalidLineNumber);
            }
            Ok(Self(key))
        }
    }
}

use ah_example_order::{OrderId, OrderLineKey};
use ah_example_order_line_id::{InvalidLineNumber, OrderLineId};
use std::collections::HashSet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let order_id = OrderId::new();
    let first = OrderLineId::try_from(OrderLineKey {
        order_id,
        line_number: 1,
    })?;
    let second = OrderLineId::try_from(OrderLineKey {
        order_id,
        line_number: 2,
    })?;
    assert_ne!(first, second);

    // A storage adapter can map the fields to separate order_id and line_number columns.
    // The database enforces uniqueness with PRIMARY KEY (order_id, line_number).
    let key = first.as_ref();
    let stored_order_id = key.order_id.to_string();
    let stored_line_number = key.line_number;

    // Restore from both columns, parsing the order ID and validating the line number separately.
    let restored = OrderLineId::try_from(OrderLineKey {
        order_id: stored_order_id.parse()?,
        line_number: stored_line_number,
    })?;
    assert_eq!(restored, first);

    let mut identities = HashSet::new();
    assert!(identities.insert(first));
    assert!(identities.insert(second));
    assert!(!identities.insert(restored));

    assert_eq!(
        OrderLineId::try_from(OrderLineKey {
            order_id,
            line_number: 0,
        }),
        Err(InvalidLineNumber)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_hashing_include_both_columns() -> Result<(), Box<dyn std::error::Error>> {
        // Use fixed existing IDs to test identity semantics independently of random generation.
        let order_a: OrderId = "order_000000000000001".parse()?;
        let order_b: OrderId = "order_000000000000002".parse()?;
        let line = |order_id, line_number| {
            OrderLineId::try_from(OrderLineKey {
                order_id,
                line_number,
            })
        };

        let first = line(order_a, 1)?;
        let same = line(order_a, 1)?;
        let other_line = line(order_a, 2)?;
        let other_order = line(order_b, 1)?;
        assert_eq!(first, same);
        assert_ne!(first, other_line);
        assert_ne!(first, other_order);
        assert_eq!(
            HashSet::from([first, same, other_line, other_order]).len(),
            3
        );
        Ok(())
    }

    #[test]
    fn line_numbers_are_validated_when_restoring_keys() -> Result<(), Box<dyn std::error::Error>> {
        let order_id: OrderId = "order_000000000000001".parse()?;
        assert_eq!(
            OrderLineId::try_from(OrderLineKey {
                order_id,
                line_number: 0,
            }),
            Err(InvalidLineNumber)
        );
        for line_number in [1, 2, u32::MAX] {
            let id = OrderLineId::try_from(OrderLineKey {
                order_id,
                line_number,
            })?;
            assert_eq!(id.as_ref().order_id, order_id);
            assert_eq!(id.as_ref().line_number, line_number);
        }
        Ok(())
    }

    #[test]
    fn example_restores_composite_identity() -> Result<(), Box<dyn std::error::Error>> {
        main()
    }
}
