//! Compile the production domain without application or infrastructure modules.
//! Any dependency on those crate modules must fail to compile in this target.

// Resolve the production crate name to this minimal target as well.
extern crate self as ah_vfs;

#[allow(dead_code, unused_imports)]
#[path = "../src"]
mod production {
    pub mod domain;
}
use production::domain;
