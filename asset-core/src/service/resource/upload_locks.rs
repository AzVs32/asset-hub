use crate::domain::UploadId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Default)]
pub(super) struct UploadLocks {
    locks: Mutex<HashMap<UploadId, Weak<AsyncMutex<()>>>>,
}

impl UploadLocks {
    pub(super) async fn lock(&self, id: &UploadId) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self
                .locks
                .lock()
                .expect("upload lock registry should not be poisoned");
            locks.retain(|_, lock| lock.strong_count() > 0);
            locks.get(id).and_then(Weak::upgrade).unwrap_or_else(|| {
                let lock = Arc::new(AsyncMutex::new(()));
                locks.insert(*id, Arc::downgrade(&lock));
                lock
            })
        };
        lock.lock_owned().await
    }
}
