//! Wrap an existing math type without adding ID behavior to that type.
//!
//! VectorId accepts only components in [-100, 100] through TryFrom<Vector>.
//! Infallible conversions could use From instead; custom errors are needed only
//! for fallible conversions. No text format or generator is needed here.
//!
//! Shared Deref exposes public fields and &self methods. It does not forward
//! operators or change method return types: arithmetic returns Vector, which
//! must be validated again before becoming VectorId. AsRef is a separate option.
//! Shared access can still expose any interior mutability in a wrapped type.
//!
//! Keep validated input paths consistent. Generated parse and generate would
//! wrap the value directly, without calling TryFrom<Vector>, so this ID selects
//! only as_ref and deref. Its field remains private in the ID's defining module.
//!
//! Run from the ah-id source root (the default derive feature is sufficient):
//! ```sh
//! cargo run --example vector
//! ```

// Simulate an existing math library that defines vectors and their operations.
mod ah_example_vector {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Vector {
        pub x: i32,
        pub y: i32,
    }

    impl Vector {
        pub const fn new(x: i32, y: i32) -> Self {
            Self { x, y }
        }

        pub fn plus(&self, other: &Self) -> Self {
            Self::new(self.x + other.x, self.y + other.y)
        }

        pub fn minus(&self, other: &Self) -> Self {
            Self::new(self.x - other.x, self.y - other.y)
        }

        pub fn scaled(&self, factor: i32) -> Self {
            Self::new(self.x * factor, self.y * factor)
        }

        pub fn dot(&self, other: &Self) -> i128 {
            // Widen before multiplying so every dot product of i32 components fits.
            i128::from(self.x) * i128::from(other.x) + i128::from(self.y) * i128::from(other.y)
        }

        pub fn length_squared(&self) -> i128 {
            self.dot(self)
        }
    }
}

// Wrap Vector externally without changing it or adding ID-specific behavior.
mod ah_example_vector_id {
    use super::ah_example_vector::Vector;

    #[derive(ah_id::Id, Debug, Clone, Copy, PartialEq, Eq)]
    #[id(as_ref, deref)]
    pub struct VectorId(Vector);

    #[derive(Debug, PartialEq, Eq)]
    pub struct OutOfBounds;

    // Only vectors with both components in [-100, 100] are valid IDs here.
    impl TryFrom<Vector> for VectorId {
        type Error = OutOfBounds;

        fn try_from(vector: Vector) -> Result<Self, Self::Error> {
            if !(-100..=100).contains(&vector.x) || !(-100..=100).contains(&vector.y) {
                return Err(OutOfBounds);
            }
            Ok(Self(vector))
        }
    }
}

use ah_example_vector::Vector;
use ah_example_vector_id::VectorId;

fn main() -> Result<(), ah_example_vector_id::OutOfBounds> {
    let a = Vector::new(3, 4);
    let b = Vector::new(1, 2);
    assert_eq!(a.plus(&b), Vector::new(4, 6));
    assert_eq!(a.minus(&b), Vector::new(2, 2));
    assert_eq!(a.scaled(2), Vector::new(6, 8));

    let a_id = VectorId::try_from(a)?;
    let b_id = VectorId::try_from(b)?;

    // Deref exposes public fields and &self methods without handwritten forwarding.
    assert_eq!((a_id.x, a_id.y), (3, 4));
    assert_eq!(a_id.length_squared(), 25);
    assert_eq!(a_id.dot(&b_id), 11);
    assert_eq!(a_id.minus(&b_id), Vector::new(2, 2));
    assert_eq!(a_id.scaled(2), Vector::new(6, 8));

    // Vector methods still return vectors; converting a result to an ID reruns validation.
    let sum: Vector = a_id.plus(&b_id);
    let sum_id = VectorId::try_from(sum)?;
    assert_eq!(sum_id.as_ref(), &Vector::new(4, 6));

    let edge_id = VectorId::try_from(Vector::new(100, 100))?;
    let outside: Vector = edge_id.scaled(2);
    assert!(VectorId::try_from(outside).is_err());
    assert_eq!(edge_id.as_ref(), &Vector::new(100, 100));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_operations_obey_arithmetic_identities() {
        let vectors = [(0, 0), (1, 0), (0, -1), (3, 4), (-5, 12), (100, -100)];
        for (x, y) in vectors {
            let a = Vector::new(x, y);
            assert_eq!(a.scaled(0), Vector::new(0, 0));
            assert_eq!(a.scaled(1), a);
            for (x, y) in vectors {
                let b = Vector::new(x, y);
                assert_eq!(a.plus(&b), b.plus(&a));
                assert_eq!(a.plus(&b).minus(&b), a);
                assert_eq!(a.plus(&b).scaled(2), a.scaled(2).plus(&b.scaled(2)));
                assert_eq!(a.dot(&b), b.dot(&a));
            }
        }
    }

    #[test]
    fn squared_length_handles_extreme_components() {
        let vector = Vector::new(i32::MIN, i32::MIN);
        assert_eq!(vector.length_squared(), 1_i128 << 63);
        assert_eq!(vector.dot(&Vector::new(0, 0)), 0);
    }

    #[test]
    fn vector_inputs_enforce_id_bounds() {
        let components = [i32::MIN, -101, -100, -99, 0, 99, 100, 101, i32::MAX];
        for x in components {
            for y in components {
                let vector = VectorId::try_from(Vector::new(x, y));
                assert_eq!(
                    vector.is_ok(),
                    (-100..=100).contains(&x) && (-100..=100).contains(&y)
                );
            }
        }
    }

    #[test]
    fn vector_methods_do_not_bypass_id_constraints() -> Result<(), ah_example_vector_id::OutOfBounds>
    {
        let id = VectorId::try_from(Vector::new(100, -100))?;
        let offset = VectorId::try_from(Vector::new(1, -1))?;
        assert!(VectorId::try_from(id.plus(&offset)).is_err());
        assert!(VectorId::try_from(id.scaled(2)).is_err());
        assert_eq!((id.x, id.y), (100, -100));
        assert_eq!(id.length_squared(), 20_000);
        Ok(())
    }

    #[test]
    fn example_uses_vector_methods_through_ids() -> Result<(), ah_example_vector_id::OutOfBounds> {
        main()
    }
}
