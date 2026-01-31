use libp2p::{Multiaddr, PeerId};

/// 对外暴露的节点事件
#[derive(Debug, Clone)]
pub enum NodeEvent {
    /// 开始监听某个地址
    Listening(Multiaddr),

    /// 发现 peers（mDNS）
    PeersDiscovered(Vec<(PeerId, Multiaddr)>),

    /// peer 已连接
    PeerConnected(PeerId),

    /// peer 已断开
    PeerDisconnected(PeerId),

    /// 收到 identify 信息
    IdentifyReceived {
        peer_id: PeerId,
        agent_version: String,
        protocol_version: String,
    },
}
