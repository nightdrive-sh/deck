//! Responses kept in memory, so that the same request is not sent twice.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError},
};

use anyhow::Result;
use tokio::sync::OnceCell;

/// Locks the memory. A poisoned lock is fine: the contents are always intact.
fn lock<T>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Responses kept in memory by key. Concurrent callers asking for the same key wait
/// for a single fetch. A failed fetch is not remembered, so it is retried the next
/// time. When full, the oldest key is forgotten.
pub(super) struct Memo<T> {
    capacity: usize,
    entries: StdMutex<MemoEntries<T>>,
}

struct MemoEntries<T> {
    cells: HashMap<String, Arc<OnceCell<T>>>,
    /// Keys in insertion order, oldest first.
    order: VecDeque<String>,
}

impl<T: Clone> Memo<T> {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: StdMutex::new(MemoEntries {
                cells: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    pub(super) async fn get_or_fetch<F, Fut>(&self, key: &str, fetch: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        self.cell(key).get_or_try_init(fetch).await.cloned()
    }

    fn cell(&self, key: &str) -> Arc<OnceCell<T>> {
        let mut entries = lock(&self.entries);
        if let Some(cell) = entries.cells.get(key) {
            return cell.clone();
        }
        while entries.order.len() >= self.capacity {
            let Some(oldest) = entries.order.pop_front() else {
                break;
            };
            entries.cells.remove(&oldest);
        }
        let cell = Arc::new(OnceCell::new());
        entries.cells.insert(key.to_owned(), cell.clone());
        entries.order.push_back(key.to_owned());
        cell
    }
}
