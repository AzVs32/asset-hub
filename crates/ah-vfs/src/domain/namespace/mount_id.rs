/// Stable mount identity, generated as UUID v7 and restored through parsing.
///
/// Hashing and ordering use the underlying UUID value. Ordering does not promise
/// a strict chronological order of mount creation.
#[derive(ah_id::Id, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[id(generate, display, parse)]
pub struct MountId(ah_id::UuidV7);
