use super::*;

/// 测试相同上传 ID 在进入监督器队列前会被合并，避免创建重复的最终化任务。
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

    scheduler.dispatch(id).unwrap();
    scheduler.dispatch(id).unwrap();

    assert_eq!(receiver.try_recv().unwrap(), id);
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

/// 测试最后一个调度器句柄释放后，会取消其持有的监督器任务。
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
