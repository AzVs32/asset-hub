/// Stable mount identity, generated as UUID v7 and restored through parsing.
#[derive(ah_id::Id, Debug, Clone, Copy, PartialEq, Eq)]
#[id(generate, display, parse)]
pub struct MountId(ah_id::UuidV7);
