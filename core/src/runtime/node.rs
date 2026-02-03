use libp2p::{identity::Keypair, swarm::Config as SwarmConfig, Swarm};
use tokio::sync::mpsc;

use super::client::{EventReceiver, NetClient};
use super::event_loop::EventLoop;
use super::transport::build_transport;
use super::CoreBehaviour;
use crate::config::NodeConfig;
use crate::Result;

const COMMAND_CHANNEL_SIZE: usize = 32;
const EVENT_CHANNEL_SIZE: usize = 64;

/// 启动节点
///
/// 返回 (NetClient, EventReceiver)：
/// - NetClient: 用于发送命令（dial, close 等）
/// - EventReceiver: 用于接收事件（peer discovered, connected 等）
pub fn start(keypair: &Keypair, config: NodeConfig) -> Result<(NetClient, EventReceiver)> {
    // 构建 transport
    let transport_output = build_transport(keypair)?;

    // 构建 behaviour
    let behaviour = CoreBehaviour::new(keypair, transport_output.relay_client, &config)?;

    // 构建 swarm
    let swarm = Swarm::new(
        transport_output.transport,
        behaviour,
        keypair.public().to_peer_id(),
        SwarmConfig::with_tokio_executor()
            .with_idle_connection_timeout(config.idle_connection_timeout),
    );
    
    // 创建 channels
    let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_SIZE);

    // 创建 event loop
    let mut event_loop = EventLoop::new(swarm, command_rx, event_tx, config.protocol_version.clone());

    // 启动监听
    event_loop.start_listen(&config.listen_addrs)?;

    // 启动 event loop
    tokio::spawn(event_loop.run());

    // 返回 client 和 event receiver
    let client = NetClient::new(command_tx);
    let event_receiver = EventReceiver::new(event_rx);

    Ok((client, event_receiver))
}
