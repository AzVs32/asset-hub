use ah_vfs::application::dto::DirQuery;
use ah_vfs_storage::{
    DirectoryCursor, DirectoryIdentity, DirectorySelection, EntryName, PageSize, StoragePath,
};

#[test]
fn virtual_page_size_and_cursor_address_are_bounded()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(DirQuery::default().size, PageSize::DEFAULT);
    assert!(PageSize::new(0).is_err());
    assert!(PageSize::new(PageSize::MAX + 1).is_err());
    let address = StoragePath::parse(&"x".repeat(ah_vfs_storage::MAX_PATH_BYTES))?;
    let selection =
        DirectorySelection::from_lookup(address, DirectoryIdentity::from_bytes([1; 16]));
    assert!(DirectoryCursor::from_checkpoint(selection, EntryName::parse("child")?).is_err());
    Ok(())
}
