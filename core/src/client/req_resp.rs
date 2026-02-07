use libp2p::PeerId;

use crate::Result;
use crate::command::{CommandFuture, SendRequestCommand, SendResponseCommand};
use crate::runtime::CborMessage;

use super::NetClient;

impl<Req, Resp> NetClient<Req, Resp>
where
    Req: CborMessage,
    Resp: CborMessage,
{
    /// 发送请求并等待响应
    pub async fn send_request(&self, peer_id: PeerId, request: Req) -> Result<Resp>
    where
        Req: Unpin,
    {
        let cmd = SendRequestCommand::new(peer_id, request);
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    /// 回复一个 inbound request
    ///
    /// `pending_id` 来自 `NodeEvent::InboundRequest` 中的标识，
    /// 用于从 PendingMap 取出对应的 `ResponseChannel` 进行回复。
    pub async fn send_response(&self, pending_id: u64, response: Resp) -> Result<()>
    where
        Resp: Unpin,
    {
        let channel = self
            .pending_channels
            .take(&pending_id)
            .ok_or_else(|| crate::error::Error::Behaviour(
                format!("No pending channel for pending_id={} (expired or already responded)", pending_id),
            ))?;
        let cmd = SendResponseCommand::new(channel, response);
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }
}
