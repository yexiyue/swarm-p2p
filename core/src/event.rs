use libp2p::{Multiaddr, PeerId};
use serde::Serialize;

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
}
