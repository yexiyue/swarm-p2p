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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_and_take() {
        let map = PendingMap::new(Duration::from_secs(60));
        map.insert(1u64, "hello");
        map.insert(2, "world");

        assert_eq!(map.len(), 2);
        assert_eq!(map.take(&1), Some("hello"));
        assert_eq!(map.len(), 1);
        assert_eq!(map.take(&1), None); // 已经取出，不能再取
    }

    #[tokio::test]
    async fn take_nonexistent_returns_none() {
        let map = PendingMap::<u64, String>::new(Duration::from_secs(60));
        assert_eq!(map.take(&999), None);
        assert!(map.is_empty());
    }

    #[tokio::test]
    async fn clone_shares_state() {
        let map = PendingMap::new(Duration::from_secs(60));
        let map2 = map.clone();

        map.insert(1u64, "value");
        assert_eq!(map2.take(&1), Some("value")); // clone 共享底层数据
        assert!(map.is_empty());
    }

    #[tokio::test]
    async fn ttl_expiry_cleans_up() {
        // TTL = 1ms，后台清理任务的首次 tick 立即执行
        // sleep 后让出执行权，清理任务会移除过期条目
        let map = PendingMap::new(Duration::from_millis(1));
        map.insert(1u64, "ephemeral");
        assert_eq!(map.len(), 1);

        // 等待超过 TTL，并让出执行权给清理任务
        tokio::time::sleep(Duration::from_millis(50)).await;
        tokio::task::yield_now().await;

        // 过期条目应被后台任务清理
        assert!(map.is_empty(), "expired entry should be cleaned up");
    }

    #[tokio::test]
    async fn non_expired_entries_survive_cleanup() {
        // TTL 足够长，条目不会被清理
        let map = PendingMap::new(Duration::from_secs(60));
        map.insert(1u64, "durable");

        tokio::task::yield_now().await;

        assert_eq!(map.len(), 1);
        assert_eq!(map.take(&1), Some("durable"));
    }
}
