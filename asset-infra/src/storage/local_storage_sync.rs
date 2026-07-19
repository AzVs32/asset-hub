use super::FileSystemScanner;
use crate::config::LocalBlobSyncConfig;
use asset_core::CoreError;
use asset_core::domain::StorageKey;
use asset_core::port::{ScannedStorageEntry, StoragePrefix, StorageScanner};
use asset_core::service::ResourceService;
use futures_util::StreamExt;
use notify::event::{AccessKind, AccessMode, ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

const EVENT_QUEUE_CAPACITY: usize = 2_048;

/// 保持本地文件系统监听器和后台协调任务存活。
pub struct LocalStorageSync {
    _watcher: RecommendedWatcher,
    task: tokio::task::JoinHandle<()>,
}

impl LocalStorageSync {
    pub async fn start(
        root: PathBuf,
        config: &LocalBlobSyncConfig,
        scanner: Arc<FileSystemScanner>,
        service: ResourceService,
    ) -> Result<Self, CoreError> {
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

        service.reconcile_storage().await?;
        let known_directories = load_known_directories(scanner.as_ref()).await?;
        let debounce = Duration::from_millis(config.debounce_milliseconds);
        let interval = Duration::from_secs(config.reconcile_interval_seconds);
        let task = tokio::spawn(run_sync_loop(
            root,
            scanner,
            service,
            receiver,
            overflowed,
            known_directories,
            debounce,
            interval,
        ));

        Ok(Self {
            _watcher: watcher,
            task,
        })
    }
}

impl Drop for LocalStorageSync {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_sync_loop(
    root: PathBuf,
    scanner: Arc<FileSystemScanner>,
    service: ResourceService,
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
                    scanner.as_ref(),
                    &service,
                    &mut known_directories,
                    events,
                    overflowed.swap(false, Ordering::AcqRel),
                ).await {
                    tracing::error!(error = %error, "automatic local storage synchronization failed");
                }
            }
            _ = interval.tick() => {
                if let Err(error) = reconcile_all(scanner.as_ref(), &service, &mut known_directories).await {
                    tracing::error!(error = %error, "periodic local storage reconciliation failed");
                }
            }
        }
    }
}

async fn reconcile_events(
    root: &Path,
    scanner: &FileSystemScanner,
    service: &ResourceService,
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
        return reconcile_all(scanner, service, known_directories).await;
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
    scanner: &FileSystemScanner,
    service: &ResourceService,
    known_directories: &mut HashSet<StorageKey>,
) -> Result<(), CoreError> {
    service.reconcile_storage().await?;
    *known_directories = load_known_directories(scanner).await?;
    Ok(())
}

async fn load_known_directories(
    scanner: &FileSystemScanner,
) -> Result<HashSet<StorageKey>, CoreError> {
    let mut directories = HashSet::new();
    let mut entries = scanner.scan(&StoragePrefix::root());
    while let Some(entry) = entries.next().await {
        if let ScannedStorageEntry::Directory(directory) = entry? {
            directories.insert(StorageKey::new(directory.path().to_owned())?);
        }
    }
    Ok(directories)
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
                if parts.is_empty() && part == asset_core::port::RESERVED_BLOB_STORAGE_PREFIX {
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
