use super::*;
use crate::sqlite::SqliteResourceRepository;
use asset_core::domain::Directory;
use asset_core::port::{DirectoryRepository, UserQuery, UserRepository};
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn user_queries_return_workspace_locations_in_one_projection() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let directories = SqliteResourceRepository::from_pool(pool.clone());
    directories.run_migrations().await.unwrap();
    let teams = Directory::new(DirectoryId::root(), "teams").unwrap();
    DirectoryRepository::insert(&directories, &teams)
        .await
        .unwrap();
    let workspace = Directory::new(teams.id(), "alice").unwrap();
    DirectoryRepository::insert(&directories, &workspace)
        .await
        .unwrap();
    let repository = SqliteIdentityRepository::new(pool);
    let user = User::new("alice", "credential-hash", UserRole::Member, workspace.id()).unwrap();
    repository.create(&user).await.unwrap();

    let users = repository.list_located().await.unwrap();

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].user().id(), user.id());
    assert_eq!(users[0].workspace().path().path(), "teams/alice");
}

#[tokio::test]
async fn invalid_persisted_user_is_a_repository_failure() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let directories = SqliteResourceRepository::from_pool(pool.clone());
    directories.run_migrations().await.unwrap();
    let repository = SqliteIdentityRepository::new(pool.clone());
    let user = User::new(
        "alice",
        "credential-hash",
        UserRole::Member,
        DirectoryId::root(),
    )
    .unwrap();
    repository.create(&user).await.unwrap();
    sqlx::query("UPDATE users SET username = 'ab' WHERE id = ?")
        .bind(user.id().to_string())
        .execute(&pool)
        .await
        .unwrap();

    assert!(matches!(
        repository.find_by_id(&user.id()).await,
        Err(CoreError::Repository {
            operation: "user.rehydrate",
            ..
        })
    ));
}
