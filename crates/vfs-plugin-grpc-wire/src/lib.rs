//! gRPC services generated from the shared plugin schema; messages are defined once.
pub use vfs_plugin_protocol::wire::*;
tonic::include_proto!("assethub.vfs.v4");

#[cfg(feature = "storage-contract")]
pub mod storage_contract;
