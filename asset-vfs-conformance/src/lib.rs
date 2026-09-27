//! Reusable behavioral checks for implementations of the `asset-vfs` contracts.
//!
//! Add this crate as a development dependency. Each interface has its own
//! fixture contract and ordinary assertion functions; callers explicitly choose
//! which checks to run in their `#[test]` functions. No test-generation macros
//! or automatic capability discovery are provided.
//!
//! Shared trees and native setup adapters live in [`driver::data`].
//!
//! Fixtures own their native storage and any temporary resources. Create a new
//! fixture for each test, keep it alive throughout the check, and do not mutate
//! its storage concurrently with a check. Separate fixtures must not interfere
//! with each other's storage.

pub mod driver;
