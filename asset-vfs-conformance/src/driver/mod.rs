//! Checks grouped by the driver interface under test.
//!
//! Each interface module defines its own `Fixture`; [`data`] owns shared tree
//! descriptions and the native setup adapter. A fixture can implement more
//! than one of these traits, but no interface requires another group's fixture.
//! Backend-specific cases such as symbolic links, native names that cannot be
//! represented, and injected I/O failures remain in the implementation's tests.
//! `WriteDriver` currently has no operations, so no write-operation suite is
//! defined. Writer exposure can be checked explicitly with
//! [`bound_driver::check_writer_presence`].

pub mod binding;
pub mod bound_driver;
pub mod data;
pub mod read_driver;

mod support;
