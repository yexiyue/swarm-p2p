use async_trait::async_trait;
use libp2p::gossipsub::IdentTopic;

use crate::error::Error;
use crate::runtime::CborMessage;

use super::super::{CommandHandler, CoreSwarm, ResultHandle};

/// Unsubscribe 命令 - 退订 GossipSub topic
pub struct UnsubscribeCommand {
    topic: String,
}

impl UnsubscribeCommand {
    pub fn new(topic: String) -> Self {
        Self { topic }
    }
}

#[async_trait]
impl<Req: CborMessage, Resp: CborMessage> CommandHandler<Req, Resp> for UnsubscribeCommand {
    type Result = bool;

    async fn run(&mut self, swarm: &mut CoreSwarm<Req, Resp>, handle: &ResultHandle<Self::Result>) {
        let Some(gossipsub) = swarm.behaviour_mut().gossipsub.as_mut() else {
            handle.finish(Err(Error::Behaviour("GossipSub is disabled".into())));
            return;
        };
        let topic = IdentTopic::new(&self.topic);
        let unsubscribed = gossipsub.unsubscribe(&topic);
        handle.finish(Ok(unsubscribed));
    }
}
