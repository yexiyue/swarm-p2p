use crate::Result;
use crate::command::gossipsub::{PublishCommand, SubscribeCommand, UnsubscribeCommand};
use crate::runtime::CborMessage;

use super::NetClient;
use super::future::CommandFuture;

impl<Req, Resp> NetClient<Req, Resp>
where
    Req: CborMessage,
    Resp: CborMessage,
{
    /// 订阅 GossipSub topic
    ///
    /// 返回 `true` 表示新订阅，`false` 表示已经订阅过
    pub async fn subscribe(&self, topic: impl Into<String>) -> Result<bool> {
        let cmd = SubscribeCommand::new(topic.into());
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    /// 退订 GossipSub topic
    ///
    /// 返回 `true` 表示已退订，`false` 表示本来就没订阅
    pub async fn unsubscribe(&self, topic: impl Into<String>) -> Result<bool> {
        let cmd = UnsubscribeCommand::new(topic.into());
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }

    /// 向 GossipSub topic 发布消息
    pub async fn publish(&self, topic: impl Into<String>, data: Vec<u8>) -> Result<()> {
        let cmd = PublishCommand::new(topic.into(), data);
        CommandFuture::new(cmd, self.command_tx.clone()).await
    }
}
