use std::time::Duration;

use libp2p::kad;
use serde::{Deserialize, Serialize};

/// DHT 查询统计信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryStatsInfo {
    /// 查询耗时
    pub duration: Option<Duration>,
    /// 发送的请求数
    pub num_requests: u32,
    /// 成功的请求数
    pub num_successes: u32,
    /// 失败的请求数
    pub num_failures: u32,
}

impl From<&kad::QueryStats> for QueryStatsInfo {
    fn from(value: &kad::QueryStats) -> Self {
        Self {
            duration: value.duration(),
            num_requests: value.num_requests(),
            num_successes: value.num_successes(),
            num_failures: value.num_failures(),
        }
    }
}
