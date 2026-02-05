use async_trait::async_trait;
use libp2p::kad::RecordKey;

use super::super::{CommandHandler, CoreSwarm, ResultHandle};

/// RemoveRecord 命令 - 从本地存储中删除记录
pub struct RemoveRecordCommand {
    key: RecordKey,
}

impl RemoveRecordCommand {
    pub fn new(key: RecordKey) -> Self {
        Self { key }
    }
}

#[async_trait]
impl CommandHandler for RemoveRecordCommand {
    type Result = ();

    async fn run(&mut self, swarm: &mut CoreSwarm, handle: &ResultHandle<Self::Result>) {
        swarm.behaviour_mut().kad.remove_record(&self.key);
        handle.finish(Ok(()));
    }
}
