use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use tracing::{info, warn};

use crate::runtime::CborMessage;

use super::{CommandHandler, CoreSwarm, ResultHandle};

/// 幂等地确保对指定 relay 持有 reservation。
///
/// 地址常驻登记：事件循环在每次 identify 该 peer 时幂等触发
/// reservation 请求（已有活跃 circuit listener 则 no-op），实现
/// 断线后随重连自动重建。未连接时顺带发起 dial。
pub struct EnsureRelayReservationCommand {
    peer_id: PeerId,
    addrs: Vec<Multiaddr>,
    pending_relay_addrs: Option<Vec<Multiaddr>>,
}

impl EnsureRelayReservationCommand {
    pub fn new(peer_id: PeerId, addrs: Vec<Multiaddr>) -> Self {
        Self {
            peer_id,
            addrs,
            pending_relay_addrs: None,
        }
    }
}

#[async_trait]
impl<Req: CborMessage, Resp: CborMessage> CommandHandler<Req, Resp>
    for EnsureRelayReservationCommand
{
    type Result = ();

    async fn run(&mut self, swarm: &mut CoreSwarm<Req, Resp>, handle: &ResultHandle<Self::Result>) {
        if !swarm.behaviour().relay_client.is_enabled() {
            handle.finish(Ok(()));
            return;
        }
        for addr in &self.addrs {
            swarm.add_peer_address(self.peer_id, addr.clone());
        }
        self.pending_relay_addrs = Some(self.addrs.clone());

        if !swarm.is_connected(&self.peer_id) {
            match swarm.dial(self.peer_id) {
                Ok(()) => info!("Dialing relay {} for reservation", self.peer_id),
                Err(e) => warn!("Failed to dial relay {}: {}", self.peer_id, e),
            }
        }
        handle.finish(Ok(()));
    }

    fn take_relay_reservation_request(&mut self) -> Option<(PeerId, Vec<Multiaddr>)> {
        self.pending_relay_addrs
            .take()
            .map(|addrs| (self.peer_id, addrs))
    }
}
