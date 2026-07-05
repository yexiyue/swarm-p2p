use async_trait::async_trait;
use libp2p::PeerId;

use crate::runtime::CborMessage;

use super::{CommandHandler, CoreSwarm, ResultHandle};

/// SetKeepAlive 命令 - 增删指定 peer 的连接保活白名单
///
/// 白名单内 peer 的连接不会被 idle_connection_timeout 空闲回收，
/// 供上层业务（如已配对设备）豁免空闲断连。
pub struct SetKeepAliveCommand {
    peer_id: PeerId,
    enabled: bool,
}

impl SetKeepAliveCommand {
    pub fn new(peer_id: PeerId, enabled: bool) -> Self {
        Self { peer_id, enabled }
    }
}

#[async_trait]
impl<Req: CborMessage, Resp: CborMessage> CommandHandler<Req, Resp> for SetKeepAliveCommand {
    type Result = ();

    async fn run(&mut self, swarm: &mut CoreSwarm<Req, Resp>, handle: &ResultHandle<Self::Result>) {
        swarm
            .behaviour_mut()
            .keep_alive
            .set_keep_alive(self.peer_id, self.enabled);
        handle.finish(Ok(()));
    }
}
