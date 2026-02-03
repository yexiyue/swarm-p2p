use libp2p::{Multiaddr, PeerId};
use serde::Serialize;

/// NAT 状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NatStatus {
    /// 公网可达
    Public,
    /// NAT 后面（私网）
    Private,
    /// 未知
    Unknown,
}

/// 对外暴露的节点事件
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NodeEvent {
    /// 开始监听某个地址
    Listening {
        addr: Multiaddr,
    },

    /// 发现 peers（mDNS）
    PeersDiscovered {
        peers: Vec<(PeerId, Multiaddr)>,
    },

    /// peer 已连接
    #[serde(rename_all = "camelCase")]
    PeerConnected {
        peer_id: PeerId,
    },

    /// peer 已断开
    #[serde(rename_all = "camelCase")]
    PeerDisconnected {
        peer_id: PeerId,
    },

    /// 收到 identify 信息
    #[serde(rename_all = "camelCase")]
    IdentifyReceived {
        peer_id: PeerId,
        agent_version: String,
        protocol_version: String,
    },

    /// Ping 成功，返回延迟
    #[serde(rename_all = "camelCase")]
    PingSuccess {
        peer_id: PeerId,
        /// 往返延迟（毫秒）
        rtt_ms: u64,
    },

    /// NAT 状态变化
    #[serde(rename_all = "camelCase")]
    NatStatusChanged {
        /// 新的 NAT 状态
        status: NatStatus,
        /// 如果是公网，返回外部地址
        public_addr: Option<Multiaddr>,
    },
}
