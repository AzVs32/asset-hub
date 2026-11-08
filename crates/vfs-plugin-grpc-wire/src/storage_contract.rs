//! Protobuf boundary for the memory-backed storage contract session.
tonic::include_proto!("assethub.storage.v1");

mod conversion;
pub use conversion::*;

pub const MAX_PAGE_ENTRIES: usize = 64;
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_FRAME_BYTES: usize = 320 * 1024;
