use super::*;
use crate::domain::DirectoryId;

#[test]
fn user_normalizes_and_validates_username() {
    let workspace = DirectoryId::new();
    let user = User::new(" alice ", "argon2-hash", UserRole::Member, workspace).unwrap();
    assert_eq!(user.username(), "alice");
    assert_eq!(user.workspace_directory_id(), workspace);
    assert!(User::new("../alice", "hash", UserRole::Member, workspace).is_err());
}

#[test]
fn workspace_directory_is_shareable_and_independent_of_role() {
    let shared_workspace = DirectoryId::new();
    assert!(
        User::new(
            "admin",
            "hash",
            UserRole::Administrator,
            DirectoryId::root()
        )
        .is_ok()
    );
    assert!(User::new("admin", "hash", UserRole::Administrator, shared_workspace).is_ok());
    assert!(User::new("alice", "hash", UserRole::Member, shared_workspace).is_ok());
    assert!(User::new("alice", "hash", UserRole::Member, DirectoryId::root()).is_ok());
}

#[test]
fn credential_hash_can_be_replaced_but_not_cleared() {
    let mut user = User::new("alice", "old-hash", UserRole::Member, DirectoryId::root()).unwrap();

    user.change_credential_hash("new-hash").unwrap();
    assert_eq!(user.credential_hash(), "new-hash");
    assert!(user.change_credential_hash("  ").is_err());
    assert_eq!(user.credential_hash(), "new-hash");
}

#[test]
fn user_rehydration_rejects_inconsistent_timestamps() {
    let created_at = chrono::Utc::now();
    let snapshot = UserSnapshot {
        id: UserId::new(),
        username: "alice".to_owned(),
        credential_hash: "hash".to_owned(),
        role: UserRole::Member,
        status: UserStatus::Active,
        workspace_directory_id: DirectoryId::root(),
        created_at,
        updated_at: created_at - chrono::Duration::seconds(1),
    };

    assert_eq!(
        User::rehydrate(snapshot),
        Err(crate::UserError::InvalidTimestamps)
    );
}
