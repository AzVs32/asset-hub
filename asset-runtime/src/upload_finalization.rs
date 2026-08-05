use asset_core::CoreError;
use asset_core::domain::UploadId;
use asset_core::service::ResourceService;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::task::{Id as TaskId, JoinError, JoinHandle, JoinSet};

type FinalizationResult = Result<asset_core::domain::Resource, CoreError>;
type CompletedTask = Result<(TaskId, (UploadId, FinalizationResult)), JoinError>;

/// Runtime-owned handle for scheduling upload finalization work.
///
/// Clones only retain access to the same supervisor. The supervisor and all child tasks are
/// aborted when the final handle is dropped, so no critical finalization task is detached.
#[derive(Clone)]
pub struct UploadFinalizationScheduler {
    inner: Arc<SchedulerInner>,
}

struct SchedulerInner {
    sender: mpsc::UnboundedSender<UploadId>,
    scheduled: Arc<Mutex<HashSet<UploadId>>>,
    supervisor: Mutex<Option<JoinHandle<()>>>,
}

impl UploadFinalizationScheduler {
    pub fn new(service: ResourceService) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let scheduled = Arc::new(Mutex::new(HashSet::new()));
        let supervisor = tokio::spawn(run_supervisor(service, receiver, scheduled.clone()));
        Self {
            inner: Arc::new(SchedulerInner {
                sender,
                scheduled,
                supervisor: Mutex::new(Some(supervisor)),
            }),
        }
    }

    /// Schedule one finalization. Duplicate active IDs are coalesced by the supervisor.
    pub fn schedule(&self, id: UploadId) -> Result<(), CoreError> {
        let mut scheduled = lock(&self.inner.scheduled);
        if !scheduled.insert(id) {
            return Ok(());
        }
        if self.inner.sender.send(id).is_err() {
            scheduled.remove(&id);
            return Err(CoreError::invariant(
                "upload finalization supervisor is not running",
            ));
        }
        Ok(())
    }
}

impl Drop for SchedulerInner {
    fn drop(&mut self) {
        if let Some(supervisor) = self
            .supervisor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            supervisor.abort();
        }
    }
}

async fn run_supervisor(
    service: ResourceService,
    mut receiver: mpsc::UnboundedReceiver<UploadId>,
    scheduled: Arc<Mutex<HashSet<UploadId>>>,
) {
    let mut task_uploads = HashMap::new();
    let mut tasks = JoinSet::new();

    loop {
        tokio::select! {
            request = receiver.recv() => {
                let Some(id) = request else {
                    break;
                };
                let service = service.clone();
                let task = tasks.spawn(async move {
                    let result = service.finalize_upload(&id).await;
                    (id, result)
                });
                task_uploads.insert(task.id(), id);
            }
            completed = tasks.join_next_with_id(), if !tasks.is_empty() => {
                if let Some(completed) = completed {
                    record_completion(completed, &scheduled, &mut task_uploads);
                }
            }
        }
    }
}

fn record_completion(
    completed: CompletedTask,
    scheduled: &Mutex<HashSet<UploadId>>,
    task_uploads: &mut HashMap<TaskId, UploadId>,
) {
    match completed {
        Ok((task_id, (id, Ok(resource)))) => {
            task_uploads.remove(&task_id);
            lock(scheduled).remove(&id);
            tracing::info!(
                upload_id = %id,
                resource_id = %resource.id(),
                "upload finalization completed"
            );
        }
        Ok((task_id, (id, Err(error)))) => {
            task_uploads.remove(&task_id);
            lock(scheduled).remove(&id);
            tracing::error!(upload_id = %id, error = %error, "upload finalization failed");
        }
        Err(error) => {
            if let Some(id) = task_uploads.remove(&error.id()) {
                lock(scheduled).remove(&id);
            }
            tracing::error!(error = %error, "upload finalization task stopped unexpectedly");
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_ids_are_coalesced_before_the_supervisor_queue() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let scheduler = UploadFinalizationScheduler {
            inner: Arc::new(SchedulerInner {
                sender,
                scheduled: Arc::new(Mutex::new(HashSet::new())),
                supervisor: Mutex::new(None),
            }),
        };
        let id = UploadId::new();

        scheduler.schedule(id).unwrap();
        scheduler.schedule(id).unwrap();

        assert_eq!(receiver.try_recv().unwrap(), id);
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn final_scheduler_handle_aborts_its_supervisor() {
        let (sender, _receiver) = mpsc::unbounded_channel();
        let supervisor = tokio::spawn(std::future::pending::<()>());
        let abort_handle = supervisor.abort_handle();
        let scheduler = UploadFinalizationScheduler {
            inner: Arc::new(SchedulerInner {
                sender,
                scheduled: Arc::new(Mutex::new(HashSet::new())),
                supervisor: Mutex::new(Some(supervisor)),
            }),
        };
        let clone = scheduler.clone();

        drop(scheduler);
        assert!(!abort_handle.is_finished());
        drop(clone);
        tokio::task::yield_now().await;

        assert!(abort_handle.is_finished());
    }
}
