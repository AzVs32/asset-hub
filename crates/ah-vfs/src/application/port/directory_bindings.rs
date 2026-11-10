//! Bounded directory bindings for backends that pin native directory handles.
use ah_vfs_storage::{
    BackendFailure, DirectoryIdentity, DirectorySelection, Failure, Operation, StorageError,
    StoragePath,
};
use std::{collections::VecDeque, sync::Mutex};

struct Binding<H> {
    selection: DirectorySelection,
    handle: H,
}

/// Keep native directory objects alive while their identities can be resumed.
/// Each opened instance owns its registry. Eviction expires the least recently
/// used binding and any outstanding cursors for it.
pub struct DirectoryBindings<H> {
    bindings: Mutex<VecDeque<Binding<H>>>,
}

impl<H> Default for DirectoryBindings<H> {
    fn default() -> Self {
        Self {
            bindings: Mutex::new(VecDeque::new()),
        }
    }
}

impl<H> DirectoryBindings<H> {
    const CAPACITY: usize = 256;

    pub fn bind(
        &self,
        address: StoragePath,
        handle: H,
        same: impl FnOnce(&H, &H) -> Result<bool, StorageError>,
    ) -> Result<DirectorySelection, StorageError> {
        let mut bindings = self.bindings.lock().map_err(|_| lock_error())?;
        let identity = if let Some(index) = bindings
            .iter()
            .position(|item| item.selection.address() == &address)
        {
            let previous = &bindings[index];
            let identity = same(&previous.handle, &handle)?.then(|| previous.selection.identity());
            bindings.remove(index);
            identity
        } else {
            None
        };
        let selection = DirectorySelection::from_lookup(
            address,
            identity.unwrap_or_else(|| {
                DirectoryIdentity::from_bytes(*ah_id::UuidV7::new().as_ref().as_bytes())
            }),
        );
        if bindings.len() == Self::CAPACITY {
            bindings.pop_front();
        }
        bindings.push_back(Binding {
            selection: selection.clone(),
            handle,
        });
        Ok(selection)
    }

    pub fn verify(
        &self,
        selection: &DirectorySelection,
        handle: &H,
        same: impl FnOnce(&H, &H) -> Result<bool, StorageError>,
    ) -> Result<(), StorageError> {
        let mut bindings = self.bindings.lock().map_err(|_| lock_error())?;
        if let Some(index) = bindings
            .iter()
            .position(|item| &item.selection == selection)
        {
            let matches = same(&bindings[index].handle, handle)?;
            if let Some(binding) = bindings.remove(index)
                && matches
            {
                bindings.push_back(binding);
                return Ok(());
            }
        }
        Err(StorageError::without_effects(
            Operation::List,
            Failure::Backend(BackendFailure::Stale),
            None,
        ))
    }
}

fn lock_error() -> StorageError {
    StorageError::without_effects(
        Operation::List,
        Failure::Backend(BackendFailure::Other),
        None,
    )
}
