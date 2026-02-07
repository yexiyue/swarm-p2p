# BUG-002: mDNS 发现后未注册地址直接 dial 导致连接失败

## 现象

mDNS 发现对端节点后，偶发 dial 失败或连接建立缓慢。多网卡环境下更明显。

## 根因分析

原代码在收到 `mdns::Event::Discovered` 后直接按 PeerId dial：

```rust
// 修复前
SwarmEvent::Behaviour(CoreBehaviourEvent::Mdns(
    libp2p::mdns::Event::Discovered(peers),
)) => {
    for (peer_id, _addr) in &peers {
        if !self.swarm.is_connected(peer_id) {
            if let Err(e) = self.swarm.dial(*peer_id) {
                warn!("Failed to dial discovered peer {}: {}", peer_id, e);
            }
        }
    }
    Some(NodeEvent::PeersDiscovered { peers })
}
```

存在两个问题：

### 问题 1：地址未注册

`swarm.dial(PeerId)` 依赖 swarm 已知的地址列表。mDNS 发现的地址在事件的 `peers` 参数中，但代码用 `_addr` 忽略了地址，没有调用 `add_peer_address` 注册。如果 swarm 此前没有该 peer 的地址记录，dial 会因无可用地址而失败。

### 问题 2：重复 dial

mDNS 在多网卡环境下可能为同一个 PeerId 返回多条记录（每个网卡接口一条）。原代码对每条记录都执行一次 dial，导致对同一 peer 发起多次并发连接尝试。

## 修复

```rust
// 修复后
SwarmEvent::Behaviour(CoreBehaviourEvent::Mdns(
    libp2p::mdns::Event::Discovered(peers),
)) => {
    // 1. 先注册所有地址
    for (peer_id, addr) in &peers {
        self.swarm.add_peer_address(*peer_id, addr.clone());
    }

    // 2. 去重后再 dial
    let dialed: std::collections::HashSet<_> =
        peers.iter().map(|(id, _)| *id).collect();

    for peer_id in &dialed {
        if !self.swarm.is_connected(peer_id) {
            if let Err(e) = self.swarm.dial(*peer_id) {
                warn!("Failed to dial discovered peer {}: {}", peer_id, e);
            }
        }
    }
    Some(NodeEvent::PeersDiscovered { peers })
}
```

先注册地址再 dial，确保 `dial(PeerId)` 能使用所有已知地址；用 `HashSet` 去重避免对同一 peer 的重复 dial。

## 涉及文件

- `core/src/runtime/event_loop.rs`
