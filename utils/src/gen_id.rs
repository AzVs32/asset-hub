mod error;
mod macros;
pub mod strategy;

#[cfg(feature = "nanoid")]
mod nanoid;
#[cfg(feature = "ulid")]
mod ulid;
#[cfg(feature = "uuid_v7")]
mod uuid_v7;

pub use error::ParseIdError;
#[cfg(feature = "nanoid")]
pub use nanoid::NanoID;
#[cfg(feature = "ulid")]
pub use ulid::ULID;
#[cfg(feature = "uuid_v7")]
pub use uuid_v7::UUID_v7;
