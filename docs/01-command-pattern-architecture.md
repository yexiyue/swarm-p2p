# 深入理解 libp2p 事件驱动架构中的命令模式

在构建 P2P 网络应用时，我们面临一个核心挑战：**如何优雅地在异步事件循环中执行请求-响应式操作？**

本文将详细介绍 `swarm-p2p` 项目中采用的命令模式架构，这是一种将命令执行与事件响应完美结合的设计。

## 问题背景

### libp2p 的事件驱动模型

libp2p 的 Swarm 是一个事件驱动系统，典型的使用方式是：

```rust
loop {
    let event = swarm.select_next_some().await;
    match event {
        SwarmEvent::ConnectionEstablished { .. } => { ... }
        SwarmEvent::ConnectionClosed { .. } => { ... }
        SwarmEvent::Behaviour(event) => { ... }
    }
}
```

### 典型需求：Dial 一个 Peer

当我们想连接到某个 Peer 时，期望的 API 是：

```rust
// 期望：简单的 async 调用
client.dial(peer_id).await?;
```

但 libp2p 的实际流程是：

```mermaid
sequenceDiagram
    participant Caller as 调用方
    participant Swarm
    participant Network as 网络

    Caller->>Swarm: swarm.dial(peer_id)
    Note right of Swarm: 只是发起连接，不等待
    Swarm->>Network: TCP 握手
    Network-->>Swarm: 连接建立
    Note over Swarm: 事件循环中产生<br/>ConnectionEstablished 事件
    Swarm--xCaller: ??? 如何通知调用方？
```

**问题：** `swarm.dial()` 只是发起连接，不会等待结果。结果以事件形式在事件循环中返回。

## 解决方案：命令模式

### 核心思想

将"发起操作"和"等待结果"封装成一个可 await 的命令：

```mermaid
flowchart TB
    subgraph Caller["调用方"]
        A["CommandFuture::new(DialCommand, sender).await"]
    end

    subgraph Channel["Channel"]
        B["mpsc::channel"]
    end

    subgraph EventLoop["EventLoop"]
        C["接收命令"]
        D["cmd.run(swarm)"]
        E["active_commands.push(cmd)"]
        F["等待 swarm 事件"]
        G{"ConnectionEstablished?"}
        H["cmd.on_event() → 完成"]
    end

    A --> B
    B --> C
    C --> D
    D --> E
    E --> F
    F --> G
    G -->|是| H
    G -->|否| F
    H -.->|通过 ResultHandle| A
```

### 架构分层

```mermaid
flowchart TB
    subgraph Layer1["Layer 1: 调用层"]
        NC["NetClient"]
        NC --> |"dial(peer_id)"| NC
        NC --> |"close(peer_id)"| NC
    end

    subgraph Layer2["Layer 2: 命令层"]
        CH["CommandHandler (trait)"]
        CH --> |run| CH
        CH --> |on_event| CH

        DC["DialCommand"]
        CC["CloseCommand"]
        SC["SendCommand"]
    end

    subgraph Layer3["Layer 3: 运行时层"]
        EL["EventLoop"]
        SW["Swarm"]
        AC["active_commands"]
    end

    Layer1 --> Layer2
    Layer2 --> Layer3
```

## 核心组件详解

### 1. ResultHandle - 结果句柄

`ResultHandle` 是命令与调用方之间的桥梁，负责传递执行结果：

```rust
pub struct ResultHandle<T>(Arc<Mutex<ResultState<T>>>);

struct ResultState<T> {
    result: Option<Result<T>>,  // 存储结果
    waker: Option<Waker>,       // 用于唤醒 Future
}
```

**工作流程：**

```mermaid
sequenceDiagram
    participant CF as CommandFuture
    participant RH as ResultHandle
    participant CT as CommandTask

    CF->>RH: poll() - 首次
    RH-->>CF: 保存 waker, 返回 Pending

    Note over CT: 命令执行完成
    CT->>RH: handle.finish(Ok(()))
    RH->>RH: 存储 result
    RH->>CF: waker.wake()

    CF->>RH: poll() - 再次
    RH-->>CF: 返回 Ready(result)
```

### 2. CommandHandler - 命令处理器 Trait

每个命令需要实现的接口：

```rust
#[async_trait]
pub trait CommandHandler: Send + 'static {
    type Result: Send + 'static;

    /// 执行命令（如：调用 swarm.dial）
    async fn run(&mut self, swarm: &mut CoreSwarm, handle: &ResultHandle<Self::Result>);

    /// 处理 swarm 事件，返回 true 继续等待，false 完成
    async fn on_event(
        &mut self,
        event: &SwarmEvent<CoreBehaviourEvent>,
        handle: &ResultHandle<Self::Result>,
    ) -> bool {
        false  // 默认不等待事件
    }
}
```

### 3. DialCommand 示例

```rust
pub struct DialCommand {
    peer_id: PeerId,
}

#[async_trait]
impl CommandHandler for DialCommand {
    type Result = ();

    async fn run(&mut self, swarm: &mut CoreSwarm, handle: &ResultHandle<()>) {
        if let Err(e) = swarm.dial(self.peer_id) {
            handle.finish(Err(Error::Dial(...))).await;
        }
    }

    async fn on_event(&mut self, event: &SwarmEvent<...>, handle: &ResultHandle<()>) -> bool {
        match event {
            SwarmEvent::ConnectionEstablished { peer_id, .. }
                if *peer_id == self.peer_id => {
                handle.finish(Ok(())).await;
                false  // 完成
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. }
                if peer_id == Some(self.peer_id) => {
                handle.finish(Err(Error::Dial(...))).await;
                false  // 完成
            }
            _ => true  // 继续等待
        }
    }
}
```

**DialCommand 时序图：**

```mermaid
sequenceDiagram
    participant Client
    participant EventLoop
    participant DialCmd
    participant Swarm

    Client->>EventLoop: dial(peer_id)
    EventLoop->>DialCmd: cmd.run(swarm)
    DialCmd->>Swarm: swarm.dial(peer_id)
    EventLoop->>EventLoop: push to active_commands

    Note over EventLoop: 等待 swarm 事件...

    Swarm-->>EventLoop: ConnectionEstablished
    EventLoop->>DialCmd: cmd.on_event(event)
    DialCmd->>DialCmd: handle.finish(Ok(()))
    DialCmd-->>EventLoop: return false (完成)
    EventLoop->>EventLoop: 从 active 移除
    EventLoop-->>Client: Ok(())
```

### 4. CommandFuture - 让命令可 await

```rust
impl<T: CommandHandler> Future for CommandFuture<T> {
    type Output = Result<T::Result>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // 首次 poll：发送命令到 EventLoop
        if let Some(handler) = this.handler.take() {
            let task = CommandTask::new(handler, this.handle.clone());
            this.sender.try_send(Box::new(task))?;
            return Poll::Pending;
        }

        // 后续 poll：检查结果
        this.handle.poll(cx)
    }
}
```

**状态转换：**

```mermaid
stateDiagram-v2
    [*] --> HasHandler: 创建 CommandFuture

    HasHandler --> Waiting: poll() 首次调用
    note right of HasHandler: handler = Some(cmd)
    note right of Waiting: handler = None

    Waiting --> Waiting: poll() 结果未就绪
    Waiting --> Completed: poll() 结果就绪

    Completed --> [*]: 返回 Ready(result)
```

### 5. EventLoop - 事件循环

```mermaid
flowchart TB
    subgraph EventLoop["EventLoop.run()"]
        Start["loop"]
        Select["tokio::select!"]

        subgraph CmdBranch["命令分支"]
            RecvCmd["command_rx.recv()"]
            RunCmd["cmd.run_boxed(swarm)"]
            PushActive["active_commands.push(cmd)"]
        end

        subgraph EventBranch["事件分支"]
            RecvEvent["swarm.select_next_some()"]
            NotifyAll["遍历 active_commands"]
            OnEvent["cmd.on_event_boxed(event)"]
            CheckKeep{"keep?"}
            Remove["swap_remove(i)"]
            Next["i += 1"]
        end

        Start --> Select
        Select --> RecvCmd
        Select --> RecvEvent

        RecvCmd --> RunCmd
        RunCmd --> PushActive
        PushActive --> Select

        RecvEvent --> NotifyAll
        NotifyAll --> OnEvent
        OnEvent --> CheckKeep
        CheckKeep -->|false| Remove
        CheckKeep -->|true| Next
        Remove --> NotifyAll
        Next --> NotifyAll
    end
```

## 数据流全景

```mermaid
flowchart LR
    subgraph UserCode["用户代码"]
        Call["client.dial(peer).await"]
    end

    subgraph CommandFuture["CommandFuture"]
        CF["poll()"]
        RH["ResultHandle"]
    end

    subgraph Channel["Channel"]
        TX["Sender"]
        RX["Receiver"]
    end

    subgraph EventLoop["EventLoop"]
        EL["事件循环"]
        AC["active_commands"]
    end

    subgraph Swarm["Swarm"]
        SW["libp2p Swarm"]
    end

    Call --> CF
    CF -->|"发送命令"| TX
    TX --> RX
    RX --> EL
    EL -->|"run()"| SW
    EL --> AC
    SW -->|"事件"| EL
    EL -->|"on_event()"| AC
    AC -->|"finish()"| RH
    RH -->|"wake()"| CF
    CF -->|"result"| Call
```

## 优点分析

### 1. 简洁的调用 API

```rust
let result = client.dial(peer_id).await?;
```

### 2. 命令可组合、可扩展

```mermaid
classDiagram
    class CommandHandler {
        <<trait>>
        +Result
        +run(swarm, handle)
        +on_event(event, handle) bool
    }

    class DialCommand {
        +peer_id: PeerId
        +run()
        +on_event() bool
    }

    class CloseCommand {
        +peer_id: PeerId
        +run()
        +on_event() bool
    }

    class SendFileCommand {
        +file_path: PathBuf
        +target: PeerId
        +run()
        +on_event() bool
    }

    CommandHandler <|.. DialCommand
    CommandHandler <|.. CloseCommand
    CommandHandler <|.. SendFileCommand
```

### 3. 类型安全

每个命令有自己的 `Result` 类型：

```rust
impl CommandHandler for DialCommand {
    type Result = ();
}

impl CommandHandler for GetPeersCommand {
    type Result = Vec<PeerInfo>;
}
```

### 4. 生命周期清晰

```mermaid
flowchart LR
    A["创建命令"] --> B["发送到 Channel"]
    B --> C["执行 run()"]
    C --> D["加入 active 列表"]
    D --> E["等待事件"]
    E --> F{"on_event 返回"}
    F -->|true| E
    F -->|false| G["从列表移除"]
    G --> H["命令完成"]
```

## 对比其他方案

```mermaid
graph TB
    subgraph Callback["方案1: 回调函数"]
        C1["dial(peer, |result| { ... })"]
        C2["容易形成回调地狱"]
    end

    subgraph Channel["方案2: Channel 双向"]
        Ch1["command_tx.send(Dial)"]
        Ch2["result_rx.recv()"]
        Ch3["无法等待特定事件"]
    end

    subgraph Command["方案3: 命令模式 ✓"]
        Cmd1["client.dial(peer).await"]
        Cmd2["可 await，类型安全"]
        Cmd3["实现稍复杂"]
    end
```

| 方案 | 优点 | 缺点 |
|------|------|------|
| **回调函数** | 简单 | 回调地狱，难以组合 |
| **Channel 双向** | 解耦 | 无法等待特定事件 |
| **本文方案** | 可 await，类型安全 | 实现稍复杂 |

## 总结

```mermaid
mindmap
  root((命令模式架构))
    封装
      发起操作
      等待结果
      统一为 Future
    桥接
      ResultHandle
      Waker 机制
      跨任务通信
    解耦
      命令不知传输机制
      EventLoop 不知命令逻辑
      职责单一
```

这种命令模式架构的核心思想是：

1. **封装**：将"发起操作"和"等待结果"封装成一个 Future
2. **桥接**：使用 `ResultHandle` 在事件循环和调用方之间传递结果
3. **解耦**：命令不知道传输机制，事件循环不知道具体命令逻辑

这使得我们可以在事件驱动的 libp2p 之上，构建出优雅的请求-响应式 API。

---

*本文档属于 swarm-p2p 项目，用于解释 `command/handler.rs` 的设计理念。*
