//! HTTP-owned ZIP generation. Core supplies the manifest and content; this module owns
//! compression, temporary files, bounded admission, and cancellation of blocking workers.

use crate::error::HttpError;
use asset_core::CoreError;
use asset_core::{
    directory::domain::DirectoryId,
    resource::{domain::Checksum, service::ContentService},
    workflow::service::{AssetWorkflowService, DirectoryArchiveManifest},
};
use axum::body::Body;
use bytes::Bytes;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;
use zip::write::SimpleFileOptions;

const CHUNK_BYTES: usize = 64 * 1024;
const BUFFER_CHUNKS: usize = 4;

/// Limits apply per HTTP process, including completed ZIPs still being downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveOptions {
    pub max_concurrent: NonZeroUsize,
    /// Maximum uncompressed resource bytes AND maximum generated ZIP length per request.
    pub max_bytes: NonZeroU64,
}

impl Default for ArchiveOptions {
    fn default() -> Self {
        Self {
            max_concurrent: NonZeroUsize::new(2).unwrap(),
            max_bytes: NonZeroU64::new(64 * 1024 * 1024 * 1024).unwrap(),
        }
    }
}

/// Owned by the HTTP executable; request handles cannot extend admission past its lifetime.
pub struct ArchiveRuntime {
    shared: Arc<Shared>,
}

#[derive(Clone)]
pub struct ArchiveDownloads {
    shared: Arc<Shared>,
}

struct Shared {
    options: ArchiveOptions,
    slots: Arc<Semaphore>,
    stop: CancellationToken,
    workers: Mutex<JoinSet<()>>,
}

impl ArchiveRuntime {
    pub fn new(options: ArchiveOptions) -> Self {
        Self {
            shared: Arc::new(Shared {
                slots: Arc::new(Semaphore::new(options.max_concurrent.get())),
                options,
                stop: CancellationToken::new(),
                workers: Mutex::new(JoinSet::new()),
            }),
        }
    }

    pub fn downloads(&self) -> ArchiveDownloads {
        ArchiveDownloads {
            shared: self.shared.clone(),
        }
    }

    /// Reject new requests and cancel generation and response streams immediately.
    pub fn begin_shutdown(&self) {
        self.shared.slots.close();
        self.shared.stop.cancel();
    }

    /// Observe every blocking worker's exit. Cancellation wakes channel reads and is checked
    /// at each bounded write. An OS filesystem call already in progress cannot be preempted.
    pub async fn shutdown(&mut self) {
        self.begin_shutdown();
        loop {
            // JoinSet remains owned even if this shutdown future is itself cancelled.
            let result = futures_util::future::poll_fn(|cx| {
                self.shared.workers.lock().unwrap().poll_join_next(cx)
            })
            .await;
            match result {
                Some(Err(error)) => tracing::error!(%error, "ZIP worker stopped unexpectedly"),
                Some(Ok(())) => {}
                None => break,
            }
        }
    }
}

impl Drop for ArchiveRuntime {
    fn drop(&mut self) {
        self.begin_shutdown();
    }
}

pub(crate) struct ArchiveDownload {
    pub filename: String,
    pub length: u64,
    pub body: Body,
}

impl ArchiveDownloads {
    pub(crate) async fn generate(
        &self,
        workflows: &AssetWorkflowService,
        content: &ContentService,
        id: &DirectoryId,
    ) -> Result<ArchiveDownload, HttpError> {
        // No unbounded queue of waiting archive requests. Keep the slot through body disposal,
        // not just compression, so slow downloads cannot accumulate unlimited temporary files.
        let permit = self
            .shared
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| HttpError::unavailable("archive capacity unavailable; retry later"))?;
        let cancelled = self.shared.stop.child_token();
        let guard = cancelled.clone().drop_guard();
        let generation = async {
            let manifest = workflows.directory_archive_manifest(id).await?;
            let filename = manifest.filename().to_owned();
            check_manifest_size(&manifest, self.shared.options.max_bytes.get())?;
            let ArchiveWorker { sender, mut result } =
                self.start_worker(cancelled.clone(), permit)?;
            let feed = feed_archive(&manifest, content, sender);
            tokio::pin!(feed);
            // Observe writer failure even if the source read is slow or stalled. Conversely,
            // source failure cancels the worker before awaiting its cleanup.
            let generated = tokio::select! {
                completed = &mut result => worker_result(completed)?,
                fed = &mut feed => {
                    if fed.is_err() { cancelled.cancel(); }
                    let completed = result.await;
                    fed?;
                    worker_result(completed)?
                }
            };
            let length = generated.length;
            let reader =
                ReaderStream::with_capacity(tokio::fs::File::from_std(generated.file), CHUNK_BYTES);
            let body = Body::from_stream(futures_util::stream::try_unfold(
                (reader, generated.permit, guard, cancelled.clone()),
                |(mut reader, permit, guard, cancelled)| async move {
                    tokio::select! {
                        biased;
                        _ = cancelled.cancelled() => Err(io::Error::new(io::ErrorKind::Interrupted, "archive download cancelled")),
                        chunk = reader.next() => match chunk {
                            Some(Ok(chunk)) => Ok(Some((chunk, (reader, permit, guard, cancelled)))),
                            Some(Err(error)) => Err(error),
                            None => Ok(None),
                        }
                    }
                },
            ));
            Ok(ArchiveDownload {
                filename,
                length,
                body,
            })
        };
        tokio::select! {
            biased;
            _ = self.shared.stop.cancelled() => Err(HttpError::unavailable("archive generation cancelled")),
            result = generation => result,
        }
    }

    fn start_worker(
        &self,
        cancelled: CancellationToken,
        permit: OwnedSemaphorePermit,
    ) -> Result<ArchiveWorker, HttpError> {
        let (sender, receiver) = mpsc::channel(BUFFER_CHUNKS);
        let (result_sender, result) = oneshot::channel();
        let max_bytes = self.shared.options.max_bytes.get();
        let mut workers = self.shared.workers.lock().unwrap();
        while let Some(result) = workers.try_join_next() {
            if let Err(error) = result {
                tracing::error!(%error, "ZIP worker stopped unexpectedly");
            }
        }
        if self.shared.stop.is_cancelled() {
            return Err(HttpError::unavailable("archive service is stopping"));
        }
        workers.spawn_blocking(move || {
            let result = write_archive(receiver, cancelled, max_bytes, permit);
            // Dropping an undeliverable result closes the temporary file and releases its slot.
            let _ = result_sender.send(result);
        });
        Ok(ArchiveWorker { sender, result })
    }
}

fn worker_result(
    result: Result<Result<Generated, HttpError>, oneshot::error::RecvError>,
) -> Result<Generated, HttpError> {
    result.map_err(|error| HttpError::internal(format!("ZIP worker failed: {error}")))?
}

struct ArchiveWorker {
    sender: mpsc::Sender<Message>,
    result: oneshot::Receiver<Result<Generated, HttpError>>,
}

fn hex_checksum(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn check_manifest_size(manifest: &DirectoryArchiveManifest, limit: u64) -> Result<(), HttpError> {
    let total = manifest.resources().iter().try_fold(0u64, |total, entry| {
        total.checked_add(entry.content_length())
    });
    if total.is_none_or(|total| total > limit) {
        return Err(HttpError::payload_too_large(format!(
            "archive resource bytes exceed {limit}"
        )));
    }
    Ok(())
}

enum Message {
    Directory(String),
    File {
        path: String,
        size: u64,
        checksum: Option<Checksum>,
    },
    Chunk(Bytes),
    EndFile,
    Finish,
}

async fn feed_archive(
    manifest: &DirectoryArchiveManifest,
    content: &ContentService,
    sender: mpsc::Sender<Message>,
) -> Result<(), HttpError> {
    for directory in manifest.directories() {
        if sender
            .send(Message::Directory(directory.clone()))
            .await
            .is_err()
        {
            return Ok(());
        }
    }
    for entry in manifest.resources() {
        if sender
            .send(Message::File {
                path: entry.path().to_owned(),
                size: entry.content_length(),
                checksum: entry.checksum().cloned(),
            })
            .await
            .is_err()
        {
            return Ok(());
        }
        let stream = content
            .stream(&entry.resource_id(), None)
            .await?
            .ok_or_else(|| {
                HttpError::not_found(format!(
                    "resource content `{}` not found",
                    entry.resource_id()
                ))
            })?;
        let mut stream = stream.into_content();
        while let Some(chunk) = stream.next().await {
            // Copy bounded pieces: a Bytes slice would keep an arbitrarily large source buffer
            // alive inside the channel. No complete-file buffering is introduced here.
            let chunk = chunk?;
            for bytes in chunk.chunks(CHUNK_BYTES) {
                if sender
                    .send(Message::Chunk(Bytes::copy_from_slice(bytes)))
                    .await
                    .is_err()
                {
                    return Ok(()); // The worker's error is authoritative (e.g. output limit).
                }
            }
        }
        if sender.send(Message::EndFile).await.is_err() {
            return Ok(());
        }
    }
    let _ = sender.send(Message::Finish).await;
    Ok(())
}

struct Generated {
    file: File,
    length: u64,
    permit: OwnedSemaphorePermit,
}

fn write_archive(
    mut receiver: mpsc::Receiver<Message>,
    cancelled: CancellationToken,
    limit: u64,
    permit: OwnedSemaphorePermit,
) -> Result<Generated, HttpError> {
    // Anonymous temporary files are removed on close, including process exit. No named partial
    // ZIP survives an error, request cancellation, or a dropped result receiver.
    let file = tempfile::tempfile().map_err(archive_io)?;
    let mut archive = zip::ZipWriter::new(LimitedFile {
        file,
        position: 0,
        limit,
        cancelled: cancelled.clone(),
    });
    let mut current: Option<(u64, Option<Checksum>, u64, Sha256)> = None;
    let handle = tokio::runtime::Handle::current();
    loop {
        let message = handle
            .block_on(async {
                tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => None,
                    message = receiver.recv() => message,
                }
            })
            .ok_or_else(|| HttpError::unavailable("archive input cancelled"))?;
        match message {
            Message::Directory(path) => {
                archive
                    .add_directory(path, SimpleFileOptions::default())
                    .map_err(archive_zip)?;
            }
            Message::File {
                path,
                size,
                checksum,
            } => {
                archive
                    .start_file(path, resource_options(size))
                    .map_err(archive_zip)?;
                current = Some((size, checksum, 0, Sha256::new()));
            }
            Message::Chunk(chunk) => {
                let (expected_size, _, size, hash) = current
                    .as_mut()
                    .ok_or_else(|| HttpError::internal("ZIP chunk without entry"))?;
                *size = size
                    .checked_add(chunk.len() as u64)
                    .ok_or_else(content_changed)?;
                if *size > *expected_size {
                    return Err(content_changed());
                }
                hash.update(&chunk);
                archive.write_all(&chunk).map_err(archive_io)?;
            }
            Message::EndFile => {
                let (expected_size, expected_checksum, size, hash) = current
                    .take()
                    .ok_or_else(|| HttpError::internal("ZIP entry missing"))?;
                if size != expected_size
                    || expected_checksum
                        .is_some_and(|expected| hex_checksum(&hash.finalize()) != expected.value())
                {
                    return Err(content_changed());
                }
            }
            Message::Finish => break,
        }
    }
    let mut output = archive.finish().map_err(archive_zip)?;
    let length = output.file.metadata().map_err(archive_io)?.len();
    output.seek(SeekFrom::Start(0)).map_err(archive_io)?;
    Ok(Generated {
        file: output.file,
        length,
        permit,
    })
}

fn resource_options(size: u64) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(size > zip::ZIP64_BYTES_THR)
}

fn content_changed() -> HttpError {
    CoreError::conflict("resource content changed during archive generation; retry the download")
        .into()
}

#[derive(Debug)]
struct OutputLimit;
impl std::fmt::Display for OutputLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("generated ZIP exceeds configured byte limit")
    }
}
impl std::error::Error for OutputLimit {}

fn archive_io(error: io::Error) -> HttpError {
    if error
        .get_ref()
        .is_some_and(|source| source.is::<OutputLimit>())
    {
        HttpError::payload_too_large(error.to_string())
    } else {
        HttpError::internal(format!("archive I/O failed: {error}"))
    }
}

fn archive_zip(error: zip::result::ZipError) -> HttpError {
    match error {
        zip::result::ZipError::Io(error) => archive_io(error),
        error => HttpError::internal(format!("archive generation failed: {error}")),
    }
}

// ZIP rewrites headers via Seek. Bound the resulting file extent, not cumulative bytes written.
struct LimitedFile {
    file: File,
    position: u64,
    limit: u64,
    cancelled: CancellationToken,
}
impl LimitedFile {
    fn check(&self) -> io::Result<()> {
        if self.cancelled.is_cancelled() {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "archive cancelled",
            ))
        } else {
            Ok(())
        }
    }
}
impl Write for LimitedFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.check()?;
        if self
            .position
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.limit)
        {
            return Err(io::Error::other(OutputLimit));
        }
        let written = self.file.write(bytes)?;
        self.position += written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.check()?;
        self.file.flush()
    }
}
impl Seek for LimitedFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check()?;
        self.position = self.file.seek(position)?;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests;
