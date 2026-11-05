//! Compile only the new storage contract, with no legacy storage, namespace,
//! application, infrastructure or protocol modules available for imports.
extern crate self as ah_vfs;

#[allow(dead_code, unused_imports)]
#[path = "../src/domain"]
mod domain {
    pub mod storage2;
}
