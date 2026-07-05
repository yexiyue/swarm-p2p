use std::num::NonZeroUsize;
use std::time::Duration;

use libp2p::{autonat, identify, identity::Keypair, kad, ping, relay, swarm::NetworkBehaviour};

pub const DEFAULT_MAX_RESERVATIONS: usize = 128;
pub const DEFAULT_MAX_RESERVATIONS_PER_PEER: usize = 4;
pub const DEFAULT_RESERVATION_DURATION_SECS: u64 = 3600;
pub const DEFAULT_MAX_CIRCUITS: usize = 16;
pub const DEFAULT_MAX_CIRCUITS_PER_PEER: usize = 4;
/// 12 小时：与客户端 LAN Helper 的 relay 限额策略一致，
/// 已配对设备间的大文件中继传输不应被时长上限掐断。
pub const DEFAULT_MAX_CIRCUIT_DURATION_SECS: u64 = 12 * 3600;
/// 0 = 不限字节（libp2p 语义），文件传输体积由应用层负责。
pub const DEFAULT_MAX_CIRCUIT_BYTES: u64 = 0;

#[derive(Debug, Clone, Copy)]
pub struct RelayLimits {
    pub max_reservations: usize,
    pub max_reservations_per_peer: usize,
    pub reservation_duration: Duration,
    pub max_circuits: usize,
    pub max_circuits_per_peer: usize,
    pub max_circuit_duration: Duration,
    pub max_circuit_bytes: u64,
}

impl Default for RelayLimits {
    fn default() -> Self {
        Self {
            max_reservations: DEFAULT_MAX_RESERVATIONS,
            max_reservations_per_peer: DEFAULT_MAX_RESERVATIONS_PER_PEER,
            reservation_duration: Duration::from_secs(DEFAULT_RESERVATION_DURATION_SECS),
            max_circuits: DEFAULT_MAX_CIRCUITS,
            max_circuits_per_peer: DEFAULT_MAX_CIRCUITS_PER_PEER,
            max_circuit_duration: Duration::from_secs(DEFAULT_MAX_CIRCUIT_DURATION_SECS),
            max_circuit_bytes: DEFAULT_MAX_CIRCUIT_BYTES,
        }
    }
}

/// 引导+中继节点的轻量网络行为
///
/// 只包含服务端必需的协议：
/// - `ping`: 心跳保活
/// - `identify`: 节点信息交换（客户端通过 identify 获取引导节点的监听地址）
/// - `kad`: Kademlia DHT Server 模式，响应所有 DHT 查询
/// - `relay`: Relay Server，为 NAT 后的节点中继流量
/// - `autonat`: AutoNAT v2 Server，响应客户端的 NAT 检测请求
#[derive(NetworkBehaviour)]
pub struct BootstrapBehaviour {
    pub ping: ping::Behaviour,
    pub identify: identify::Behaviour,
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    pub relay: relay::Behaviour,
    pub autonat: autonat::v2::server::Behaviour,
}

impl BootstrapBehaviour {
    pub fn new(keypair: &Keypair, relay_limits: RelayLimits) -> Self {
        let peer_id = keypair.public().to_peer_id();

        // ===== Ping =====
        let ping = ping::Behaviour::new(
            ping::Config::new()
                .with_interval(Duration::from_secs(15))
                .with_timeout(Duration::from_secs(10)),
        );

        // ===== Identify =====
        // protocol_version 必须与客户端一致（"/swarmdrop/1.0.0"），
        // 客户端 event_loop 只在 protocol_version 匹配时才将对方加入 Kad 路由表。
        let identify = identify::Behaviour::new(
            identify::Config::new("/swarmdrop/1.0.0".to_string(), keypair.public())
                .with_agent_version(format!("swarm-bootstrap/{}", env!("CARGO_PKG_VERSION")))
                .with_push_listen_addr_updates(true)
                .with_cache_size(1000),
        );

        // ===== Kademlia DHT =====
        // 强制 Server 模式：始终响应 DHT 查询
        // 作为引导节点，record_ttl 和 replication_factor 适当放大
        let mut kad_config = kad::Config::default();
        kad_config
            .set_query_timeout(Duration::from_secs(60))
            .set_record_ttl(Some(Duration::from_secs(7200))) // 2 小时
            .set_replication_factor(NonZeroUsize::new(20).unwrap())
            .set_publication_interval(Some(Duration::from_secs(3600)))
            .set_provider_record_ttl(Some(Duration::from_secs(7200)));

        let mut kad =
            kad::Behaviour::with_config(peer_id, kad::store::MemoryStore::new(peer_id), kad_config);
        kad.set_mode(Some(kad::Mode::Server));

        // ===== Relay Server =====
        // 为 NAT 后的节点提供中继服务
        // relay::Behaviour 是服务端，与客户端的 relay::client::Behaviour 不同
        //
        // libp2p 默认限制过于严格（128KB / 2min），文件传输会被切断。
        // 默认放宽为 字节不限 + 12h，与客户端 LAN Helper 策略对齐
        // （理想情况下 DCUtR 打洞成功后会走直连，relay 只在打洞失败时兜底；
        // public_reachability 开启后 LanOnly 设备也直接在本节点保活 reservation）。
        let relay_config = relay::Config {
            max_reservations: relay_limits.max_reservations,
            max_reservations_per_peer: relay_limits.max_reservations_per_peer,
            reservation_duration: relay_limits.reservation_duration,
            max_circuits: relay_limits.max_circuits,
            max_circuits_per_peer: relay_limits.max_circuits_per_peer,
            max_circuit_bytes: relay_limits.max_circuit_bytes,
            max_circuit_duration: relay_limits.max_circuit_duration,
            ..Default::default()
        };
        let relay = relay::Behaviour::new(peer_id, relay_config);

        // ===== AutoNAT v2 Server =====
        // 为客户端提供 NAT 检测服务：客户端请求引导节点回拨其地址，
        // 以此判断客户端是否公网可达。
        let autonat = autonat::v2::server::Behaviour::default();

        Self {
            ping,
            identify,
            kad,
            relay,
            autonat,
        }
    }
}
