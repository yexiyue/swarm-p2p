use async_trait::async_trait;
use libp2p::kad::RecordKey;

use super::super::{CommandHandler, CoreSwarm, ResultHandle};

pub struct StopProvideCommand {
    key: RecordKey,
}

impl StopProvideCommand {
    pub fn new(key: RecordKey) -> Self {
        Self { key }
    }
}

#[async_trait]
impl CommandHandler for StopProvideCommand {
    type Result = ();

    async fn run(&mut self, swarm: &mut CoreSwarm, handle: &ResultHandle<Self::Result>) {
        swarm.behaviour_mut().kad.stop_providing(&self.key);
        handle.finish(Ok(()));
    }
}
