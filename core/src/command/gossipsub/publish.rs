use async_trait::async_trait;
use libp2p::gossipsub::IdentTopic;

use crate::error::Error;
use crate::runtime::CborMessage;

use super::super::{CommandHandler, CoreSwarm, ResultHandle};

/// Publish 命令 - 向 GossipSub topic 发布消息
pub struct PublishCommand {
    topic: String,
    data: Option<Vec<u8>>,
}

impl PublishCommand {
    pub fn new(topic: String, data: Vec<u8>) -> Self {
        Self {
            topic,
            data: Some(data),
        }
    }
}

#[async_trait]
impl<Req: CborMessage, Resp: CborMessage> CommandHandler<Req, Resp> for PublishCommand {
    type Result = ();

    async fn run(&mut self, swarm: &mut CoreSwarm<Req, Resp>, handle: &ResultHandle<Self::Result>) {
        let Some(gossipsub) = swarm.behaviour_mut().gossipsub.as_mut() else {
            handle.finish(Err(Error::Behaviour("GossipSub is disabled".into())));
            return;
        };
        let Some(data) = self.data.take() else {
            handle.finish(Err(Error::Behaviour(
                "PublishCommand: run called twice".into(),
            )));
            return;
        };
        let topic = IdentTopic::new(&self.topic);
        match gossipsub.publish(topic, data) {
            Ok(_message_id) => handle.finish(Ok(())),
            Err(e) => handle.finish(Err(Error::Behaviour(format!(
                "GossipSub publish failed: {}",
                e
            )))),
        }
    }
}
