//! Virtual filesystem domain types, extension ports, and application services.
//!
//! Runtimes supply implementations of [`driver::Driver`] and
//! [`mount::MountRepository`]. Concrete drivers, database selection, local
//! directory creation, and configuration loading belong to the runtime/infra
//! layers. Driver registration is private to [`DriverService`].

pub mod driver;
pub mod entry;
pub mod error;
pub mod mount;
pub mod namespace;

mod service;

pub use service::{DriverService, MountService, VfsService};
