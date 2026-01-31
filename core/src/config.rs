use std::time::Duration;

use libp2p::{Multiaddr, PeerId};

/// 节点配置
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// identify 协议版本（如 "/myapp/1.0.0"）
    pub protocol_version: String,

    /// identify agent 版本（如 "myapp/1.0.0;os=macos"）
    pub agent_version: String,

    /// 监听地址
    pub listen_addrs: Vec<Multiaddr>,

    /// Kademlia DHT 引导节点
    pub bootstrap_peers: Vec<(PeerId, Multiaddr)>,

    /// 启用 mDNS 局域网发现
    pub enable_mdns: bool,

    /// 启用 relay 中继客户端（NAT 穿透）
    pub enable_relay_client: bool,

    /// 启用 DCUtR 打洞
    pub enable_dcutr: bool,

    /// 启用 AutoNAT 检测
    pub enable_autonat: bool,

    /// 空闲连接超时时间
    pub idle_connection_timeout: Duration,

    /// Ping 间隔
    pub ping_interval: Duration,

    /// Ping 超时
    pub ping_timeout: Duration,

    /// Kademlia 查询超时
    pub kad_query_timeout: Duration,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            protocol_version: "/swarm-p2p/1.0.0".into(),
            agent_version: format!("swarm-p2p/{}", env!("CARGO_PKG_VERSION")),
            listen_addrs: vec![
                "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
                "/ip6/::/tcp/0".parse().unwrap(),
            ],
            bootstrap_peers: vec![],
            enable_mdns: true,
            enable_relay_client: true,
            enable_dcutr: true,
            enable_autonat: true,
            idle_connection_timeout: Duration::from_secs(60),
            ping_interval: Duration::from_secs(15),
            ping_timeout: Duration::from_secs(10),
            kad_query_timeout: Duration::from_secs(60),
        }
    }
}

impl NodeConfig {
    pub fn new(protocol_version: impl Into<String>, agent_version: impl Into<String>) -> Self {
        Self {
            protocol_version: protocol_version.into(),
            agent_version: agent_version.into(),
            ..Default::default()
        }
    }

    pub fn with_listen_addrs(mut self, addrs: Vec<Multiaddr>) -> Self {
        self.listen_addrs = addrs;
        self
    }

    pub fn with_bootstrap_peers(mut self, peers: Vec<(PeerId, Multiaddr)>) -> Self {
        self.bootstrap_peers = peers;
        self
    }

    pub fn with_mdns(mut self, enable: bool) -> Self {
        self.enable_mdns = enable;
        self
    }

    pub fn with_relay_client(mut self, enable: bool) -> Self {
        self.enable_relay_client = enable;
        self
    }

    pub fn with_dcutr(mut self, enable: bool) -> Self {
        self.enable_dcutr = enable;
        self
    }

    pub fn with_autonat(mut self, enable: bool) -> Self {
        self.enable_autonat = enable;
        self
    }
}
