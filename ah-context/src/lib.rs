#![doc = include_str!("../README.md")]

extern crate self as ah_context;

mod auto;
mod builder;
mod context;
mod error;
mod global;

/// Automatically registers a concrete struct, preserving its fields and visibility.
///
/// Default construction requires `Default`. For fallible initialization, use
/// `init = path` naming a zero-argument function returning `Result<Self, E>`,
/// where `E: Into<Box<dyn std::error::Error + Send + Sync>>`.
/// String messages and typed or boxed errors are preserved as the error source.
///
/// ```
/// #[ah_context::context(init = build)]
/// struct Catalog { label: String }
///
/// fn build() -> Result<Catalog, std::io::Error> {
///     Ok(Catalog { label: "custom".into() })
/// }
/// ah_context::load()?;
/// assert_eq!(ah_context::get::<Catalog>()?.label, "custom");
/// # Ok::<(), ah_context::ContextError>(())
/// ```
///
/// The macro requires a concrete, non-generic struct:
///
/// ```compile_fail
/// #[ah_context::context()]
/// #[derive(Default)]
/// struct Generic<T> { value: T }
/// ```
///
/// Default construction requires an actual `Default` implementation:
///
/// ```compile_fail
/// #[ah_context::context()]
/// struct MissingDefault;
/// ```
///
/// Shared values must be thread-safe:
///
/// ```compile_fail
/// #[ah_context::context()]
/// #[derive(Default)]
/// struct NotSync { value: std::cell::Cell<u32> }
/// ```
pub use ah_context_macros::context;

/// Registers a concrete type from the calling module, including external types.
///
/// The defining crate does not need to depend on `ah-context`. Place the declaration
/// at module scope in the runtime. `register!(Type)` uses `Default`, while
/// `register!(Type, init = path)` uses a fallible constructor. Both share the same
/// construction requirements and registration set as [`context`].
///
/// ```
/// mod vfs_core {
///     pub struct DriverKinds(pub Vec<&'static str>);
/// }
///
/// ah_context::register!(vfs_core::DriverKinds, init = build_driver_kinds);
///
/// fn build_driver_kinds() -> Result<vfs_core::DriverKinds, std::io::Error> {
///     Ok(vfs_core::DriverKinds(vec!["local", "memory"]))
/// }
///
/// ah_context::load()?;
/// assert_eq!(ah_context::get::<vfs_core::DriverKinds>()?.0, ["local", "memory"]);
/// # Ok::<(), ah_context::ContextError>(())
/// ```
pub use ah_context_macros::register;
pub use builder::ContextBuilder;
pub use context::Context;
pub use error::ContextError;
pub use global::{get, load, load_with};

#[doc(hidden)]
pub mod __private {
    pub use crate::auto::{AH_CONTEXT_AUTO_REGISTRATIONS, AutoRegistration};
    pub use linkme;
}
