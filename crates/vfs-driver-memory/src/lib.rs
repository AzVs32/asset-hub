//! Host-managed settings with volatile, capacity-limited file contents.

mod capacity;
mod driver;
mod error;
mod storage;

pub use capacity::{Capacity, CapacityError, CapacityUnit};
pub use driver::MemoryDriver;
pub use error::MemoryDriverError;
pub use storage::MemoryStorage;
