use std::{fmt, str::FromStr};
use thiserror::Error;

/// Binary capacity units. `B` is one byte; each subsequent unit multiplies by 1024.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityUnit {
    B,
    KiB,
    MiB,
    GiB,
    TiB,
}

impl CapacityUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::B => "B",
            Self::KiB => "KiB",
            Self::MiB => "MiB",
            Self::GiB => "GiB",
            Self::TiB => "TiB",
        }
    }

    const fn multiplier(self) -> u64 {
        match self {
            Self::B => 1,
            Self::KiB => 1 << 10,
            Self::MiB => 1 << 20,
            Self::GiB => 1 << 30,
            Self::TiB => 1 << 40,
        }
    }
}

impl fmt::Display for CapacityUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CapacityUnit {
    type Err = CapacityError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "B" => Ok(Self::B),
            "KiB" => Ok(Self::KiB),
            "MiB" => Ok(Self::MiB),
            "GiB" => Ok(Self::GiB),
            "TiB" => Ok(Self::TiB),
            _ => Err(CapacityError::InvalidUnit),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CapacityError {
    #[error("capacity size must be positive")]
    Zero,
    #[error("capacity exceeds the supported numeric range")]
    Overflow,
    #[error("capacity unit must be B, KiB, MiB, GiB or TiB")]
    InvalidUnit,
}

/// Validated instance payload quota, retaining the original size and unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capacity {
    size: u64,
    unit: CapacityUnit,
    bytes: u64,
}

impl Capacity {
    /// Reject zero, unsupported numeric ranges and byte-conversion overflow.
    pub fn new(size: u64, unit: CapacityUnit) -> Result<Self, CapacityError> {
        if size == 0 {
            return Err(CapacityError::Zero);
        }
        i64::try_from(size).map_err(|_| CapacityError::Overflow)?;
        let bytes = size
            .checked_mul(unit.multiplier())
            .ok_or(CapacityError::Overflow)?;
        Ok(Self { size, unit, bytes })
    }

    pub fn size(self) -> u64 {
        self.size
    }
    pub fn unit(self) -> CapacityUnit {
        self.unit
    }
    pub fn bytes(self) -> u64 {
        self.bytes
    }
}
