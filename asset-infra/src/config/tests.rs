use super::*;

#[test]
fn normalization_keeps_sqlite_in_the_blob_data_domain() {
    let database = DatabaseConfig::default();
    database.validate().unwrap();
    let blob = BlobConfig::default().normalize().unwrap();

    assert!(blob.local.root.is_absolute());
    assert_eq!(
        database.sqlite_path_in(blob.local_root()),
        blob.local.root.join(".asset-hub/asset-hub.sqlite")
    );
}
