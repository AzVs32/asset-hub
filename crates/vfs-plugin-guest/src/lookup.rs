use crate::api::{ErrorContext, Result};
use ah_vfs_storage::{
    ContractViolation, Entry, EntryType, Failure, Operation, Rejection, StoragePath,
};

/// Scope-local observations for addressable namespaces. The version value
/// identifies the selected object and can be used for a conditional effect.
pub struct DirectBindings<T> {
    context: ErrorContext,
    observations: Vec<(Entry, T)>,
    missing: std::collections::BTreeSet<StoragePath>,
}

impl<T: PartialEq> DirectBindings<T> {
    pub fn new(operation: Operation) -> Self {
        Self {
            context: ErrorContext::storage(operation),
            observations: Vec::new(),
            missing: Default::default(),
        }
    }

    pub fn get(&self, id: u64) -> Result<(&Entry, &T)> {
        let index = id.checked_sub(1).and_then(|id| usize::try_from(id).ok());
        index
            .and_then(|index| self.observations.get(index))
            .map(|(entry, value)| (entry, value))
            .ok_or_else(|| {
                self.context
                    .failure(Failure::Backend(ah_vfs_storage::BackendFailure::Stale))
            })
    }

    pub fn child_path(&self, parent: u64, name: &ah_vfs_storage::EntryName) -> Result<StoragePath> {
        let (parent, _) = self.get(parent)?;
        if parent.entry_type() != EntryType::Directory {
            return Err(self
                .context
                .failure(Failure::Rejected(Rejection::NotDirectory)));
        }
        parent.path().join(name).map_err(|_| {
            self.context
                .failure(Failure::Rejected(Rejection::PathTooLong))
        })
    }

    pub fn observe(
        &mut self,
        address: StoragePath,
        value: Result<(Entry, T)>,
    ) -> Result<(u64, Entry)> {
        let previous = self
            .observations
            .iter()
            .position(|(entry, _)| entry.path() == &address);
        let conflict = || self.context.failure(Failure::Rejected(Rejection::Conflict));
        match value {
            Ok((entry, _)) if entry.path() != &address => Err(self
                .context
                .failure(Failure::Contract(ContractViolation::InvalidEntry))),
            Ok((entry, value)) => {
                if self.missing.contains(&address) {
                    return Err(conflict());
                }
                if let Some(index) = previous {
                    let (expected, version) = &self.observations[index];
                    if expected.entry_type() != entry.entry_type() || *version != value {
                        return Err(conflict());
                    }
                    return Ok(((index + 1) as u64, entry));
                }
                self.observations.push((entry.clone(), value));
                Ok((self.observations.len() as u64, entry))
            }
            Err(error) if is_missing(&error) => {
                if previous.is_some() {
                    return Err(conflict());
                }
                self.missing.insert(address);
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub fn record_creation(
        &mut self,
        address: &StoragePath,
        value: (Entry, T),
    ) -> Result<(u64, Entry)> {
        self.missing.remove(address);
        self.observe(address.clone(), Ok(value))
    }

    pub fn allow_reobserve(&mut self, address: &StoragePath) {
        self.missing.remove(address);
    }

    pub fn slot(
        &self,
        slot: crate::api::EntrySlot<'_>,
        address: &StoragePath,
    ) -> Result<(&T, Option<(&Entry, &T)>)> {
        let invalid = || self.context.failure(Failure::InvalidBinding);
        let (_, parent) = self.get(slot.parent)?;
        let entry = slot.entry.map(|id| self.get(id)).transpose()?;
        if &self.child_path(slot.parent, slot.name)? != address
            || entry.is_some_and(|(entry, _)| entry.path() != address)
            || (entry.is_none() && !self.missing.contains(address))
        {
            return Err(invalid());
        }
        Ok((parent, entry))
    }

    pub fn verify(&self, mut load: impl FnMut(&StoragePath) -> Result<(Entry, T)>) -> Result<()> {
        let conflict = || self.context.failure(Failure::Rejected(Rejection::Conflict));
        for (entry, version) in &self.observations {
            let (current, current_version) = load(entry.path()).map_err(|error| {
                if is_missing(&error) {
                    conflict()
                } else {
                    error
                }
            })?;
            if entry.path() != current.path()
                || entry.entry_type() != current.entry_type()
                || *version != current_version
            {
                return Err(conflict());
            }
        }
        Ok(())
    }
}

fn is_missing(error: &crate::api::Failure) -> bool {
    error
        .storage_error()
        .is_some_and(|error| error.failure() == Failure::Rejected(Rejection::NotFound))
}
