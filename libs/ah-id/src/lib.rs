//! Strongly typed identifiers independent of application and storage concerns.
//!
//! Built-in value types are enabled separately through `uuid_v7`, `ulid` and
//! `nanoid` features. The `derive` feature is enabled by default; disable default
//! features to use values without the derive implementation.
//!
//! ```
//! # #[cfg(all(feature = "derive", feature = "uuid_v7"))]
//! # {
//! #[derive(ah_id::Id, Debug, Clone, Copy, PartialEq, Eq)]
//! #[id(generate, display, parse)]
//! pub struct MountId(ah_id::UuidV7);
//!
//! let id = MountId::new();
//! let restored: MountId = id.to_string().parse()?;
//! assert_eq!(restored, id);
//! # }
//! # Ok::<(), ah_id::InvalidIdError>(())
//! ```

extern crate self as ah_id;

mod error;
mod generate;

#[cfg(feature = "nanoid")]
mod nanoid;
#[cfg(feature = "ulid")]
mod ulid;
#[cfg(feature = "uuid_v7")]
mod uuid_v7;

pub use error::InvalidIdError;
pub use generate::Generate;

/// Derive selected ID capabilities for a newtype.
///
/// Requires one private tuple field and explicit capability flags:
/// - `generate`: `new()` and [`Generate`], delegated to the field's `Generate`.
/// - `display`: [`std::fmt::Display`], delegated to the field's display.
/// - `parse`: `FromStr` and fallible text conversions, delegated to the field's
///   parser with its original error type.
/// - `as_ref`: explicit shared access through `AsRef<Field>`.
/// - `deref`: shared `Deref`, including automatic field and method access.
///
/// Shared access does not prevent interior mutability supplied by the field.
/// These flags are independent; `deref` does not generate `AsRef`.
///
/// Generation and parsing wrap an already valid field value. If the ID has
/// extra rules, enforce them in the field type or implement the corresponding
/// constructors and conversions manually instead of selecting those flags.
///
/// Does not change the struct or add other derives, raw conversions, mutable
/// accessors or a default identity.
#[cfg(feature = "derive")]
pub use ah_id_macros::Id;

#[cfg(feature = "nanoid")]
pub use self::nanoid::NanoId;
#[cfg(feature = "ulid")]
pub use self::ulid::Ulid;
#[cfg(feature = "uuid_v7")]
pub use uuid_v7::UuidV7;
