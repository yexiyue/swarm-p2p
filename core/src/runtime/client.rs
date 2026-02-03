use libp2p::PeerId;
use tokio::sync::mpsc;

use crate::Result;
use crate::command::{Command, CommandFuture, DialCommand};
use crate::event::NodeEvent;

/// 网络客户端，用于发送命令
#[derive(Clone)]
pub struct NetClient {
    command_tx: mpsc::Sender<Command>,
}

impl NetClient {
    pub(crate) fn new(command_tx: mpsc::Sender<Command>) -> Self {
        Self { command_tx }
    }

    /// 连接到指定 peer
    pub async fn dial(&self, peer_id: PeerId) -> Result<()> {
        let cmd = DialCommand::new(peer_id);
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    pub fn shutdown(self) {
        drop(self.command_tx);
    }
}

/// 事件接收器
pub struct EventReceiver {
    event_rx: mpsc::Receiver<NodeEvent>,
}

impl EventReceiver {
    pub(crate) fn new(event_rx: mpsc::Receiver<NodeEvent>) -> Self {
        Self { event_rx }
    }

    /// 接收下一个事件
    pub async fn recv(&mut self) -> Option<NodeEvent> {
        self.event_rx.recv().await
    }
}
