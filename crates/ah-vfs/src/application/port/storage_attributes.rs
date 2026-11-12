//! Optional typed storage attributes, independent of file formats.
use ah_vfs_storage::AttributeSupport;

pub trait StorageAttributes: Send + Sync {
    /// Declared permissions and xattr support, respectively. The values must
    /// match DriverInfo and do not grant access to any individual entry.
    fn support(&self) -> (AttributeSupport, AttributeSupport);
}
