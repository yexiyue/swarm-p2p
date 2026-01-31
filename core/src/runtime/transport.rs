use libp2p::{
    core::muxing::StreamMuxerBox,
    identity::Keypair,
    relay,
    tcp, yamux, PeerId, Transport,
};

use crate::Result;

/// Transport 构建结果
pub struct TransportOutput {
    pub transport: libp2p::core::transport::Boxed<(PeerId, StreamMuxerBox)>,
    pub relay_client: relay::client::Behaviour,
}

/// 构建 Transport 层
///
/// 包含：
/// - TCP + Noise + Yamux
/// - Relay client（用于 NAT 穿透）
pub fn build_transport(keypair: &Keypair) -> Result<TransportOutput> {
    let peer_id = keypair.public().to_peer_id();

    // TCP transport with Noise encryption and Yamux muxing
    let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default())
        .upgrade(libp2p::core::upgrade::Version::V1)
        .authenticate(libp2p::noise::Config::new(keypair).expect("noise config"))
        .multiplex(yamux::Config::default())
        .boxed();

    // Relay client transport
    let (relay_transport, relay_client) = relay::client::new(peer_id);

    // Relay transport with Noise + Yamux
    let relay_transport = relay_transport
        .upgrade(libp2p::core::upgrade::Version::V1)
        .authenticate(libp2p::noise::Config::new(keypair).expect("noise config"))
        .multiplex(yamux::Config::default())
        .boxed();

    // 组合 TCP 和 Relay transport
    let transport = libp2p::core::transport::OrTransport::new(tcp_transport, relay_transport)
        .map(|either, _| match either {
            futures::future::Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            futures::future::Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed();

    Ok(TransportOutput {
        transport,
        relay_client,
    })
}
