use super::EntryName;
use crate::error::VfsError;
use crate::namespace::NamespaceError;

#[test]
fn entry_name_is_a_validated_virtual_path_segment() {
    let name = EntryName::try_from("My File.mp4").unwrap();

    assert_eq!(name.as_str(), "My File.mp4");
    assert_eq!(name.as_ref(), "My File.mp4");
    assert_eq!(name.to_string(), "My File.mp4");
    assert!(matches!(
        EntryName::try_from("nested/file").unwrap_err(),
        VfsError::Namespace(NamespaceError::InvalidEntryName)
    ));
    assert!(matches!(
        EntryName::try_from("..").unwrap_err(),
        VfsError::Namespace(NamespaceError::InvalidEntryName)
    ));
}
