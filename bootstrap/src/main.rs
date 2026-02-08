use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use libp2p::Multiaddr;
use tracing::info;

/// SwarmDrop 引导+中继节点
#[derive(Parser, Debug)]
#[command(name = "swarm-bootstrap")]
#[command(version, about = "SwarmDrop DHT bootstrap and relay server")]
struct Args {
    /// TCP 监听端口
    #[arg(long, default_value = "4001")]
    tcp_port: u16,

    /// QUIC 监听端口
    #[arg(long, default_value = "4001")]
    quic_port: u16,

    /// 密钥文件路径（protobuf 编码的 Ed25519 密钥对）
    #[arg(long, default_value = "identity.key")]
    key_file: PathBuf,

    /// 监听的 IP 地址
    #[arg(long, default_value = "0.0.0.0")]
    listen_addr: String,

    /// 空闲连接超时（秒）
    #[arg(long, default_value = "120")]
    idle_timeout: u64,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    let keypair = swarm_bootstrap::util::load_or_generate_keypair(&args.key_file)?;
    let peer_id = keypair.public().to_peer_id();
    info!("Node PeerId: {}", peer_id);

    let tcp_addr: Multiaddr = format!("/ip4/{}/tcp/{}", args.listen_addr, args.tcp_port).parse()?;
    let quic_addr: Multiaddr =
        format!("/ip4/{}/udp/{}/quic-v1", args.listen_addr, args.quic_port).parse()?;

    info!("TCP listen address: {}", tcp_addr);
    info!("QUIC listen address: {}", quic_addr);

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(swarm_bootstrap::run(
            keypair,
            tcp_addr,
            quic_addr,
            Duration::from_secs(args.idle_timeout),
        ))
}
