use super::*;
use asset_core::directory::domain::Directory;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::io::Read;
use std::time::Duration;

fn options(max_bytes: u64) -> ArchiveOptions {
    ArchiveOptions {
        max_concurrent: NonZeroUsize::new(1).unwrap(),
        max_bytes: NonZeroU64::new(max_bytes).unwrap(),
    }
}

fn start(runtime: &ArchiveRuntime) -> ArchiveWorker {
    let permit = runtime.shared.slots.clone().try_acquire_owned().unwrap();
    runtime
        .downloads()
        .start_worker(runtime.shared.stop.child_token(), permit)
        .unwrap()
}

fn file_message(path: &str, content: &[u8]) -> Message {
    Message::File {
        path: path.to_owned(),
        size: content.len() as u64,
        checksum: Some(Checksum::sha256(hex_checksum(&Sha256::digest(content))).unwrap()),
    }
}

// Prevent regressions that produce unreadable ZIP32 files, lose empty/Unicode directories,
// or release disk admission while a completed archive is still alive.
#[tokio::test]
async fn archive_preserves_entries_and_holds_capacity_until_file_disposal() {
    let mut runtime = ArchiveRuntime::new(options(1024 * 1024));
    let worker = start(&runtime);
    for message in [
        Message::Directory("资料/".into()),
        Message::Directory("资料/空目录/".into()),
        file_message("资料/note.txt", b"hello"),
        Message::Chunk(Bytes::from_static(b"hello")),
        Message::EndFile,
        Message::Finish,
    ] {
        worker.sender.send(message).await.unwrap();
    }
    let generated = worker.result.await.unwrap().unwrap();
    assert!(runtime.shared.slots.clone().try_acquire_owned().is_err());
    let mut zip = zip::ZipArchive::new(&generated.file).unwrap();
    assert!(zip.by_name("资料/空目录/").unwrap().is_dir());
    let mut text = String::new();
    zip.by_name("资料/note.txt")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, "hello");
    drop(zip);
    drop(generated);
    assert_eq!(runtime.shared.slots.available_permits(), 1);
    runtime.shutdown().await;
}

// A limit failure can occur only when ZIP central-directory metadata is written at finish.
// It must still return 413 and release the worker/file slot, rather than send a partial ZIP.
#[tokio::test]
async fn output_limit_includes_zip_metadata_and_releases_capacity() {
    let mut runtime = ArchiveRuntime::new(options(1));
    let worker = start(&runtime);
    worker.sender.send(Message::Finish).await.unwrap();
    let error = worker.result.await.unwrap().err().unwrap();
    assert_eq!(
        error.into_response().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    runtime.shutdown().await;
    assert_eq!(runtime.shared.slots.available_permits(), 1);
}

// Equal-size out-of-band edits must not silently produce an archive with unexpected content.
#[tokio::test]
async fn checksum_change_rejects_archive_and_cleans_up() {
    let mut runtime = ArchiveRuntime::new(options(1024));
    let worker = start(&runtime);
    for message in [
        file_message("note.txt", b"old"),
        Message::Chunk(Bytes::from_static(b"new")),
        Message::EndFile,
    ] {
        worker.sender.send(message).await.unwrap();
    }
    let error = worker.result.await.unwrap().err().unwrap();
    assert_eq!(error.into_response().status(), StatusCode::CONFLICT);
    runtime.shutdown().await;
    assert_eq!(runtime.shared.slots.available_permits(), 1);
}

// Cancellation must wake a blocking worker waiting for more input even if the sender lives on;
// shutdown must observe its exit, and old request handles must reject new work.
#[tokio::test]
async fn shutdown_wakes_idle_writer_and_joins_it_with_sender_still_alive() {
    let mut runtime = ArchiveRuntime::new(options(1024));
    let downloads = runtime.downloads();
    let worker = start(&runtime);
    worker
        .sender
        .send(file_message("pending.txt", b"pending"))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while worker.sender.capacity() != BUFFER_CHUNKS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), runtime.shutdown())
        .await
        .unwrap();
    assert!(worker.result.await.unwrap().is_err());
    assert_eq!(runtime.shared.slots.available_permits(), 1);
    assert!(downloads.shared.slots.clone().try_acquire_owned().is_err());
    assert!(runtime.shared.workers.lock().unwrap().is_empty());
}

// Request abandonment must release an idle worker independently of global shutdown.
#[tokio::test]
async fn abandoned_input_releases_writer_and_file() {
    let mut runtime = ArchiveRuntime::new(options(1024));
    let worker = start(&runtime);
    drop(worker.sender);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), worker.result)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    runtime.shutdown().await;
    assert_eq!(runtime.shared.slots.available_permits(), 1);
}

// Ordinary resources must not gain ZIP64 local headers; crossing the per-entry threshold must
// enable them without requiring a multi-gigabyte test fixture.
#[test]
fn zip64_is_selected_only_above_the_zip32_entry_threshold() {
    for (declared_size, expected_version) in [
        (zip::ZIP64_BYTES_THR, 20u16),
        (zip::ZIP64_BYTES_THR + 1, 45u16),
    ] {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("entry", resource_options(declared_size))
            .unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), expected_version);
    }
}

// A source-read failure cancels the writer for cleanup, but that internal cancellation must not
// race the HTTP error and turn a missing resource (404) into a shutdown response (503).
#[tokio::test]
async fn missing_source_preserves_404_while_cancelling_worker() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("note.txt"), b"old").unwrap();
    let mut config = asset_infra::config::AssetInfraConfig::default();
    config.blob.local.root = root.path().to_owned();
    config.blob.local.sync.enabled = false;
    let runtime = asset_runtime::AssetRuntime::new(config).await.unwrap();
    runtime
        .storage_maintenance_service()
        .scan_resources()
        .await
        .unwrap();
    std::fs::remove_file(root.path().join("note.txt")).unwrap();
    let mut archives = ArchiveRuntime::new(options(1024));
    let error = archives
        .downloads()
        .generate(
            &runtime.asset_workflow_service(),
            &runtime.content_service(),
            &Directory::root().id(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.into_response().status(), StatusCode::NOT_FOUND);
    archives.shutdown().await;
    assert_eq!(archives.shared.slots.available_permits(), 1);
}
