use async_trait::async_trait;
use libp2p::kad::{self, Record};
use libp2p::swarm::SwarmEvent;
use tracing::{error, info};

use crate::error::Error;
use crate::runtime::CoreBehaviourEvent;
use crate::util::QueryStatsInfo;

use super::super::{CommandHandler, CoreSwarm, ResultHandle};

pub struct PutRecordCommand {
    record: Record,
    query_id: Option<kad::QueryId>,
    stats: Option<kad::QueryStats>,
}

impl PutRecordCommand {
    pub fn new(record: Record) -> Self {
        Self {
            record,
            query_id: None,
            stats: None,
        }
    }
}

#[async_trait]
impl CommandHandler for PutRecordCommand {
    type Result = QueryStatsInfo;

    async fn run(&mut self, swarm: &mut CoreSwarm, handle: &ResultHandle<Self::Result>) {
        match swarm
            .behaviour_mut()
            .kad
            .put_record(self.record.clone(), kad::Quorum::One)
        {
            Ok(query_id) => {
                self.query_id = Some(query_id);
            }
            Err(e) => {
                handle.finish(Err(e.into()));
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
            result: kad::QueryResult::PutRecord(res),
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

        // 非最后一步，继续等待
        if !step.last {
            return true;
        }

        // 查询完成，处理结果
        let stats_info = QueryStatsInfo::from(self.stats.as_ref().unwrap());
        match res {
            Ok(_) => {
                info!("PutRecord success: {:?}", stats_info);
                handle.finish(Ok(stats_info));
            }
            Err(e) => {
                error!("PutRecord error: {:?}", e);
                handle.finish(Err(Error::KadPutRecord(format!("{:?}", e))));
            }
        }

        false // 完成
    }
}
