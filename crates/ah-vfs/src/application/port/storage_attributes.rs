//! Optional typed storage attributes, independent of file formats.
use super::{LookupContext, StorageFuture};
use ah_vfs_storage::{AttributeRequest, AttributeResult, AttributeSupport};

pub trait StorageAttributes: Send + Sync {
    /// Declared permissions and xattr support, respectively. The values must
    /// match DriverInfo and do not grant access to any individual entry.
    fn support(&self) -> (AttributeSupport, AttributeSupport);

    /// Resolve the bound operand, preserve its final link and apply one action.
    /// Return a complete result within the request bounds. Xattr writes must
    /// enforce the requested existence policy atomically.
    fn execute(
        &self,
        request: AttributeRequest,
        context: LookupContext,
    ) -> StorageFuture<'_, AttributeResult>;
}
