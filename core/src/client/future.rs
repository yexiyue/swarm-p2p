use std::task::{Context, Poll};

use crate::Result;
use crate::command::{Command, CommandHandler, CommandTask, ResultHandle};
use crate::runtime::CborMessage;

/// 命令 Future，使任意 CommandHandler 可被 await
pub struct CommandFuture<T, Req, Resp>
where
    T: CommandHandler<Req, Resp> + Send + 'static,
    Req: CborMessage,
    Resp: CborMessage,
{
    handler: Option<T>,
    handle: ResultHandle<T::Result>,
    sender: tokio::sync::mpsc::Sender<Command<Req, Resp>>,
}

impl<T, Req, Resp> CommandFuture<T, Req, Resp>
where
    T: CommandHandler<Req, Resp> + Send + 'static,
    T::Result: Send + 'static,
    Req: CborMessage,
    Resp: CborMessage,
{
    pub fn new(handler: T, sender: tokio::sync::mpsc::Sender<Command<Req, Resp>>) -> Self {
        Self {
            handler: Some(handler),
            handle: ResultHandle::new(),
            sender,
        }
    }
}

impl<T, Req, Resp> std::future::Future for CommandFuture<T, Req, Resp>
where
    T: CommandHandler<Req, Resp> + Send + Unpin + 'static,
    T::Result: Send + 'static,
    Req: CborMessage,
    Resp: CborMessage,
{
    type Output = Result<T::Result>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // 首次 poll 时发送命令
        if let Some(handler) = this.handler.take() {
            let task = CommandTask::new(handler, this.handle.clone());
            match this.sender.try_send(Box::new(task)) {
                Ok(_) => return Poll::Pending,
                Err(_) => {
                    return Poll::Ready(Err(crate::error::Error::Behaviour(
                        "command channel closed".into(),
                    )));
                }
            }
        }

        // 后续 poll 检查结果
        this.handle.poll(cx)
    }
}
