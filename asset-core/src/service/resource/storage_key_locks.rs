use crate::domain::StorageKey;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// 仅串行化同一对象键上的上传、发布和存储协调。
#[derive(Default)]
pub(super) struct StorageKeyLocks {
    locks: Mutex<HashMap<StorageKey, Weak<AsyncMutex<()>>>>,
}

impl StorageKeyLocks {
    pub(super) async fn lock(&self, key: &StorageKey) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self
                .locks
                .lock()
                .expect("storage key lock registry should not be poisoned");
            locks.retain(|_, lock| lock.strong_count() > 0);
            locks.get(key).and_then(Weak::upgrade).unwrap_or_else(|| {
                let lock = Arc::new(AsyncMutex::new(()));
                locks.insert(key.clone(), Arc::downgrade(&lock));
                lock
            })
        };
        lock.lock_owned().await
    }

    pub(super) async fn lock_many(&self, keys: &[StorageKey]) -> Vec<OwnedMutexGuard<()>> {
        let mut keys = keys.to_vec();
        keys.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        keys.dedup();

        let mut guards = Vec::with_capacity(keys.len());
        for key in keys {
            guards.push(self.lock(&key).await);
        }
        guards
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn serializes_only_the_same_storage_key() {
        let locks = Arc::new(StorageKeyLocks::default());
        let first_key = StorageKey::new("assets/first.bin").unwrap();
        let second_key = StorageKey::new("assets/second.bin").unwrap();
        let first_guard = locks.lock(&first_key).await;

        let same_key_waiter = {
            let locks = locks.clone();
            let first_key = first_key.clone();
            tokio::spawn(async move {
                let _guard = locks.lock(&first_key).await;
            })
        };
        tokio::task::yield_now().await;
        assert!(!same_key_waiter.is_finished());

        tokio::time::timeout(Duration::from_secs(1), locks.lock(&second_key))
            .await
            .expect("an unrelated storage key must not be blocked");

        drop(first_guard);
        tokio::time::timeout(Duration::from_secs(1), same_key_waiter)
            .await
            .expect("same-key waiter should resume after release")
            .unwrap();
    }
}
