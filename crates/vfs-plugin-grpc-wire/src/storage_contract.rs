//! Protobuf boundary for the memory-backed storage contract session.
tonic::include_proto!("assethub.storage.v2");

mod conversion;
pub use conversion::*;

pub const MAX_FRAME_BYTES: usize = 320 * 1024;
