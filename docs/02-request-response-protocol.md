# Request-Response 协议：双向请求的完整实现

在 [上一篇文章](./01-command-pattern-architecture.md) 中，我们介绍了命令模式如何将 libp2p 的事件驱动模型封装为可 `await` 的 API。本文将深入讲解 **Request-Response 协议**的完整实现——这是 P2P 文件传输的核心通信机制。

## 概览

Request-Response 协议涉及**两个方向**的数据流：

```mermaid
flowchart LR
    subgraph NodeA["节点 A"]
        A_Client["NetClient"]
        A_Loop["EventLoop"]
    end

    subgraph NodeB["节点 B"]
        B_Loop["EventLoop"]
        B_Client["NetClient"]
        B_Frontend["前端 / 上层应用"]
    end

    A_Client -->|"send_request(peer_b, req)"| A_Loop
    A_Loop -->|"libp2p 网络"| B_Loop
    B_Loop -->|"NodeEvent::InboundRequest"| B_Frontend
    B_Frontend -->|"send_response(pending_id, resp)"| B_Client
    B_Client --> B_Loop
    B_Loop -->|"libp2p 网络"| A_Loop
    A_Loop -->|"Ok(resp)"| A_Client
```

- **出站请求（Outbound）**：`NetClient::send_request()` → 发送请求 → 等待远端响应
- **入站请求（Inbound）**：收到远端请求 → 通知上层 → 上层调用 `NetClient::send_response()` 回复

关键挑战在于：**入站请求**需要将 libp2p 的 `ResponseChannel`（一次性、不可 Clone）安全地暂存，等待上层处理后取回使用。

## libp2p Request-Response 基础

### CBOR 编码

我们使用 libp2p 内置的 CBOR 编解码器，请求和响应类型需要满足 `CborMessage` trait：

```rust
pub trait CborMessage:
    Debug + Clone + Serialize + Send + Sync + for<'a> Deserialize<'a> + 'static
{
}
```

### Behaviour 配置

```rust
let req_resp = request_response::cbor::Behaviour::new(
    [(
        StreamProtocol::new("/swarmdrop/1.0.0"),
        request_response::ProtocolSupport::Full,  // 双向支持
    )],
    request_response::Config::default(),
);
```

`ProtocolSupport::Full` 表示每个节点既可以发送请求也可以接收请求。

### libp2p 产生的事件

libp2p 的 request-response 模块会产生以下几种关键事件：

```mermaid
flowchart TB
    subgraph Events["ReqResp 事件类型"]
        direction TB
        E1["Message::Request<br/>收到对端的请求<br/>携带 ResponseChannel"]
        E2["Message::Response<br/>收到对端的响应<br/>携带 request_id"]
        E3["OutboundFailure<br/>发送请求失败"]
        E4["InboundFailure<br/>回复响应失败"]
    end

    style E1 fill:#e1f5fe
    style E2 fill:#e8f5e9
    style E3 fill:#ffebee
    style E4 fill:#ffebee
```

其中 `Message::Request` 是最特殊的——它携带了一个 **`ResponseChannel`**，这是一个一次性的回复通道，**必须被消费才能回复请求**。

## 出站请求：SendRequestCommand

出站方向的实现相对直观，遵循标准的命令模式。

### 流程

```mermaid
sequenceDiagram
    participant Client as NetClient
    participant CF as CommandFuture
    participant EL as EventLoop
    participant Cmd as SendRequestCommand
    participant Swarm as libp2p Swarm
    participant Remote as 远端节点

    Client->>CF: send_request(peer_id, request)
    CF->>EL: 通过 channel 发送命令
    EL->>Cmd: cmd.run(swarm)
    Cmd->>Swarm: req_resp.send_request(&peer_id, request)
    Swarm-->>Cmd: 返回 OutboundRequestId
    Cmd->>Cmd: 保存 request_id
    EL->>EL: push to active_commands

    Note over EL: 等待 swarm 事件...

    Swarm->>Remote: 网络传输请求
    Remote-->>Swarm: 响应数据
    Swarm-->>EL: Message::Response { request_id, response }

    EL->>Cmd: cmd.on_event(event)
    Cmd->>Cmd: 校验 request_id 匹配
    Cmd->>CF: handle.finish(Ok(response))
    CF-->>Client: Ok(response)
```

### 代码实现

```rust
// client/req_resp.rs
pub async fn send_request(&self, peer_id: PeerId, request: Req) -> Result<Resp>
where
    Req: Unpin,
{
    let cmd = SendRequestCommand::new(peer_id, request);
    CommandFuture::new(cmd, self.command_tx.clone()).await
}
```

`SendRequestCommand` 的 `on_event` 接收 owned event，匹配两种事件并**消费**它们（返回 `None`）：

```rust
async fn on_event(
    &mut self,
    event: SwarmEvent<CoreBehaviourEvent<Req, Resp>>,
    handle: &ResultHandle<Resp>,
) -> OnEventResult<Req, Resp> {
    match &event {
        // 成功收到响应
        SwarmEvent::Behaviour(CoreBehaviourEvent::ReqResp(Event::Message {
            peer,
            message: Message::Response { request_id, response },
            ..
        })) if self.request_id.as_ref() == Some(request_id)
            && *peer == self.peer_id =>
        {
            handle.finish(Ok(response.clone()));
            (false, None)  // 消费事件，命令完成
        }
        // 发送失败
        SwarmEvent::Behaviour(CoreBehaviourEvent::ReqResp(Event::OutboundFailure {
            peer, request_id, error, ..
        })) if self.request_id.as_ref() == Some(request_id)
            && *peer == self.peer_id =>
        {
            handle.finish(Err(Error::Behaviour(...)));
            (false, None)  // 消费事件，命令完成
        }
        _ => (true, Some(event))  // 继续等待，不消费
    }
}
```

注意 `request_id` 的校验——同一时刻可能有多个 `SendRequestCommand` 在等待响应，每个只关心自己的 `request_id`。匹配到的事件会被消费（`None`），不会传递给其他命令或前端。

## 入站请求：核心设计挑战

入站方向的复杂度远高于出站，因为它涉及**跨组件的所有权转移**。

### 问题分析

当 libp2p 收到远端请求时，产生的事件包含：

```rust
SwarmEvent::Behaviour(CoreBehaviourEvent::ReqResp(Event::Message {
    peer,
    message: Message::Request {
        request,           // 请求内容（可 Clone）
        channel,           // ResponseChannel（不可 Clone，不可 Sync）
        ..
    },
}))
```

`ResponseChannel` 的特性决定了整个设计：

```mermaid
flowchart TB
    subgraph RC["ResponseChannel 的约束"]
        direction TB
        P1["Send ✓ — 可跨线程移动"]
        P2["Clone ✗ — 一次性消费"]
        P3["Sync ✗ — 不可共享引用"]
        P4["内部是 oneshot::Sender"]
    end

    subgraph Impact["设计影响"]
        direction TB
        I1["不能用 moka 缓存<br/>（需要 Clone）"]
        I2["不能用 DashMap<br/>（需要 Sync）"]
        I3["必须从 owned event 中 move 出来"]
        I4["必须精确取回并消费"]
    end

    RC --> Impact

    style P2 fill:#ffebee
    style P3 fill:#ffebee
    style I1 fill:#fff3e0
    style I2 fill:#fff3e0
```

### 设计方案

我们需要解决的核心问题是：**EventLoop 收到 `ResponseChannel` 后，如何让 `NetClient` 在稍后取回并使用它？**

```mermaid
flowchart LR
    subgraph EL["EventLoop"]
        Recv["收到 InboundRequest"]
        Store["存入 PendingMap"]
        Notify["发送 NodeEvent"]
    end

    subgraph PM["PendingMap"]
        Map["Mutex&lt;HashMap&lt;u64, ResponseChannel&gt;&gt;"]
    end

    subgraph App["上层应用"]
        Handle["处理请求"]
        Reply["send_response(pending_id, resp)"]
    end

    subgraph NC["NetClient"]
        Take["pending_map.take(pending_id)"]
        Send["SendResponseCommand"]
    end

    Recv --> Store
    Store --> Map
    Recv --> Notify
    Notify --> Handle
    Handle --> Reply
    Reply --> Take
    Take --> Map
    Map --> Send
```

关键设计决策：

1. **`PendingMap`** — 使用 `Mutex<HashMap>` 而非 `DashMap`（因为 `ResponseChannel` 不是 `Sync`）
2. **`pending_id`** — 单调递增的 u64 标识，连接 EventLoop（存入）和 NetClient（取出）
3. **共享所有权** — `PendingMap` 通过 `Arc` 在 EventLoop 和 NetClient 之间共享

## PendingMap：带 TTL 的暂存容器

### 为什么不用现有方案？

| 方案 | 问题 |
|------|------|
| `moka` 缓存 | 要求 value 实现 `Clone`，`ResponseChannel` 不满足 |
| `DashMap` | 要求 value 实现 `Sync`，`ResponseChannel` 不满足 |
| 普通 `HashMap` | 没有自动过期，不可跨线程共享 |

### 实现

```rust
pub struct PendingMap<K, V> {
    inner: Arc<Mutex<HashMap<K, PendingEntry<V>>>>,
}

struct PendingEntry<V> {
    value: V,
    created_at: Instant,
}
```

内部启动一个 tokio 定时任务，周期性清理过期条目：

```mermaid
flowchart TB
    subgraph PendingMap
        direction TB
        Insert["insert(pending_id, channel)"]
        Take["take(pending_id) → Option&lt;V&gt;"]
        Map["HashMap&lt;K, PendingEntry&lt;V&gt;&gt;"]
    end

    subgraph Cleanup["清理任务（每 10 秒）"]
        Tick["interval.tick()"]
        Check["遍历所有条目"]
        Expired{"created_at + TTL < now?"}
        Remove["retain 移除过期"]
    end

    Insert --> Map
    Take --> Map
    Tick --> Check
    Check --> Expired
    Expired -->|"是"| Remove
    Remove --> Map
    Expired -->|"否"| Check
```

TTL 机制确保即使上层忘记回复，`ResponseChannel` 也不会永远占用内存。默认 TTL 为 60 秒。

```rust
impl<K, V> PendingMap<K, V>
where
    K: Eq + Hash + Send + 'static,
    V: Send + 'static,
{
    pub fn new(ttl: Duration) -> Self {
        let map = Arc::new(Mutex::new(HashMap::new()));
        let map_clone = Arc::clone(&map);

        // 启动清理任务
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                let now = Instant::now();
                map_clone
                    .lock()
                    .retain(|_, v| now.duration_since(v.created_at) < ttl);
            }
        });

        Self { inner: map }
    }
}
```

> **设计取舍**：使用 `Mutex` 而非 `RwLock`，因为 `insert` 和 `take` 都是写操作，读写锁没有优势。`parking_lot::Mutex` 在无竞争时开销极低。

## 入站请求的完整流程

### 第一阶段：EventLoop 接收并暂存

事件流经**责任链**——先通过 `active_commands`，未被消费的事件再进入 `convert_to_node_event`：

```mermaid
sequenceDiagram
    participant Remote as 远端节点
    participant Swarm as libp2p Swarm
    participant EL as EventLoop
    participant Cmds as active_commands
    participant PM as PendingMap
    participant TX as event_tx

    Remote->>Swarm: 发送请求
    Swarm->>EL: SwarmEvent::Behaviour(ReqResp(Message::Request))

    Note over EL,Cmds: 责任链：依次传递 owned event
    EL->>Cmds: cmd[0].on_event_boxed(event)
    Cmds-->>EL: (keep, Some(event)) — 不消费，传递
    EL->>Cmds: cmd[1].on_event_boxed(event)
    Cmds-->>EL: (keep, Some(event)) — 不消费，传递

    Note over EL: 剩余事件进入 convert_to_node_event（owned）
    EL->>EL: match event { Message::Request { request, channel, .. } }
    EL->>EL: pending_id = next_pending_id()
    EL->>PM: insert(pending_id, channel)
    EL->>TX: send(NodeEvent::InboundRequest { peer_id, pending_id, request })
```

关键实现：

```rust
async fn handle_swarm_event(&mut self, event: SwarmEvent<CoreBehaviourEvent<Req, Resp>>) {
    // 责任链：依次传递 owned event，命令可选择消费或传递
    let mut remaining = Some(event);
    let mut i = 0;
    while i < self.active_commands.len() {
        let Some(event) = remaining.take() else {
            break; // 事件已被消费，停止传递
        };
        let (keep, returned) = self.active_commands[i].on_event_boxed(event).await;
        remaining = returned;
        if keep { i += 1; } else { self.active_commands.swap_remove(i); }
    }

    // 未被命令消费的事件，转换为前端事件
    let Some(event) = remaining else { return; };
    if let Some(evt) = self.convert_to_node_event(event) {
        let _ = self.event_tx.send(evt).await;
    }
}
```

`convert_to_node_event` 同样接收 owned event，直接在内部处理 InboundRequest：

```rust
fn convert_to_node_event(&mut self, event: SwarmEvent<...>) -> Option<NodeEvent<Req>> {
    match event {
        // ... 其他事件（ConnectionEstablished, Ping, Identify 等）...
        SwarmEvent::Behaviour(CoreBehaviourEvent::ReqResp(ReqRespEvent::Message {
            peer,
            message: Message::Request { request, channel, .. },
            ..
        })) => {
            let pending_id = self.next_pending_id();
            self.pending_channels.insert(pending_id, channel);  // 直接 move
            Some(NodeEvent::InboundRequest { peer_id: peer, pending_id, request })
        }
        // ...
    }
}
```

**事件传递流程：**

```mermaid
flowchart TB
    E["SwarmEvent (owned)"]
    C1["cmd[0].on_event(event)"]
    C2["cmd[1].on_event(event)"]
    CN["..."]
    Conv["convert_to_node_event(event)"]
    FE["发送 NodeEvent 给前端"]

    E --> C1
    C1 -->|"Some(event)"| C2
    C1 -->|"None"| Stop1["事件被消费，停止"]
    C2 -->|"Some(event)"| CN
    C2 -->|"None"| Stop2["事件被消费，停止"]
    CN -->|"Some(event)"| Conv
    Conv --> FE

    style Stop1 fill:#ffebee
    style Stop2 fill:#ffebee
    style FE fill:#e8f5e9
```

整个流程全程使用 owned event，无需借用与所有权分离的两步操作。`ResponseChannel`（不可 Clone）在 `convert_to_node_event` 中直接从 owned event move 出来，存入 `PendingMap`。

### 第二阶段：上层处理并回复

```mermaid
sequenceDiagram
    participant RX as EventReceiver
    participant App as 上层应用
    participant NC as NetClient
    participant PM as PendingMap
    participant CF as CommandFuture
    participant EL as EventLoop
    participant Cmd as SendResponseCommand
    participant Swarm

    RX->>App: NodeEvent::InboundRequest { peer_id, pending_id, request }
    App->>App: 处理请求，构造响应

    App->>NC: send_response(pending_id, response)
    NC->>PM: take(pending_id)
    PM-->>NC: Some(ResponseChannel)
    NC->>CF: CommandFuture::new(SendResponseCommand)
    CF->>EL: 通过 channel 发送命令
    EL->>Cmd: cmd.run(swarm)
    Cmd->>Swarm: req_resp.send_response(channel, response)
    Swarm-->>Cmd: Ok(()) / Err(...)
    Cmd->>CF: handle.finish(result)
    CF-->>NC: result
    NC-->>App: Ok(())
```

`NetClient::send_response` 的实现：

```rust
pub async fn send_response(&self, pending_id: u64, response: Resp) -> Result<()>
where
    Resp: Unpin,
{
    // 从 PendingMap 取出 ResponseChannel
    let channel = self.pending_channels.take(&pending_id)
        .ok_or_else(|| Error::Behaviour(
            format!("No pending channel for pending_id={} (expired or already responded)", pending_id),
        ))?;
    // 复用现有的 SendResponseCommand
    let cmd = SendResponseCommand::new(channel, response);
    CommandFuture::new(cmd, self.command_tx.clone()).await
}
```

注意 `take` 的语义——取出后 PendingMap 中不再有该条目，保证**一个请求只能被回复一次**。

## 完整生命周期

以下是一个完整的请求-响应交互的全景图：

```mermaid
sequenceDiagram
    box rgb(232, 245, 233) 节点 A（请求方）
        participant A_Client as NetClient A
        participant A_Loop as EventLoop A
    end

    box rgb(227, 242, 253) 网络
        participant Net as libp2p Network
    end

    box rgb(255, 243, 224) 节点 B（响应方）
        participant B_Loop as EventLoop B
        participant B_PM as PendingMap
        participant B_App as 上层应用
        participant B_Client as NetClient B
    end

    Note over A_Client,B_Client: 完整的请求-响应生命周期

    A_Client->>A_Loop: send_request(peer_b, "hello")
    A_Loop->>Net: Request { data: "hello" }
    Net->>B_Loop: SwarmEvent::Message::Request { request, channel }

    B_Loop->>B_Loop: pending_id = 0
    B_Loop->>B_PM: insert(0, channel)
    B_Loop->>B_App: NodeEvent::InboundRequest { pending_id: 0, request: "hello" }

    B_App->>B_App: 处理请求...
    B_App->>B_Client: send_response(0, "world")
    B_Client->>B_PM: take(0)
    B_PM-->>B_Client: Some(channel)
    B_Client->>B_Loop: SendResponseCommand(channel, "world")
    B_Loop->>Net: Response { data: "world" }

    Net->>A_Loop: SwarmEvent::Message::Response { response }
    A_Loop->>A_Client: Ok("world")
```

## NodeEvent 的泛型设计

为了让 `InboundRequest` 携带请求内容，`NodeEvent` 被设计为泛型：

```rust
pub enum NodeEvent<Req = ()> {
    Listening { addr: Multiaddr },
    PeerConnected { peer_id: PeerId },
    PeerDisconnected { peer_id: PeerId },
    // ... 其他变体 ...

    /// 收到对端的 request-response 请求
    InboundRequest {
        peer_id: PeerId,
        pending_id: u64,   // 回复时的唯一标识
        request: Req,       // 请求内容
    },
}
```

默认类型参数 `Req = ()` 保证了不使用 request-response 时的向后兼容：

```rust
// 不关心请求内容时
let event: NodeEvent = receiver.recv().await;

// 使用自定义请求类型时
let event: NodeEvent<MyRequest> = receiver.recv().await;
```

## 模块组织

整个 request-response 相关代码分布在以下模块中：

```mermaid
flowchart TB
    subgraph client["client/ — 公共 API"]
        mod_rs["mod.rs<br/>NetClient 结构体"]
        req_resp_rs["req_resp.rs<br/>send_request / send_response"]
        future_rs["future.rs<br/>CommandFuture"]
    end

    subgraph command["command/ — 命令实现"]
        send_req["req_resp/send_request.rs<br/>SendRequestCommand"]
        send_resp["req_resp/send_response.rs<br/>SendResponseCommand"]
        handler["handler.rs<br/>CommandHandler trait"]
    end

    subgraph runtime["runtime/ — 运行时"]
        event_loop["event_loop.rs<br/>EventLoop 事件分发"]
        behaviour["behaviour.rs<br/>CoreBehaviour 协议配置"]
    end

    subgraph shared["共享组件"]
        pending["pending_map.rs<br/>PendingMap TTL 暂存"]
        event["event.rs<br/>NodeEvent 泛型事件"]
    end

    req_resp_rs --> future_rs
    future_rs --> handler
    req_resp_rs --> send_req
    req_resp_rs --> send_resp
    event_loop --> pending
    req_resp_rs --> pending
    event_loop --> event

    style client fill:#e8f5e9
    style command fill:#e1f5fe
    style runtime fill:#fff3e0
    style shared fill:#f3e5f5
```

依赖方向始终保持单向：`client → command`，`client → runtime`（仅类型），`command` 和 `runtime` 互不依赖 `client`。

## 错误处理

### 出站请求失败

```mermaid
flowchart TB
    A["send_request(peer, req)"] --> B{"channel 是否畅通?"}
    B -->|"否"| C["Error::Behaviour<br/>command channel closed"]
    B -->|"是"| D{"请求是否送达?"}
    D -->|"否"| E["OutboundFailure<br/>连接断开 / 超时"]
    D -->|"是"| F{"是否收到响应?"}
    F -->|"是"| G["Ok(response)"]
    F -->|"超时"| H["OutboundFailure<br/>ResponseOmission"]
```

### 入站回复失败

```mermaid
flowchart TB
    A["send_response(pending_id, resp)"] --> B{"PendingMap 中有该条目?"}
    B -->|"否"| C["Error::Behaviour<br/>expired or already responded"]
    B -->|"是"| D{"ResponseChannel 有效?"}
    D -->|"否"| E["Error::Behaviour<br/>channel closed"]
    D -->|"是"| F["Ok(())"]
```

常见的入站回复失败场景：

| 场景 | 原因 | 错误信息 |
|------|------|----------|
| 超过 TTL | PendingMap 自动清理了 | `No pending channel for pending_id=N (expired or already responded)` |
| 重复回复 | `take` 已消费过 | 同上 |
| 对端断开 | ResponseChannel 内部 sender 已关闭 | `Failed to send response: channel closed` |

## 总结

```mermaid
mindmap
  root((Request-Response 协议))
    出站请求
      SendRequestCommand
      匹配 request_id
      等待 Response 或 Failure
    入站请求
      ResponseChannel 暂存
      PendingMap TTL 管理
      pending_id 映射
    核心挑战
      ResponseChannel 不可 Clone
      ResponseChannel 不可 Sync
      借用与所有权分离
    模块职责
      client/ — 公共 API
      command/ — 命令实现
      runtime/ — 事件分发
      PendingMap — 跨组件桥接
```

整个设计的核心思想：

1. **所有权转移链**：`SwarmEvent` → `EventLoop.insert()` → `PendingMap` → `NetClient.take()` → `SendResponseCommand`
2. **一次性语义**：`ResponseChannel` 和 `pending_id` 都是一次性消费，确保每个请求只被回复一次
3. **TTL 兜底**：即使上层忘记回复，PendingMap 的自动清理也能防止内存泄漏
4. **复用命令模式**：入站回复不需要新的通信机制，直接复用已有的 `SendResponseCommand` + `CommandFuture`

---

*本文档属于 swarm-p2p 项目，用于解释 request-response 协议的双向通信实现。*
