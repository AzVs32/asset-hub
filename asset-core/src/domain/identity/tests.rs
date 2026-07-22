use super::*;
use crate::domain::ResourceDirectory;

#[test]
fn user_normalizes_and_validates_username() {
    let workspace = ResourceDirectory::from_path("users/alice").unwrap();
    let user = User::new(
        " alice ",
        "argon2-hash",
        UserRole::Member,
        workspace.clone(),
    )
    .unwrap();
    assert_eq!(user.username(), "alice");
    assert_eq!(user.workspace_directory(), &workspace);
    assert!(User::new("../alice", "hash", UserRole::Member, workspace).is_err());
}

#[test]
fn workspace_directory_is_shareable_and_independent_of_role() {
    let shared_workspace = ResourceDirectory::from_path("shared").unwrap();
    assert!(
        User::new(
            "admin",
            "hash",
            UserRole::Administrator,
            ResourceDirectory::root()
        )
        .is_ok()
    );
    assert!(
        User::new(
            "admin",
            "hash",
            UserRole::Administrator,
            shared_workspace.clone()
        )
        .is_ok()
    );
    assert!(User::new("alice", "hash", UserRole::Member, shared_workspace).is_ok());
    assert!(User::new("alice", "hash", UserRole::Member, ResourceDirectory::root()).is_ok());
}

#[test]
fn credential_hash_can_be_replaced_but_not_cleared() {
    let mut user = User::new(
        "alice",
        "old-hash",
        UserRole::Member,
        ResourceDirectory::root(),
    )
    .unwrap();

    user.change_credential_hash("new-hash").unwrap();
    assert_eq!(user.credential_hash(), "new-hash");
    assert!(user.change_credential_hash("  ").is_err());
    assert_eq!(user.credential_hash(), "new-hash");
}
