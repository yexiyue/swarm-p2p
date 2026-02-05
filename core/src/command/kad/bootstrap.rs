use async_trait::async_trait;
use libp2p::kad::{self, QueryId};
use libp2p::swarm::SwarmEvent;
use tracing::{error, info};

use crate::error::Error;
use crate::runtime::CoreBehaviourEvent;
use crate::util::QueryStatsInfo;

use super::super::{CommandHandler, CoreSwarm, ResultHandle};

/// Bootstrap 命令结果
#[derive(Debug, Clone)]
pub struct BootstrapResult {
    /// 剩余待查询的节点数量
    pub num_remaining: u32,
    /// 查询统计信息
    pub stats: QueryStatsInfo,
}

/// Bootstrap 命令 - 加入 DHT 网络，填充路由表
pub struct BootstrapCommand {
    query_id: Option<QueryId>,
    stats: Option<kad::QueryStats>,
}

impl BootstrapCommand {
    pub fn new() -> Self {
        Self {
            query_id: None,
            stats: None,
        }
    }
}

impl Default for BootstrapCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandHandler for BootstrapCommand {
    type Result = BootstrapResult;

    async fn run(&mut self, swarm: &mut CoreSwarm, handle: &ResultHandle<Self::Result>) {
        match swarm.behaviour_mut().kad.bootstrap() {
            Ok(query_id) => {
                self.query_id = Some(query_id);
                info!("Bootstrap started, query_id: {:?}", query_id);
            }
            Err(e) => {
                error!("Bootstrap failed to start: {:?}", e);
                handle.finish(Err(Error::Behaviour(format!(
                    "Bootstrap failed: no known peers"
                ))));
            }
        }
    }

    async fn on_event(
        &mut self,
        event: &SwarmEvent<CoreBehaviourEvent>,
        handle: &ResultHandle<Self::Result>,
    ) -> bool {
        // 只处理 Kademlia OutboundQueryProgressed 事件
        let SwarmEvent::Behaviour(CoreBehaviourEvent::Kad(kad::Event::OutboundQueryProgressed {
            id,
            result: kad::QueryResult::Bootstrap(res),
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
            Ok(kad::BootstrapOk {
                peer,
                num_remaining,
            }) => {
                info!(
                    "Bootstrap progress: peer {:?}, {} remaining",
                    peer, num_remaining
                );
            }
            Err(e) => {
                error!("Bootstrap error: {:?}", e);
                handle.finish(Err(Error::Behaviour(format!("Bootstrap error: {:?}", e))));
                return false;
            }
        }

        // 非最后一步，继续等待
        if !step.last {
            return true;
        }

        // Bootstrap 完成
        let stats_info = QueryStatsInfo::from(self.stats.as_ref().unwrap());
        info!("Bootstrap completed: {:?}", stats_info);

        // 获取最后一次的 num_remaining
        let num_remaining = match res {
            Ok(kad::BootstrapOk { num_remaining, .. }) => *num_remaining,
            Err(_) => 0,
        };

        handle.finish(Ok(BootstrapResult {
            num_remaining,
            stats: stats_info,
        }));

        false // 完成
    }
}
