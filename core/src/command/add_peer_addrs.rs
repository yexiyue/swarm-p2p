use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};

use crate::runtime::CborMessage;

use super::{CommandHandler, CoreSwarm, ResultHandle};

/// AddPeerAddrs 命令 - 将指定 peer 的地址注册到 Swarm 地址簿
pub struct AddPeerAddrsCommand {
    peer_id: PeerId,
    addrs: Vec<Multiaddr>,
}

impl AddPeerAddrsCommand {
    pub fn new(peer_id: PeerId, addrs: Vec<Multiaddr>) -> Self {
        Self { peer_id, addrs }
    }
}

#[async_trait]
impl<Req: CborMessage, Resp: CborMessage> CommandHandler<Req, Resp> for AddPeerAddrsCommand {
    type Result = ();

    async fn run(&mut self, swarm: &mut CoreSwarm<Req, Resp>, handle: &ResultHandle<Self::Result>) {
        for addr in &self.addrs {
            swarm.add_peer_address(self.peer_id, addr.clone());
        }
        handle.finish(Ok(()));
    }
}
