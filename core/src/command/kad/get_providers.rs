use async_trait::async_trait;
use libp2p::PeerId;
use libp2p::kad::{self, RecordKey};
use libp2p::swarm::SwarmEvent;
use tracing::{error, info};

use crate::error::Error;
use crate::runtime::CoreBehaviourEvent;
use crate::util::QueryStatsInfo;

use super::super::{CommandHandler, CoreSwarm, ResultHandle};

/// GetProviders 命令结果
#[derive(Debug, Clone)]
pub struct GetProvidersResult {
    /// 找到的 Provider PeerId 列表
    pub providers: Vec<PeerId>,
    /// 查询统计信息
    pub stats: QueryStatsInfo,
}

pub struct GetProvidersCommand {
    key: RecordKey,
    query_id: Option<kad::QueryId>,
    providers: Vec<PeerId>,
    stats: Option<kad::QueryStats>,
}

impl GetProvidersCommand {
    pub fn new(key: RecordKey) -> Self {
        Self {
            key,
            query_id: None,
            providers: Vec::new(),
            stats: None,
        }
    }
}

#[async_trait]
impl CommandHandler for GetProvidersCommand {
    type Result = GetProvidersResult;

    async fn run(&mut self, swarm: &mut CoreSwarm, _handle: &ResultHandle<Self::Result>) {
        let query_id = swarm.behaviour_mut().kad.get_providers(self.key.clone());
        self.query_id = Some(query_id);
    }

    async fn on_event(
        &mut self,
        event: &SwarmEvent<CoreBehaviourEvent>,
        handle: &ResultHandle<Self::Result>,
    ) -> bool {
        // 只处理 Kademlia OutboundQueryProgressed 事件
        let SwarmEvent::Behaviour(CoreBehaviourEvent::Kad(kad::Event::OutboundQueryProgressed {
            id,
            result: kad::QueryResult::GetProviders(res),
            stats,
            step,
        })) = event
        else {
            return true; // 继续等待
        };

        // 检查是否是我们的查询
        if self.query_id != Some(*id) {
            return true;
        }

        // 累积统计
        self.stats = Some(match self.stats.take() {
            Some(s) => s.merge(stats.clone()),
            None => stats.clone(),
        });

        // 处理结果
        match res {
            Ok(kad::GetProvidersOk::FoundProviders { providers, .. }) => {
                // 收集 providers
                self.providers.extend(providers.iter().cloned());
                info!(
                    "GetProviders progress: found {} providers so far",
                    self.providers.len()
                );
            }
            Ok(kad::GetProvidersOk::FinishedWithNoAdditionalRecord { closest_peers }) => {
                // 查询结束，closest_peers 是最近的节点（不一定是 provider）
                info!(
                    "GetProviders finished, {} closest peers",
                    closest_peers.len()
                );
            }
            Err(e) => {
                error!("GetProviders error: {:?}", e);
                handle.finish(Err(Error::KadGetProviders(format!("{:?}", e))));
                return false;
            }
        }

        // 非最后一步，继续等待
        if !step.last {
            return true;
        }

        // 查询完成
        let stats_info = QueryStatsInfo::from(self.stats.as_ref().unwrap());
        info!(
            "GetProviders completed: {} providers, {:?}",
            self.providers.len(),
            stats_info
        );

        handle.finish(Ok(GetProvidersResult {
            providers: std::mem::take(&mut self.providers),
            stats: stats_info,
        }));

        false // 完成
    }
}
