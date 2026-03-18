use async_trait::async_trait;
use libp2p::gossipsub::IdentTopic;

use crate::error::Error;
use crate::runtime::CborMessage;

use super::super::{CommandHandler, CoreSwarm, ResultHandle};

/// Subscribe 命令 - 订阅 GossipSub topic
pub struct SubscribeCommand {
    topic: String,
}

impl SubscribeCommand {
    pub fn new(topic: String) -> Self {
        Self { topic }
    }
}

#[async_trait]
impl<Req: CborMessage, Resp: CborMessage> CommandHandler<Req, Resp> for SubscribeCommand {
    type Result = bool;

    async fn run(&mut self, swarm: &mut CoreSwarm<Req, Resp>, handle: &ResultHandle<Self::Result>) {
        let Some(gossipsub) = swarm.behaviour_mut().gossipsub.as_mut() else {
            handle.finish(Err(Error::Behaviour("GossipSub is disabled".into())));
            return;
        };
        let topic = IdentTopic::new(&self.topic);
        match gossipsub.subscribe(&topic) {
            Ok(subscribed) => handle.finish(Ok(subscribed)),
            Err(e) => handle.finish(Err(Error::Behaviour(format!(
                "GossipSub subscribe failed: {}",
                e
            )))),
        }
    }
}
