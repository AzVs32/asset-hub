use asset_core::CoreError;
use asset_core::{resource::service::StorageMaintenanceService, storage::StorageKey};
use futures_util::StreamExt;
use notify::event::{AccessKind, AccessMode, ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

const EVENT_QUEUE_CAPACITY: usize = 2_048;
const MAX_CONCURRENT_CHECKSUMS: usize = 4;

/// 保持本地文件系统监听器和后台协调任务存活。
pub struct LocalStorageSync {
    _watcher: RecommendedWatcher,
    task: tokio::task::JoinHandle<()>,
}

impl LocalStorageSync {
    pub async fn start(
        root: PathBuf,
        debounce: Duration,
        reconcile_interval: Duration,
        service: StorageMaintenanceService,
    ) -> Result<Self, CoreError> {
        if debounce.is_zero() {
            return Err(CoreError::configuration(
                "local storage sync debounce must be greater than zero",
            ));
        }
        if reconcile_interval.is_zero() {
            return Err(CoreError::configuration(
                "local storage sync reconcile interval must be greater than zero",
            ));
        }
        let (sender, receiver) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let overflowed = Arc::new(AtomicBool::new(false));
        let callback_overflowed = overflowed.clone();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            if sender.try_send(event).is_err() {
                callback_overflowed.store(true, Ordering::Release);
            }
        })
        .map_err(notify_error)?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(notify_error)?;

        tracing::info!("incremental storage reconciliation started");
        let task = tokio::spawn(async move {
            let (known_directories, pending) = match service.reconcile_storage_on_startup().await {
                Ok(report) => {
                    log_reconciliation("initial", &report);
                    (
                        report.directory_keys().iter().cloned().collect(),
                        report.pending_verification_keys().to_vec(),
                    )
                }
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        "initial local storage reconciliation failed; periodic reconciliation will retry"
                    );
                    (HashSet::new(), Vec::new())
                }
            };
            // Both futures are owned by this task. Aborting it drops all in-flight checks;
            // no per-file task can retain a service beyond the synchronization owner's lifetime.
            let verification_service = service.clone();
            tokio::join!(
                verify_pending(pending, move |key| {
                    let service = verification_service.clone();
                    async move { service.reconcile_storage_keys(&[key]).await }
                }),
                run_sync_loop(
                    root,
                    service,
                    receiver,
                    overflowed,
                    known_directories,
                    debounce,
                    reconcile_interval,
                )
            );
        });

        Ok(Self {
            _watcher: watcher,
            task,
        })
    }
}

async fn verify_pending<F>(keys: Vec<StorageKey>, mut verify: impl FnMut(StorageKey) -> F)
where
    F: Future<Output = Result<(), CoreError>>,
{
    use futures_util::FutureExt;
    futures_util::stream::iter(keys)
        .map(move |key| {
            let operation = verify(key.clone());
            async move {
                match AssertUnwindSafe(operation).catch_unwind().await {
                    Ok(Ok(())) => tracing::info!(storage_key = %key, "background content checksum verification completed"),
                    Ok(Err(error)) => tracing::error!(storage_key = %key, error = %error, "background content checksum verification failed"),
                    Err(_) => tracing::error!(storage_key = %key, "background content checksum verification panicked"),
                }
            }
        })
        .buffer_unordered(MAX_CONCURRENT_CHECKSUMS)
        .for_each(|()| async {})
        .await;
}

impl Drop for LocalStorageSync {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_sync_loop(
    root: PathBuf,
    service: StorageMaintenanceService,
    mut receiver: mpsc::Receiver<notify::Result<Event>>,
    overflowed: Arc<AtomicBool>,
    mut known_directories: HashSet<StorageKey>,
    debounce: Duration,
    reconcile_interval: Duration,
) {
    let mut interval = tokio::time::interval(reconcile_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;

    loop {
        tokio::select! {
            event = receiver.recv() => {
                let Some(event) = event else { break };
                let mut events = vec![event];
                let deadline = tokio::time::Instant::now() + debounce;
                while let Ok(Some(event)) = tokio::time::timeout_at(deadline, receiver.recv()).await {
                    events.push(event);
                }
                if let Err(error) = reconcile_events(
                    &root,
                    &service,
                    &mut known_directories,
                    events,
                    overflowed.swap(false, Ordering::AcqRel),
                ).await {
                    tracing::error!(error = %error, "automatic local storage synchronization failed");
                }
            }
            _ = interval.tick() => {
                if let Err(error) = reconcile_all(&service, &mut known_directories).await {
                    tracing::error!(error = %error, "periodic local storage reconciliation failed");
                }
            }
        }
    }
}

async fn reconcile_events(
    root: &Path,
    service: &StorageMaintenanceService,
    known_directories: &mut HashSet<StorageKey>,
    events: Vec<notify::Result<Event>>,
    mut full_reconciliation: bool,
) -> Result<(), CoreError> {
    let mut changed_files = HashSet::new();
    let mut renames = Vec::new();

    for event in events {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!(error = %error, "file system watcher reported an error");
                full_reconciliation = true;
                continue;
            }
        };
        if event.need_rescan() {
            full_reconciliation = true;
        }
        if !event_affects_storage_state(event.kind) {
            continue;
        }
        let keys = event
            .paths
            .iter()
            .filter_map(|path| storage_key_from_path(root, path).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        if keys.is_empty() {
            continue;
        }
        if keys
            .iter()
            .any(|key| known_directories.contains(key) || root.join(key.as_str()).is_dir())
        {
            full_reconciliation = true;
            continue;
        }
        if matches!(
            event.kind,
            EventKind::Modify(ModifyKind::Name(RenameMode::Both))
        ) && keys.len() == 2
        {
            renames.push((keys[0].clone(), keys[1].clone()));
        } else {
            changed_files.extend(keys);
        }
    }

    if full_reconciliation {
        return reconcile_all(service, known_directories).await;
    }
    for (from, to) in renames {
        service.reconcile_storage_rename(&from, &to).await?;
        changed_files.remove(&from);
        changed_files.remove(&to);
    }
    let mut changed_files = changed_files.into_iter().collect::<Vec<_>>();
    changed_files.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    service.reconcile_storage_keys(&changed_files).await
}

async fn reconcile_all(
    service: &StorageMaintenanceService,
    known_directories: &mut HashSet<StorageKey>,
) -> Result<(), CoreError> {
    let report = service.reconcile_storage().await?;
    log_reconciliation("periodic", &report);
    *known_directories = report.directory_keys().iter().cloned().collect();
    Ok(())
}

fn log_reconciliation(
    phase: &str,
    report: &asset_core::resource::service::StorageReconciliationReport,
) {
    tracing::info!(
        phase,
        files = report.files,
        hashed_files = report.hashed_files,
        unchanged_files = report.unchanged_files,
        pending_verification_files = report.pending_verification_keys().len(),
        directories = report.directories,
        removed_resources = report.removed_resources,
        elapsed_ms = report.elapsed.as_millis(),
        hash_elapsed_ms = report.hash_elapsed.as_millis(),
        "storage reconciliation completed"
    );
}

fn storage_key_from_path(root: &Path, path: &Path) -> Result<Option<StorageKey>, CoreError> {
    let relative = match path.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative,
        Ok(_) => return Ok(None),
        Err(_) => return Ok(None),
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    CoreError::configuration("local storage path must be valid UTF-8")
                })?;
                if parts.is_empty() && part == asset_core::storage::RESERVED_BLOB_STORAGE_PREFIX {
                    return Ok(None);
                }
                parts.push(part);
            }
            Component::CurDir => {}
            _ => {
                return Err(CoreError::configuration(
                    "local storage event contains an invalid path",
                ));
            }
        }
    }
    StorageKey::new(parts.join("/"))
        .map(Some)
        .map_err(CoreError::from)
}

fn notify_error(error: notify::Error) -> CoreError {
    CoreError::configuration(format!("local storage watcher failed: {error}"))
}

fn event_affects_storage_state(kind: EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_)) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prevent detached or eagerly spawned per-file work, and a single failure stopping the queue.
    #[tokio::test]
    async fn verification_is_bounded_survives_failures_and_drops_with_its_owner() {
        use std::sync::atomic::AtomicUsize;
        struct Active(Arc<AtomicUsize>);
        impl Drop for Active {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let active = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let permits = Arc::new(tokio::sync::Semaphore::new(0));
        let (started, mut starts) = mpsc::unbounded_channel();
        let keys = (0..MAX_CONCURRENT_CHECKSUMS * 2)
            .map(|n| StorageKey::new(n.to_string()).unwrap())
            .collect();
        let owner = tokio::spawn(verify_pending(keys, {
            let active = active.clone();
            let completed = completed.clone();
            let permits = permits.clone();
            move |key| {
                let active = active.clone();
                let completed = completed.clone();
                let permits = permits.clone();
                let started = started.clone();
                async move {
                    active.fetch_add(1, Ordering::SeqCst);
                    let _guard = Active(active);
                    started.send(()).unwrap();
                    permits.acquire().await.unwrap().forget();
                    match key.as_str() {
                        "0" => Err(CoreError::invariant("test verification failure")),
                        "1" => panic!("test verification panic"),
                        _ => {
                            completed.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        }
                    }
                }
            }
        }));
        for _ in 0..MAX_CONCURRENT_CHECKSUMS {
            tokio::time::timeout(Duration::from_secs(2), starts.recv())
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(active.load(Ordering::SeqCst), MAX_CONCURRENT_CHECKSUMS);
        assert!(starts.try_recv().is_err());
        permits.add_permits(2);
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(2), starts.recv())
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(active.load(Ordering::SeqCst), MAX_CONCURRENT_CHECKSUMS);
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(completed.load(Ordering::SeqCst), 0);
        assert!(starts.try_recv().is_err());
    }

    #[test]
    fn read_events_do_not_trigger_checksum_reconciliation() {
        assert!(!event_affects_storage_state(EventKind::Access(
            AccessKind::Open(AccessMode::Read)
        )));
        assert!(!event_affects_storage_state(EventKind::Access(
            AccessKind::Close(AccessMode::Read)
        )));
        assert!(event_affects_storage_state(EventKind::Access(
            AccessKind::Close(AccessMode::Write)
        )));
        assert!(event_affects_storage_state(EventKind::Modify(
            ModifyKind::Data(notify::event::DataChange::Any)
        )));
    }

    #[test]
    fn reserved_internal_paths_are_not_exposed_as_storage_changes() {
        let root = Path::new("storage-root");
        assert_eq!(
            storage_key_from_path(root, &root.join("docs/readme.md")).unwrap(),
            Some(StorageKey::new("docs/readme.md").unwrap())
        );
        assert_eq!(
            storage_key_from_path(root, &root.join(".asset-hub/asset-hub.sqlite")).unwrap(),
            None
        );
    }
}
