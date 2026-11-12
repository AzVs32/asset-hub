//! Host-side operations which resolve operands before dispatching to a storage port.
use super::{
    self as resolve,
    operand::{
        CreateDirectoryOperand, ListOperand, ListOperandTarget, ReadOperand, RemoveOperand,
        RenameOperand, StatOperand, WriteOperand,
    },
    policy::IntermediateSymlinks,
};
use crate::application::port::{Storage, StorageFileReader, StorageFileWriter, StorageFuture};
use ah_vfs_storage::{
    DirectoryPage, DirectorySelection, Entry, Failure, Operation, StorageError, StoragePath,
};

/// Per-call inputs for resolving pathname operands on the opened storage instance.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LookupContext {
    /// An instance-relative base address for relative operands. Each lookup
    /// resolves it from the instance root. A later call
    /// may select a different directory at the same address. Absolute operands
    /// start at the root.
    pub working_directory: StoragePath,
    /// The policy for links before the final component of an operand.
    pub intermediate_symlinks: IntermediateSymlinks,
}

/// VFS operations on one opened backend namespace. The resolver owns lookup
/// policy and sends the backend canonical requests with its own observations.
pub trait StorageOperations: Storage {
    /// Retrieve metadata for the entry selected by the request's lookup policy.
    fn stat_entry(&self, request: StatOperand, context: LookupContext) -> StorageFuture<'_, Entry> {
        Box::pin(resolve::stat(self, request, context))
    }

    /// Resolve the initial operand or independently verify the cursor's directory.
    fn select_directory<'a>(
        &'a self,
        request: &'a ListOperand,
        context: LookupContext,
    ) -> StorageFuture<'a, DirectorySelection> {
        Box::pin(async move {
            match request.target() {
                ListOperandTarget::Path(_) => {
                    resolve::select_directory(self, request, context).await
                }
                ListOperandTarget::Continue(cursor) => self.resume_directory(cursor).await,
            }
        })
    }

    /// Select a directory separately before enumerating a page.
    fn list_entries<'a>(
        &'a self,
        request: &'a ListOperand,
        context: LookupContext,
    ) -> StorageFuture<'a, DirectoryPage> {
        Box::pin(async move {
            let selection = self.select_directory(request, context).await?;
            let bound = request.bind_selection(selection).map_err(|reason| {
                StorageError::without_effects(Operation::List, Failure::Backend(reason), None)
            })?;
            self.list_page(&bound).await
        })
    }

    /// Resolve the request once and open a range-limited stream bound to the selected file.
    /// Check capabilities before acquiring a reader. Empty ranges and offsets at or
    /// past EOF still perform lookup and return a stream at EOF for a regular file.
    fn read_file(
        &self,
        request: ReadOperand,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        Box::pin(resolve::read(self, request, context))
    }

    /// Stage a complete-file write. The target changes only when commit publishes it.
    fn write_file(
        &self,
        request: WriteOperand,
        context: LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        Box::pin(resolve::write(self, request, context))
    }

    /// Create a directory in the backend.
    fn create_dir(
        &self,
        request: CreateDirectoryOperand,
        context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        Box::pin(resolve::create_directory(self, request, context))
    }

    /// Remove one entry of the requested kind, preserving the final link.
    fn remove_entry(
        &self,
        request: RemoveOperand,
        context: LookupContext,
    ) -> StorageFuture<'_, ()> {
        Box::pin(resolve::remove(self, request, context))
    }

    /// Resolve both operands in one instance and perform a single atomic rename.
    fn move_entry(
        &self,
        request: RenameOperand,
        context: LookupContext,
    ) -> StorageFuture<'_, Entry> {
        Box::pin(resolve::rename(self, request, context))
    }
}

impl<T: Storage + ?Sized> StorageOperations for T {}
