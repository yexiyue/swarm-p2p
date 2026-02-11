use async_trait::async_trait;
use libp2p::Multiaddr;

use crate::runtime::CborMessage;

use super::{CommandHandler, CoreSwarm, ResultHandle};

/// GetListenAddrs 命令 - 获取本节点的所有可达地址（监听地址 + 外部地址）
pub struct GetListenAddrsCommand;

impl GetListenAddrsCommand {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl<Req: CborMessage, Resp: CborMessage> CommandHandler<Req, Resp> for GetListenAddrsCommand {
    type Result = Vec<Multiaddr>;

    async fn run(&mut self, swarm: &mut CoreSwarm<Req, Resp>, handle: &ResultHandle<Self::Result>) {
        let mut addrs: Vec<Multiaddr> = swarm.listeners().cloned().collect();
        addrs.extend(swarm.external_addresses().cloned());
        addrs.sort();
        addrs.dedup();
        handle.finish(Ok(addrs));
    }
}
