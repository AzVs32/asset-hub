//! Virtual paths, mount rules and application boundaries for Asset Hub's VFS.

pub mod application;
pub mod config;
pub mod domain;
pub mod infrastructure;

#[cfg(test)]
extern crate self as ah_vfs;

#[cfg(feature = "testing")]
pub mod testing;
