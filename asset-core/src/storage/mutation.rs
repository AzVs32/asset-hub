use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// 进程内物理存储变更协调器。
///
/// 目录搬迁和依赖目录路径解析的 Blob 发布必须共享同一个实例，避免目录路径在一次
/// 多步骤文件操作期间发生变化。该协调器由组合根创建并注入，不通过业务 service 暴露。
#[derive(Clone, Default)]
pub struct StorageMutationCoordinator {
    lock: Arc<Mutex<()>>,
}

impl StorageMutationCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn enter(&self) -> OwnedMutexGuard<()> {
        self.lock.clone().lock_owned().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cloned_coordinators_share_the_same_mutation_boundary() {
        let coordinator = StorageMutationCoordinator::new();
        let guard = coordinator.enter().await;
        let waiter = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                let _guard = coordinator.enter().await;
            })
        };

        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        drop(guard);
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("a cloned coordinator must resume after the shared guard is released")
            .unwrap();
    }
}
