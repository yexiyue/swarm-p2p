mod bootstrap;
mod get_closest_peers;
mod get_providers;
mod get_record;
mod put_record;
mod remove_record;
mod start_provide;
mod stop_provide;

pub use bootstrap::*;
pub use get_closest_peers::*;
pub use get_providers::*;
pub use get_record::*;
pub use put_record::*;
pub use remove_record::*;
pub use start_provide::*;
pub use stop_provide::*;

use libp2p::kad;

/// 累积 Kad 查询统计（多步查询中每步都会产生新的 stats）
fn merge_stats(existing: &mut Option<kad::QueryStats>, incoming: kad::QueryStats) {
    *existing = Some(match existing.take() {
        Some(s) => s.merge(incoming),
        None => incoming,
    });
}
