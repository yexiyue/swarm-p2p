use async_trait::async_trait;
use libp2p::PeerId;

use crate::runtime::CborMessage;

use super::{CommandHandler, CoreSwarm, ResultHandle};

/// IsConnected 命令 - 检查是否已连接到指定 peer
pub struct IsConnectedCommand {
    peer_id: PeerId,
}

impl IsConnectedCommand {
    pub fn new(peer_id: PeerId) -> Self {
        Self { peer_id }
    }
}

#[async_trait]
impl<Req: CborMessage, Resp: CborMessage> CommandHandler<Req, Resp> for IsConnectedCommand {
    type Result = bool;

    async fn run(&mut self, swarm: &mut CoreSwarm<Req, Resp>, handle: &ResultHandle<Self::Result>) {
        handle.finish(Ok(swarm.is_connected(&self.peer_id)));
    }
}
