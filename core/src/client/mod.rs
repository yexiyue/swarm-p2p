mod future;
mod kad;
mod req_resp;

use libp2p::{Multiaddr, PeerId};
use tokio::sync::mpsc;

use crate::Result;
use crate::command::{
    AddPeerAddrsCommand, Command, DialCommand, DisconnectCommand, GetListenAddrsCommand,
    IsConnectedCommand,
};
use crate::event::NodeEvent;
use crate::pending_map::PendingMap;
use crate::runtime::CborMessage;
use future::CommandFuture;

/// 网络客户端，用于发送命令
pub struct NetClient<Req, Resp>
where
    Req: CborMessage,
    Resp: CborMessage,
{
    command_tx: mpsc::Sender<Command<Req, Resp>>,
    pending_channels: PendingMap<u64, libp2p::request_response::ResponseChannel<Resp>>,
}

impl<Req, Resp> Clone for NetClient<Req, Resp>
where
    Req: CborMessage,
    Resp: CborMessage,
{
    fn clone(&self) -> Self {
        Self {
            command_tx: self.command_tx.clone(),
            pending_channels: self.pending_channels.clone(),
        }
    }
}

impl<Req, Resp> NetClient<Req, Resp>
where
    Req: CborMessage,
    Resp: CborMessage,
{
    pub(crate) fn new(
        command_tx: mpsc::Sender<Command<Req, Resp>>,
        pending_channels: PendingMap<u64, libp2p::request_response::ResponseChannel<Resp>>,
    ) -> Self {
        Self {
            command_tx,
            pending_channels,
        }
    }

    /// 连接到指定 peer
    pub async fn dial(&self, peer_id: PeerId) -> Result<()> {
        let cmd = DialCommand::new(peer_id);
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    /// 检查是否已连接到指定 peer
    pub async fn is_connected(&self, peer_id: PeerId) -> Result<bool> {
        let cmd = IsConnectedCommand::new(peer_id);
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    /// 断开与指定 peer 的所有连接
    pub async fn disconnect(&self, peer_id: PeerId) -> Result<()> {
        let cmd = DisconnectCommand::new(peer_id);
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    /// 获取本节点的所有可达地址（监听地址 + 外部地址）
    pub async fn get_addrs(&self) -> Result<Vec<Multiaddr>> {
        let cmd = GetListenAddrsCommand::new();
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    /// 将指定 peer 的地址注册到 Swarm 地址簿
    pub async fn add_peer_addrs(&self, peer_id: PeerId, addrs: Vec<Multiaddr>) -> Result<()> {
        let cmd = AddPeerAddrsCommand::new(peer_id, addrs);
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    pub fn shutdown(self) {
        drop(self.command_tx);
    }
}

/// 事件接收器
pub struct EventReceiver<Req = ()> {
    event_rx: mpsc::Receiver<NodeEvent<Req>>,
}

impl<Req> EventReceiver<Req> {
    pub(crate) fn new(event_rx: mpsc::Receiver<NodeEvent<Req>>) -> Self {
        Self { event_rx }
    }

    /// 接收下一个事件
    pub async fn recv(&mut self) -> Option<NodeEvent<Req>> {
        self.event_rx.recv().await
    }
}
