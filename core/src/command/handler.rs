use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use libp2p::Swarm;
use libp2p::swarm::SwarmEvent;

use crate::Result;
use crate::runtime::{CoreBehaviour, CoreBehaviourEvent};

/// Swarm 类型别名
pub type CoreSwarm = Swarm<CoreBehaviour>;

/// 命令结果句柄，用于命令完成时返回结果
#[derive(Debug)]
pub struct ResultHandle<T>(Arc<Mutex<ResultState<T>>>);

#[derive(Debug)]
struct ResultState<T> {
    result: Option<Result<T>>,
    waker: Option<Waker>,
}

impl<T> Default for ResultState<T> {
    fn default() -> Self {
        Self {
            result: None,
            waker: None,
        }
    }
}

impl<T> Clone for ResultHandle<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> ResultHandle<T> {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(ResultState::default())))
    }

    pub fn poll(&self, cx: &Context<'_>) -> Poll<Result<T>> {
        let mut state = self.0.lock();
        if let Some(result) = state.result.take() {
            Poll::Ready(result)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    /// 完成命令并返回结果
    pub fn finish(&self, result: Result<T>) {
        let mut state = self.0.lock();
        state.result = Some(result);
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }
}

/// 命令处理器 trait
#[async_trait]
pub trait CommandHandler: Send + 'static {
    type Result: Send + 'static;

    /// 执行命令
    async fn run(&mut self, swarm: &mut CoreSwarm, handle: &ResultHandle<Self::Result>);

    /// 处理 swarm 事件，返回 true 继续等待，false 完成
    async fn on_event(
        &mut self,
        _event: &SwarmEvent<CoreBehaviourEvent>,
        _handle: &ResultHandle<Self::Result>,
    ) -> bool {
        false
    }
}

/// 命令 trait object 包装
pub type Command = Box<dyn CommandTrait + Send>;

/// 用于 trait object 的命令接口
#[async_trait]
pub trait CommandTrait: Send {
    async fn run_boxed(&mut self, swarm: &mut CoreSwarm);
    async fn on_event_boxed(&mut self, event: &SwarmEvent<CoreBehaviourEvent>) -> bool;
}

/// 命令任务，包装 CommandHandler + ResultHandle
pub struct CommandTask<T>
where
    T: CommandHandler,
{
    handler: T,
    handle: ResultHandle<T::Result>,
}

impl<T> CommandTask<T>
where
    T: CommandHandler,
{
    pub fn new(handler: T, handle: ResultHandle<T::Result>) -> Self {
        Self { handler, handle }
    }
}

#[async_trait]
impl<T> CommandTrait for CommandTask<T>
where
    T: CommandHandler + Send + 'static,
    T::Result: Send + 'static,
{
    async fn run_boxed(&mut self, swarm: &mut CoreSwarm) {
        self.handler.run(swarm, &self.handle).await;
    }

    async fn on_event_boxed(&mut self, event: &SwarmEvent<CoreBehaviourEvent>) -> bool {
        self.handler.on_event(event, &self.handle).await
    }
}

/// 命令 Future，使任意 CommandHandler 可被 await
pub struct CommandFuture<T>
where
    T: CommandHandler + Send + 'static,
{
    handler: Option<T>,
    handle: ResultHandle<T::Result>,
    sender: tokio::sync::mpsc::Sender<Command>,
}

impl<T> CommandFuture<T>
where
    T: CommandHandler + Send + 'static,
    T::Result: Send + 'static,
{
    pub fn new(handler: T, sender: tokio::sync::mpsc::Sender<Command>) -> Self {
        Self {
            handler: Some(handler),
            handle: ResultHandle::new(),
            sender,
        }
    }
}

impl<T> std::future::Future for CommandFuture<T>
where
    T: CommandHandler + Send + Unpin + 'static,
    T::Result: Send + 'static,
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
