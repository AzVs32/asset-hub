use super::*;
use ah_vfs_storage as d;
use prost::Message;
pub fn encode_entry(entry: d::Entry) -> Result<vfs_plugin_protocol::Entry, d::ContractViolation> {
    vfs_plugin_protocol::encode_entry(&entry).map_err(|_| d::ContractViolation::InvalidEntry)
}
pub fn decode_entry(entry: vfs_plugin_protocol::Entry) -> Result<d::Entry, d::ContractViolation> {
    vfs_plugin_protocol::decode_entry(entry).map_err(|_| d::ContractViolation::InvalidEntry)
}

pub fn encode_selection(selection: &d::DirectorySelection) -> Selection {
    Selection {
        address: selection.address().as_str().into(),
        identity: selection.identity().as_bytes().to_vec(),
    }
}

fn parse_selection(
    selection: Selection,
) -> Result<(d::StoragePath, d::DirectoryIdentity), d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidPage;
    let address = d::StoragePath::parse(&selection.address).map_err(|_| invalid)?;
    let identity = selection
        .identity
        .as_slice()
        .try_into()
        .map_err(|_| invalid)?;
    Ok((address, d::DirectoryIdentity::from_bytes(identity)))
}

/// Decode a separately correlated selection from the authenticated executor.
pub fn decode_selection(
    selection: Selection,
) -> Result<d::DirectorySelection, d::ContractViolation> {
    let (address, identity) = parse_selection(selection)?;
    Ok(d::DirectorySelection::from_lookup(address, identity))
}

pub fn encode_page(page: d::DirectoryPage) -> Result<Page, d::ContractViolation> {
    let (selection, entries, next) = page.into_parts();
    let invalid = d::ContractViolation::InvalidPage;
    if entries.len() > MAX_PAGE_ENTRIES {
        return Err(invalid);
    }
    let completion = match next {
        Some(name) => page::Completion::After(name.as_str().into()),
        None => page::Completion::End(Unit {}),
    };
    let mut encoded = Vec::new();
    for entry in entries {
        encode_entry(entry)?
            .encode_length_delimited(&mut encoded)
            .map_err(|_| invalid)?;
    }
    Ok(Page {
        reported: Some(encode_selection(&selection)),
        entries: encoded,
        completion: Some(completion),
    })
}

pub fn decode_page(
    bound: &d::BoundListRequest<'_>,
    page: Page,
) -> Result<d::DirectoryPage, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidPage;
    if page.entries.len() > MAX_FRAME_BYTES {
        return Err(invalid);
    }
    let (reported, identity) = parse_selection(page.reported.ok_or(invalid)?)?;
    let after = match page.completion.ok_or(invalid)? {
        page::Completion::End(_) => None,
        page::Completion::After(name) => Some(d::EntryName::parse(&name).map_err(|_| invalid)?),
    };
    let mut bytes = page.entries.as_slice();
    let mut entries = Vec::new();
    while !bytes.is_empty() {
        if entries.len() == MAX_PAGE_ENTRIES.min(bound.size().get()) {
            return Err(invalid);
        }
        let entry =
            vfs_plugin_protocol::Entry::decode_length_delimited(&mut bytes).map_err(|_| invalid)?;
        entries.push(decode_entry(entry)?);
    }
    d::DirectoryPage::from_parts(bound, &reported, identity, entries, after).map_err(|_| invalid)
}

pub fn encode_failure(
    error: &d::StorageError,
) -> Result<vfs_plugin_protocol::StorageFailure, d::ContractViolation> {
    vfs_plugin_protocol::encode_storage_error(error)
}

pub fn decode_failure(
    expected: d::Operation,
    failure: vfs_plugin_protocol::StorageFailure,
) -> Result<d::StorageError, d::ContractViolation> {
    let error = vfs_plugin_protocol::decode_storage_error(failure)?;
    if error.operation() != expected {
        return Err(d::ContractViolation::InvalidResponse);
    }
    Ok(error)
}
