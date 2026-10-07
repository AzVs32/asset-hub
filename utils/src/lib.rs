mod gen_id;

#[cfg(feature = "nanoid")]
pub use gen_id::NanoID;
pub use gen_id::ParseIdError;
#[cfg(feature = "ulid")]
pub use gen_id::ULID;
#[cfg(feature = "uuid_v7")]
pub use gen_id::UUID_v7;

#[doc(hidden)]
pub use gen_id::strategy as __private;
