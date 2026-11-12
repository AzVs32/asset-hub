//! Decode canonical execution payloads separately from scope-owned observations.
use crate::{BoundCall, BoundOp, BoundSlot, BoundTarget, EntrySlot};
use ah_vfs_storage as d;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExecutionError {
    #[error("missing operation or binding reference")]
    Missing,
    #[error("invalid operation parameter or address")]
    Policy,
    #[error("invalid read range")]
    Range(#[from] d::RangeError),
}

use ExecutionError as Error;

pub struct Slot {
    pub parent: u64,
    pub name: d::EntryName,
    pub path: d::StoragePath,
    pub entry: Option<u64>,
}

impl Slot {
    fn decode(slot: EntrySlot, entry: Option<u64>) -> Result<Self, Error> {
        if slot.parent == 0 || entry == Some(0) {
            return Err(Error::Missing);
        }
        let (name, path) = crate::decode_entry_slot(&slot).map_err(|_| Error::Policy)?;
        Ok(Self {
            parent: slot.parent,
            name,
            path,
            entry,
        })
    }
}

impl BoundSlot {
    fn decode(self) -> Result<Slot, Error> {
        Slot::decode(self.slot.ok_or(Error::Missing)?, self.entry)
    }
}

pub struct Target {
    pub slot: Option<Slot>,
    pub entry: u64,
}

impl Target {
    pub fn path(&self) -> d::StoragePath {
        self.slot
            .as_ref()
            .map(|slot| slot.path.clone())
            .unwrap_or_default()
    }
}

impl BoundTarget {
    fn decode(self) -> Result<Target, Error> {
        if self.entry == 0 {
            return Err(Error::Missing);
        }
        Ok(Target {
            slot: self
                .slot
                .map(|slot| Slot::decode(slot, Some(self.entry)))
                .transpose()?,
            entry: self.entry,
        })
    }
}

pub enum Execution {
    Stat(Target, d::StatRequest),
    Read(Slot, d::ReadRequest),
    Write(Slot, d::WriteRequest),
    CreateDirectory(Slot, d::CreateDirectoryRequest),
    Remove(Slot, d::RemoveRequest),
    Rename(Slot, Slot, d::RenameRequest),
    SelectDirectory(Target, d::ListRequest),
}

impl Execution {
    pub fn operation(&self) -> d::Operation {
        match self {
            Self::Stat(..) => d::Operation::Stat,
            Self::Read(..) => d::Operation::Read,
            Self::Write(..) => d::Operation::Write,
            Self::CreateDirectory(..) => d::Operation::CreateDirectory,
            Self::Remove(..) => d::Operation::Remove,
            Self::Rename(..) => d::Operation::Rename,
            Self::SelectDirectory(..) => d::Operation::List,
        }
    }
}

pub fn decode_bound_call(call: BoundCall) -> Result<(u64, Execution), Error> {
    if call.lookup == 0 {
        return Err(Error::Missing);
    }
    let execution = match call.operation.ok_or(Error::Missing)? {
        BoundOp::Stat(target) => {
            let target = target.decode()?;
            let request = d::StatRequest::new(target.path());
            Execution::Stat(target, request)
        }
        BoundOp::Read(wire) => {
            let slot = wire.target.ok_or(Error::Missing)?.decode()?;
            let range = d::ReadRange::new(wire.offset, wire.length)?;
            let request = d::ReadRequest::new(slot.path.clone(), range);
            Execution::Read(slot, request)
        }
        BoundOp::Write(wire) => {
            let slot = wire.target.ok_or(Error::Missing)?.decode()?;
            let (mode, publication) =
                crate::lookup::decode_write_policy(wire.mode, wire.publication)?;
            let request = d::WriteRequest::new(slot.path.clone(), mode, publication)
                .map_err(|_| Error::Policy)?;
            Execution::Write(slot, request)
        }
        BoundOp::CreateDirectory(target) => {
            let slot = target.decode()?;
            let request =
                d::CreateDirectoryRequest::new(slot.path.clone()).map_err(|_| Error::Policy)?;
            Execution::CreateDirectory(slot, request)
        }
        BoundOp::Remove(wire) => {
            let slot = wire.target.ok_or(Error::Missing)?.decode()?;
            let kind = crate::lookup::decode_remove_kind(wire.kind)?;
            let request =
                d::RemoveRequest::new(slot.path.clone(), kind).map_err(|_| Error::Policy)?;
            Execution::Remove(slot, request)
        }
        BoundOp::Rename(wire) => {
            let source = wire.source.ok_or(Error::Missing)?.decode()?;
            let destination = wire.destination.ok_or(Error::Missing)?.decode()?;
            let mode = crate::lookup::decode_rename_mode(wire.mode)?;
            let request =
                d::RenameRequest::new(source.path.clone(), destination.path.clone(), mode)
                    .map_err(|_| Error::Policy)?;
            Execution::Rename(source, destination, request)
        }
        BoundOp::SelectDirectory(wire) => {
            let target = wire.target.ok_or(Error::Missing)?.decode()?;
            let size = d::PageSize::new(wire.size as usize).map_err(|_| Error::Policy)?;
            let request = d::ListRequest::new(target.path(), size);
            Execution::SelectDirectory(target, request)
        }
    };
    Ok((call.lookup, execution))
}
