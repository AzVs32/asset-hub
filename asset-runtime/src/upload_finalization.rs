//! 上传会话进入 Finalizing 状态后的后台调度器。
//!
//! Core 负责校验上传状态并执行一次最终化用例，Runtime 只负责把上传 ID 放入队列、
//! 去除重复请求，以及持有后台任务的生命周期。这样 HTTP 等应用入口无需自行管理
//! Tokio 任务，也不会因为请求处理结束而让关键的最终化任务失去所有者。

use asset_core::CoreError;
use asset_core::domain::UploadId;
use asset_core::service::ResourceService;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::task::{Id as TaskId, JoinError, JoinHandle, JoinSet};

type FinalizationResult = Result<asset_core::domain::Resource, CoreError>;
// JoinSet 正常返回任务产生的 (UploadId, FinalizationResult)；若任务 panic 或被取消，
// 则只能从 JoinError 中取得任务 ID，因此监督器还需要维护任务 ID 到上传 ID 的映射。
type CompletedTask = Result<(TaskId, (UploadId, FinalizationResult)), JoinError>;

/// Runtime 持有的上传最终化调度句柄。
///
/// 句柄的 clone 只是共享同一个监督器，并不会创建新的后台消费者。最后一个句柄释放时，
/// 监督器会被取消；监督器持有的所有子任务也会随之取消，因此不会遗留脱离 Runtime
/// 生命周期的关键最终化任务。
#[derive(Clone)]
pub struct UploadFinalizationScheduler {
    inner: Arc<SchedulerInner>,
}

struct SchedulerInner {
    /// 向唯一的监督器发送待最终化的上传 ID。
    sender: mpsc::UnboundedSender<UploadId>,
    /// 当前已排队或正在执行的上传 ID，用于合并重复调度请求。
    scheduled: Arc<Mutex<HashSet<UploadId>>>,
    /// 监督器的 JoinHandle；由最后一个共享句柄负责取消并回收其生命周期。
    supervisor: Mutex<Option<JoinHandle<()>>>,
}

impl UploadFinalizationScheduler {
    pub fn new(service: ResourceService) -> Self {
        // 调度接口是同步方法，因此使用无界 channel 将“提交请求”和“执行最终化”解耦。
        // receiver 只交给下面启动的唯一监督器，所有 scheduler clone 都复用这个队列。
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

    /// 调度一个上传最终化任务。
    ///
    /// 同一 ID 在请求已入队但尚未完成期间只会保留一份；最终化成功或失败后，
    /// ID 会从 scheduled 中移除，后续调用即可再次尝试。发送失败时要撤销预先
    /// 占用的集合项，避免监督器已停止后该 ID 永久处于“已调度”状态。
    pub fn schedule(&self, id: UploadId) -> Result<(), CoreError> {
        let mut scheduled = lock(&self.inner.scheduled);
        // 先占位再发送，保证并发调用不会为同一个上传创建多个子任务。
        if !scheduled.insert(id) {
            return Ok(());
        }
        if self.inner.sender.send(id).is_err() {
            // channel 关闭意味着监督器已不再接收请求；恢复集合状态后把故障交给调用方。
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
        // SchedulerInner 只会在最后一个 Arc clone 释放时 drop。只取出一次 JoinHandle，
        // 再取消监督器；监督器结束时其 JoinSet 的 Drop 会继续取消仍在运行的子任务。
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
    // 监督器是该模块唯一消费队列的位置。task_uploads 用来在 JoinError（任务 panic、
    // 取消等非正常结束）中反查上传 ID；正常完成时上传 ID 会随任务结果直接返回。
    let mut task_uploads = HashMap::new();
    let mut tasks = JoinSet::new();

    loop {
        tokio::select! {
            request = receiver.recv() => {
                let Some(id) = request else {
                    // 所有 scheduler 句柄都已释放，发送端关闭；离开后 JoinSet 会清理子任务。
                    break;
                };
                let service = service.clone();
                // 每个上传由独立子任务执行，避免某个慢上传阻塞其他 ID 的最终化。
                let task = tasks.spawn(async move {
                    let result = service.finalize_upload(&id).await;
                    (id, result)
                });
                // 在任务结果中记录对应关系，供非正常结束时清理 scheduled。
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
    // 无论最终化成功、Core 返回错误，还是 Tokio 报告任务异常，都必须释放去重占位，
    // 否则该上传 ID 将无法再次调度。任务映射也要同步删除，避免持续积累。
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
            // 业务失败仍是一次已观察到的完成事件；记录错误后允许调用方稍后重试。
            task_uploads.remove(&task_id);
            lock(scheduled).remove(&id);
            tracing::error!(upload_id = %id, error = %error, "upload finalization failed");
        }
        Err(error) => {
            // JoinError 不携带任务自身返回的上传 ID，只能通过 task_id 反查并释放占位。
            if let Some(id) = task_uploads.remove(&error.id()) {
                lock(scheduled).remove(&id);
            }
            tracing::error!(error = %error, "upload finalization task stopped unexpectedly");
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // 后台任务异常不会让调度器永久失效；即使某次持锁代码 panic 导致 poisoned，仍恢复
    // 内部集合继续运行。集合只保存调度状态，不承载需要回滚的独立持久化数据。
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests;
