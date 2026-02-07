use std::{
    collections::HashMap,
    hash::Hash,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use tokio::time;

struct PendingEntry<V> {
    value: V,
    created_at: Instant,
}

/// 带 TTL 自动清理的并发 Map
///
/// 适用于跨 task 按 key 存取、一次性消费的场景（如 ResponseChannel 暂存）。
/// 内部启动一个 tokio 定时任务，周期性清理过期条目。
///
/// 使用 `Mutex<HashMap>` 而非 DashMap，因为 value 类型（如 `ResponseChannel`）
/// 可能不满足 `Sync` 约束。对于低竞争场景完全够用。
pub struct PendingMap<K, V> {
    inner: Arc<Mutex<HashMap<K, PendingEntry<V>>>>,
}

impl<K, V> Clone for PendingMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<K, V> PendingMap<K, V>
where
    K: Eq + Hash + Send + 'static,
    V: Send + 'static,
{
    pub fn new(ttl: Duration) -> Self {
        let map = Arc::new(Mutex::new(HashMap::new()));
        let map_clone = Arc::clone(&map);

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(10));

            loop {
                interval.tick().await;
                let now = Instant::now();
                map_clone
                    .lock()
                    .retain(|_, v: &mut PendingEntry<V>| now.duration_since(v.created_at) < ttl);
            }
        });

        Self { inner: map }
    }

    pub fn insert(&self, key: K, value: V) {
        self.inner.lock().insert(
            key,
            PendingEntry {
                value,
                created_at: Instant::now(),
            },
        );
    }

    pub fn take(&self, key: &K) -> Option<V> {
        self.inner.lock().remove(key).map(|v| v.value)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}
