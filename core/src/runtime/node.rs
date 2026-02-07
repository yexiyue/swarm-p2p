use std::time::Duration;

use anyhow::Result;
use libp2p::{noise, tcp, yamux, SwarmBuilder};
use tokio::sync::mpsc;

use super::event_loop::EventLoop;
use super::{CborMessage, CoreBehaviour};
use crate::client::{EventReceiver, NetClient};
use crate::config::NodeConfig;
use crate::pending_map::PendingMap;

const COMMAND_CHANNEL_SIZE: usize = 32;
const EVENT_CHANNEL_SIZE: usize = 64;

/// 启动节点
///
/// 返回 (NetClient, EventReceiver)：
/// - NetClient: 用于发送命令（dial, close 等）
/// - EventReceiver: 用于接收事件（peer discovered, connected 等）
///
/// Transport 层包含：
/// - TCP + Noise + Yamux（稳定连接，防火墙友好）
/// - QUIC（内置 TLS 1.3 加密和多路复用，NAT 穿透更优）
/// - Relay client（无法直连时的兜底）
/// - DNS 解析（支持 /dnsaddr/, /dns4/, /dns6/ multiaddr）
/// Pending request 默认 TTL（60 秒）
const PENDING_REQUEST_TTL: Duration = Duration::from_secs(60);

pub fn start<Req, Resp>(
    keypair: libp2p::identity::Keypair,
    config: NodeConfig,
) -> Result<(NetClient<Req, Resp>, EventReceiver<Req>)>
where
    Req: CborMessage,
    Resp: CborMessage,
{
    // 使用 SwarmBuilder 构建 swarm
    // 自动配置 TCP + QUIC + DNS + Relay 多协议传输层
    let swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
        .with_quic()
        .with_dns()?
        .with_relay_client(noise::Config::new, yamux::Config::default)?
        .with_behaviour(|key, relay_client| CoreBehaviour::<Req, Resp>::new(key, relay_client, &config))
        .unwrap()
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(config.idle_connection_timeout))
        .build();

    // 创建 channels
    let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);

    // 创建共享的 PendingMap（EventLoop 存入，NetClient 取出）
    let pending_channels = PendingMap::new(PENDING_REQUEST_TTL);

    // 创建 event loop
    let mut event_loop = EventLoop::new(
        swarm,
        command_rx,
        event_tx,
        pending_channels.clone(),
        config.protocol_version.clone(),
    );

    // 启动监听
    event_loop.start_listen(&config.listen_addrs)?;

    // 启动 event loop
    tokio::spawn(event_loop.run());

    // 返回 client 和 event receiver
    let client = NetClient::new(command_tx, pending_channels);
    let event_receiver = EventReceiver::new(event_rx);

    Ok((client, event_receiver))
}
