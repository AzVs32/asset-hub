use vfs_driver_memory::{Capacity, CapacityError, CapacityUnit};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn units_round_trip_without_losing_original_size() -> TestResult {
    for (unit, multiplier) in [
        (CapacityUnit::B, 1),
        (CapacityUnit::KiB, 1 << 10),
        (CapacityUnit::MiB, 1 << 20),
        (CapacityUnit::GiB, 1 << 30),
        (CapacityUnit::TiB, 1 << 40),
    ] {
        let capacity = Capacity::new(3, unit)?;
        assert_eq!(capacity.size(), 3);
        assert_eq!(capacity.unit(), unit);
        assert_eq!(capacity.bytes(), 3 * multiplier);
        assert_eq!(unit.as_str().parse::<CapacityUnit>()?, unit);
        assert_eq!(unit.to_string(), unit.as_str());
    }
    Ok(())
}

#[test]
fn invalid_values_never_construct_capacity() -> TestResult {
    assert_eq!(Capacity::new(0, CapacityUnit::B), Err(CapacityError::Zero));
    assert_eq!(
        Capacity::new(u64::MAX, CapacityUnit::B),
        Err(CapacityError::Overflow)
    );
    assert_eq!(
        Capacity::new(i64::MAX as u64, CapacityUnit::TiB),
        Err(CapacityError::Overflow)
    );
    for input in ["", "KB", "mb", " MiB", "bytes"] {
        assert_eq!(
            input.parse::<CapacityUnit>(),
            Err(CapacityError::InvalidUnit)
        );
    }
    assert_eq!(
        Capacity::new(i64::MAX as u64, CapacityUnit::B)?.bytes(),
        i64::MAX as u64
    );
    Ok(())
}
