/// The optional write interface of a bound backend.
///
/// This trait currently reserves the capability entry point without declaring
/// write operations. Those operations will be defined when their contracts are
/// introduced. Any future operations must remain inside the bound root.
pub trait WriteDriver: Send + Sync {}
