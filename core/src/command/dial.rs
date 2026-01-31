use async_trait::async_trait;
use libp2p::{swarm::SwarmEvent, PeerId};

use crate::error::Error;
use crate::runtime::CoreBehaviourEvent;

use super::{CommandHandler, CoreSwarm, ResultHandle};

/// Dial 命令 - 连接到指定 peer
pub struct DialCommand {
    peer_id: PeerId,
}

impl DialCommand {
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id }
    }
}

#[async_trait]
impl CommandHandler for DialCommand {
    type Result = ();

    async fn run(&mut self, swarm: &mut CoreSwarm, handle: &ResultHandle<Self::Result>) {
        if let Err(e) = swarm.dial(self.peer_id) {
            handle.finish(Err(Error::Dial(format!(
                "failed to dial {}: {}",
                self.peer_id, e
            ))));
        }
    }

    async fn on_event(
        &mut self,
        event: &SwarmEvent<CoreBehaviourEvent>,
        handle: &ResultHandle<Self::Result>,
    ) -> bool {
        match event {
            SwarmEvent::ConnectionEstablished { peer_id, .. } if *peer_id == self.peer_id => {
                handle.finish(Ok(()));
                false // 完成
            }
            SwarmEvent::OutgoingConnectionError {
                peer_id: Some(peer_id),
                error,
                ..
            } if *peer_id == self.peer_id => {
                handle.finish(Err(Error::Dial(format!(
                    "failed to dial {}: {}",
                    self.peer_id, error
                ))));
                false // 完成
            }
            _ => true, // 继续等待
        }
    }
}
