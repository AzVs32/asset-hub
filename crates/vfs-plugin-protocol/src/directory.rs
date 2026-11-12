//! Directory selection and pages encoded on the formal plugin protocol.
use super::{DirectoryCheckpoint, DirectorySelection, Empty, List, Page, Reply, Value, page};
use ah_vfs_storage as d;
use prost::Message;

const INVALID: d::ContractViolation = d::ContractViolation::InvalidPage;
// A bounded path plus fixed-width metadata and protobuf field framing.
const MAX_ENTRY_BYTES: usize = d::MAX_PATH_BYTES + 64;

pub fn encode_selection(selection: &d::DirectorySelection) -> DirectorySelection {
    DirectorySelection {
        address: selection.address().as_str().into(),
        identity: selection.identity().as_bytes().to_vec(),
    }
}

pub fn decode_selection(
    value: DirectorySelection,
) -> Result<d::DirectorySelection, d::ContractViolation> {
    let address = d::StoragePath::parse(&value.address).map_err(|_| INVALID)?;
    let identity = value.identity.as_slice().try_into().map_err(|_| INVALID)?;
    Ok(d::DirectorySelection::from_lookup(
        address,
        d::DirectoryIdentity::from_bytes(identity),
    ))
}

fn decode_size(value: u32) -> Result<d::PageSize, d::ContractViolation> {
    d::PageSize::new(value as usize).map_err(|_| INVALID)
}

fn decode_cursor(
    selection: d::DirectorySelection,
    after: &str,
) -> Result<d::DirectoryCursor, d::ContractViolation> {
    let after = d::EntryName::parse(after).map_err(|_| INVALID)?;
    d::DirectoryCursor::from_checkpoint(selection, after).map_err(|_| INVALID)
}

pub fn encode_checkpoint(cursor: &d::DirectoryCursor) -> DirectoryCheckpoint {
    DirectoryCheckpoint {
        selection: Some(encode_selection(cursor.selection())),
        after: cursor.after().as_str().into(),
    }
}

pub fn decode_checkpoint(
    checkpoint: DirectoryCheckpoint,
) -> Result<d::DirectoryCursor, d::ContractViolation> {
    let selection = decode_selection(checkpoint.selection.ok_or(INVALID)?)?;
    decode_cursor(selection, &checkpoint.after)
}

pub fn encode_list(bound: &d::BoundListRequest) -> List {
    List {
        selection: Some(encode_selection(bound.selection())),
        after: bound.after().map(|name| name.as_str().into()),
        size: bound.size().get() as u32,
    }
}

/// The caller must independently verify the requested binding against its
/// live storage before enumerating.
pub fn decode_list(
    value: List,
) -> Result<(d::ListRequest, d::DirectorySelection), d::ContractViolation> {
    let size = decode_size(value.size)?;
    let selection = decode_selection(value.selection.ok_or(INVALID)?)?;
    let request = match value.after {
        Some(after) => d::ListRequest::continuing(decode_cursor(selection.clone(), &after)?, size),
        None => d::ListRequest::new(selection.address().clone(), size),
    };
    Ok((request, selection))
}

pub fn encode_page(value: d::DirectoryPage) -> Result<Reply, d::ContractViolation> {
    let (selection, entries, next) = value.into_parts();
    let mut data = Vec::new();
    for entry in entries {
        let entry = super::encode_entry(&entry).map_err(|_| INVALID)?;
        if entry.encoded_len() + 10 > super::MAX_DATA_BYTES.saturating_sub(data.len()) {
            return Err(INVALID);
        }
        entry
            .encode_length_delimited(&mut data)
            .map_err(|_| INVALID)?;
    }
    let completion = match next {
        Some(name) => page::Completion::After(name.as_str().into()),
        None => page::Completion::End(Empty {}),
    };
    Ok(Reply {
        result: Some(Value::Page(Page {
            reported: Some(encode_selection(&selection)),
            completion: Some(completion),
        })),
        data,
    })
}

pub fn decode_page(
    bound: &d::BoundListRequest,
    value: Page,
    mut data: &[u8],
) -> Result<d::DirectoryPage, d::ContractViolation> {
    if data.len() > super::MAX_DATA_BYTES {
        return Err(INVALID);
    }
    let reported = decode_selection(value.reported.ok_or(INVALID)?)?;
    if &reported != bound.selection() {
        return Err(INVALID);
    }
    let next = match value.completion.ok_or(INVALID)? {
        page::Completion::End(_) => None,
        page::Completion::After(name) => Some(d::EntryName::parse(&name).map_err(|_| INVALID)?),
    };
    let mut entries = Vec::new();
    while !data.is_empty() {
        if entries.len() == bound.size().get() {
            return Err(INVALID);
        }
        let size = prost::decode_length_delimiter(&mut data).map_err(|_| INVALID)?;
        if size > MAX_ENTRY_BYTES || size > data.len() {
            return Err(INVALID);
        }
        let (entry, remaining) = data.split_at(size);
        let entry = super::Entry::decode(entry).map_err(|_| INVALID)?;
        data = remaining;
        entries.push(super::decode_entry(entry).map_err(|_| INVALID)?);
    }
    d::DirectoryPage::from_parts(
        bound,
        reported.address(),
        reported.identity(),
        entries,
        next,
    )
    .map_err(|_| INVALID)
}
