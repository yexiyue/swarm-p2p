//! 集成测试：keep-alive 白名单 vs idle_connection_timeout
//!
//! 用极短的空闲超时（1s）验证：
//! - 白名单内 peer 的连接不被空闲回收
//! - 白名单外连接照常空闲回收
//! - 运行中移除白名单后恢复空闲回收（NotifyHandler 动态路径）

mod common;

use std::time::Duration;

use common::*;
use swarm_p2p_core::libp2p::PeerId;
use swarm_p2p_core::{NodeConfig, NodeEvent, start};
use tokio::time::timeout;

const IDLE: Duration = Duration::from_secs(1);

fn short_idle_config() -> NodeConfig {
    explicit_dial_config().with_idle_connection_timeout(IDLE)
}

struct Node {
    client: swarm_p2p_core::NetClient<Ping, Pong>,
    events: swarm_p2p_core::EventReceiver<Ping>,
    peer_id: PeerId,
}

fn start_node() -> (Node, swarm_p2p_core::libp2p::Multiaddr) {
    let keypair = swarm_p2p_core::libp2p::identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from_public_key(&keypair.public());
    let (client, events, _dc) =
        start::<Ping, Pong>(keypair, short_idle_config()).expect("failed to start node");
    (
        Node {
            client,
            events,
            peer_id,
        },
        "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
    )
}

async fn connect(a: &mut Node, b: &mut Node) {
    let addr_a = wait_for_listen_addr(&mut a.events).await;
    let addr_b = wait_for_listen_addr(&mut b.events).await;
    connect_by_explicit_dial(&a.client, a.peer_id, addr_a, &b.client, b.peer_id, addr_b).await;
}

/// 在给定时间窗内等待对指定 peer 的 PeerDisconnected 事件
async fn wait_disconnected(
    events: &mut swarm_p2p_core::EventReceiver<Ping>,
    peer: PeerId,
    window: Duration,
) -> bool {
    timeout(window, async {
        loop {
            match events.recv().await {
                Some(NodeEvent::PeerDisconnected { peer_id }) if peer_id == peer => return,
                Some(_) => continue,
                None => panic!("event stream closed"),
            }
        }
    })
    .await
    .is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn whitelisted_peer_survives_idle_timeout() {
    let (mut a, _) = start_node();
    let (mut b, _) = start_node();

    // 建连前双向加入白名单（配对场景下两端各自豁免对方）
    a.client
        .set_keep_alive(b.peer_id, true)
        .await
        .expect("A set_keep_alive");
    b.client
        .set_keep_alive(a.peer_id, true)
        .await
        .expect("B set_keep_alive");

    connect(&mut a, &mut b).await;

    // 等待远超 idle 超时的时间，连接必须仍在
    let disconnected = wait_disconnected(&mut a.events, b.peer_id, IDLE * 4).await;
    assert!(!disconnected, "whitelisted connection must not idle-close");
    assert!(
        a.client
            .is_connected(b.peer_id)
            .await
            .expect("is_connected"),
        "A should still be connected to B"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn non_whitelisted_peer_idle_closes() {
    let (mut a, _) = start_node();
    let (mut b, _) = start_node();

    connect(&mut a, &mut b).await;

    // 无白名单：空闲 1s 后应被回收（留足事件传播余量）
    let disconnected = wait_disconnected(&mut a.events, b.peer_id, TIMEOUT).await;
    assert!(
        disconnected,
        "idle connection should close without keep-alive"
    );
    assert!(
        !a.client
            .is_connected(b.peer_id)
            .await
            .expect("is_connected"),
        "A should be disconnected from B"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_keep_alive_resumes_idle_close() {
    let (mut a, _) = start_node();
    let (mut b, _) = start_node();

    a.client
        .set_keep_alive(b.peer_id, true)
        .await
        .expect("A set_keep_alive");
    b.client
        .set_keep_alive(a.peer_id, true)
        .await
        .expect("B set_keep_alive");

    connect(&mut a, &mut b).await;

    // 白名单生效期：连接稳定存活
    let disconnected = wait_disconnected(&mut a.events, b.peer_id, IDLE * 3).await;
    assert!(!disconnected, "connection should survive while whitelisted");

    // 双向移除白名单（对既有连接走 NotifyHandler 动态下发）
    a.client
        .set_keep_alive(b.peer_id, false)
        .await
        .expect("A unset_keep_alive");
    b.client
        .set_keep_alive(a.peer_id, false)
        .await
        .expect("B unset_keep_alive");

    let disconnected = wait_disconnected(&mut a.events, b.peer_id, TIMEOUT).await;
    assert!(
        disconnected,
        "connection should idle-close after keep-alive removed"
    );
}
