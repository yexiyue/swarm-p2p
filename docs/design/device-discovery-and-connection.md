# 设备发现与连接设计

本文档描述 swarm-p2p 的设备发现和连接机制，包括局域网发现、跨网络连接、以及收藏设备自动重连。

## 概述

swarm-p2p 支持两种设备发现方式：

| 场景 | 机制 | 需要分享码 |
|-----|------|-----------|
| 局域网 | mDNS | 否 |
| 跨网络 | Kad DHT + 分享码 | 是 |

## 1. 局域网发现（mDNS）

### 工作流程

```mermaid
sequenceDiagram
    participant A as 设备 A
    participant Net as 局域网
    participant B as 设备 B

    A->>Net: mDNS 广播（我是 A，地址是...）
    B->>Net: mDNS 广播（我是 B，地址是...）
    Net-->>A: 发现设备 B
    Net-->>B: 发现设备 A

    Note over A,B: 触发 PeersDiscovered 事件

    A->>B: 自动建立连接
    B-->>A: 连接确认

    Note over A,B: 触发 PeerConnected 事件

    A->>B: Identify 协议
    B-->>A: 返回设备信息（agent_version）

    Note over A,B: 触发 IdentifyReceived 事件
```

### 事件流

```mermaid
flowchart LR
    A[mDNS 发现] --> B[PeersDiscovered]
    B --> C[自动 Dial]
    C --> D[PeerConnected]
    D --> E[Identify 交换]
    E --> F[IdentifyReceived]
    F --> G{协议版本匹配?}
    G -->|是| H[加入 Kad 路由表]
    G -->|否| I[忽略]
```

### 前端显示

局域网发现的设备显示在"附近设备"列表，可直接点击发送文件，无需分享码。

## 2. 跨网络连接（分享码 + Kad）

### 为什么不用 Kad 发现设备列表？

- DHT 网络可能有成千上万节点
- 大多数节点不是目标设备
- 无法预知连接质量

**因此：Kad 不用于"发现设备列表"，而是用于"通过分享码找到特定设备"。**

### 分享码机制

```mermaid
flowchart TB
    subgraph 发送方
        A1[选择文件] --> A2[生成 session_id]
        A2 --> A3["分享码 = encode(session_id)"]
        A3 --> A4["DHT: PUT_PROVIDER(hash(session_id), peer_id)"]
        A4 --> A5[显示分享码: AB12CD]
        A5 --> A6[等待连接...]
    end

    subgraph 接收方
        B1[输入分享码: AB12CD] --> B2["session_id = decode(分享码)"]
        B2 --> B3["DHT: GET_PROVIDERS(hash(session_id))"]
        B3 --> B4[获取发送方 PeerId]
        B4 --> B5[连接发送方]
    end

    A6 -.->|P2P 连接| B5
```

### 详细时序

```mermaid
sequenceDiagram
    participant Sender as 发送方
    participant DHT as Kad DHT
    participant Receiver as 接收方

    Note over Sender: 用户选择文件
    Sender->>Sender: 生成 session_id
    Sender->>Sender: 分享码 = Base32(session_id)

    Sender->>DHT: PUT_PROVIDER(hash(session_id), my_peer_id)
    DHT-->>Sender: Provider 发布成功

    Note over Sender: 显示分享码 AB12CD
    Note over Sender,Receiver: 用户通过微信/口头告知分享码

    Note over Receiver: 用户输入分享码
    Receiver->>Receiver: session_id = decode(AB12CD)
    Receiver->>DHT: GET_PROVIDERS(hash(session_id))
    DHT-->>Receiver: 返回 [sender_peer_id]

    Receiver->>Sender: Dial(sender_peer_id)

    alt NAT 穿透成功
        Sender-->>Receiver: 直连建立
    else NAT 穿透失败
        Sender-->>Receiver: 通过 Relay 中继连接
    end

    Note over Sender,Receiver: 开始文件传输
```

### Kad API 使用

**发送方 - 发布 Provider：**

```rust
// 生成分享码
let session_id: [u8; 8] = random();
let share_code = base32_encode(&session_id[..4]);

// 在 DHT 发布
let key = kad::RecordKey::new(&sha256(&session_id));
swarm.behaviour_mut().kad.start_providing(key)?;
```

**接收方 - 查询 Provider：**

```rust
// 解析分享码
let session_id = base32_decode(&share_code);

// 查询 DHT
let key = kad::RecordKey::new(&sha256(&session_id));
swarm.behaviour_mut().kad.get_providers(key);

// 处理查询结果（在事件循环中）
kad::Event::OutboundQueryProgressed {
    result: QueryResult::GetProviders(Ok(GetProvidersOk::FoundProviders { providers, .. })),
    ..
} => {
    for peer_id in providers {
        swarm.dial(peer_id)?;
    }
}
```

## 3. 收藏设备自动重连

### 问题

配对成功后，下次如何自动连接该设备，而不需要每次都输入分享码？

### 解决方案

```mermaid
flowchart TB
    subgraph 设备启动时
        A1[启动节点] --> A2["DHT: 注册自己<br/>PUT_PROVIDER(hash(peer_id), peer_id)"]
        A2 --> A3[读取收藏设备列表]
        A3 --> A4{遍历收藏设备}
    end

    subgraph 尝试连接每个收藏设备
        A4 --> B1{mDNS 发现?}
        B1 -->|是| B2[直接连接]
        B1 -->|否| B3["DHT: GET_PROVIDERS(hash(peer_id))"]
        B3 --> B4{找到地址?}
        B4 -->|是| B5[连接]
        B4 -->|否| B6[稍后重试]
    end
```

### 收藏设备数据模型

```rust
struct FavoriteDevice {
    peer_id: PeerId,          // 设备的 PeerId
    name: String,             // 设备名称
    fingerprint: String,      // 设备指纹（用于验证）
    added_at: i64,            // 添加时间
    last_connected: Option<i64>, // 上次连接时间
}
```

### 自动重连时序

```mermaid
sequenceDiagram
    participant A as 设备 A
    participant DHT as Kad DHT
    participant B as 收藏设备 B

    Note over A: 启动时
    A->>DHT: 注册自己 PUT_PROVIDER(hash(A), A)
    A->>A: 读取收藏列表: [B, C, D]

    loop 对于每个收藏设备
        A->>A: 检查 mDNS 是否发现 B
        alt mDNS 发现
            A->>B: 直接连接
        else mDNS 未发现
            A->>DHT: GET_PROVIDERS(hash(B))
            DHT-->>A: 返回 B 的地址
            A->>B: 连接 B
        end
    end

    B-->>A: 连接成功
    Note over A,B: 触发 PeerConnected 事件
```

### 需要的 Command

| Command | 参数 | 用途 |
|---------|-----|------|
| `RegisterSelf` | - | 启动时在 DHT 注册自己 |
| `FindPeer` | `peer_id: PeerId` | 通过 PeerId 查找设备地址 |
| `StartProviding` | `key: Vec<u8>` | 发布分享码 Provider |
| `GetProviders` | `key: Vec<u8>` | 查询分享码 Provider |

### 需要的 Event

| Event | 字段 | 用途 |
|-------|-----|------|
| `ProviderPublished` | `key` | 分享码发布成功 |
| `ProvidersFound` | `key, providers` | 找到 Provider |
| `PeerFound` | `peer_id, addrs` | 找到收藏设备 |

## 4. 协议版本隔离

### 为什么需要？

swarm-p2p 可能被多个应用使用（如 SwarmDrop、SwarmNote），它们应该有独立的 DHT 网络。

### 实现方式

```mermaid
flowchart LR
    subgraph Identify 交换
        A[收到 IdentifyReceived] --> B{protocol_version<br/>== 本机 protocol_version?}
        B -->|是| C[加入 Kad 路由表]
        B -->|否| D[不加入 Kad]
    end
```

**配置示例：**

```rust
// SwarmDrop
let config = NodeConfig::new(
    "/swarmdrop/1.0.0",  // protocol_version
    "SwarmDrop/1.0.0 (Windows; Desktop; \"我的电脑\")",  // agent_version
);

// SwarmNote
let config = NodeConfig::new(
    "/swarmnote/1.0.0",  // protocol_version
    "SwarmNote/1.0.0 (macOS; Laptop; \"工作电脑\")",
);
```

这样 SwarmDrop 和 SwarmNote 的 DHT 网络自动隔离。

## 5. 完整架构图

```mermaid
flowchart TB
    subgraph 前端
        UI[React UI]
    end

    subgraph Tauri
        CMD[Tauri Commands]
    end

    subgraph swarm-p2p
        NC[NetClient]
        ER[EventReceiver]
        EL[EventLoop]

        subgraph Behaviours
            MDNS[mDNS]
            KAD[Kademlia]
            PING[Ping]
            ID[Identify]
            NAT[AutoNAT]
            DCUTR[DCUtR]
            RELAY[Relay]
        end
    end

    UI -->|invoke| CMD
    CMD -->|Command| NC
    NC -->|mpsc| EL
    EL -->|NodeEvent| ER
    ER -->|event| CMD
    CMD -->|emit| UI

    EL <--> MDNS
    EL <--> KAD
    EL <--> PING
    EL <--> ID
    EL <--> NAT
    EL <--> DCUTR
    EL <--> RELAY
```

## 6. 事件汇总

| 事件 | 触发条件 | 用途 |
|-----|---------|------|
| `Listening` | 开始监听地址 | 显示本机地址 |
| `PeersDiscovered` | mDNS 发现设备 | 显示附近设备 |
| `PeerConnected` | 与 peer 第一个连接建立 | 更新连接状态 |
| `PeerDisconnected` | 与 peer 最后一个连接关闭 | 更新连接状态 |
| `IdentifyReceived` | 收到对方身份信息 | 显示设备名称/类型 |
| `PingSuccess` | ping 成功 | 显示延迟 |
| `NatStatusChanged` | NAT 状态变化 | 显示网络状态 |

## 7. 待实现功能

- [ ] `RegisterSelf` Command - 启动时注册自己
- [ ] `FindPeer` Command - 查找收藏设备
- [ ] `StartProviding` Command - 发布分享码
- [ ] `GetProviders` Command - 查询分享码
- [ ] `ProviderPublished` Event - 发布成功通知
- [ ] `ProvidersFound` Event - 查询结果通知
- [ ] `PeerFound` Event - 设备查找结果
