//! Compile the storage contract without namespace or shared driver modules.
//! Storage values and operations must be usable without the host domain model.

// Resolve the production crate name to this minimal target as well.
extern crate self as ah_vfs;

#[allow(dead_code, unused_imports)]
#[path = "../src/domain"]
mod domain {
    pub mod storage;
}
