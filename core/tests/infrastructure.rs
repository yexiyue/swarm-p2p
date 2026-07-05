//! 集成测试：LAN Helper / infrastructure peer 动态注册。

mod common;

use common::*;
use swarm_p2p_core::libp2p::PeerId;
use swarm_p2p_core::{
    InfrastructureRoles, LanHelperConfig, NetClient, NodeConfig, NodeEvent, start,
};
use tokio::time::{Duration, timeout};

fn helper_config() -> NodeConfig {
    // 单机测试全在 loopback 上：必须公告 loopback 地址，
    // 否则 reservation 响应无地址、客户端以 NoAddressesInReservation 拒绝
    explicit_dial_config().with_lan_helper(LanHelperConfig {
        announce_loopback_addrs: true,
        ..LanHelperConfig::default()
    })
}

fn client_config(relay_client: bool) -> NodeConfig {
    NodeConfig::new("/test/1.0.0", "test/1.0.0")
        .with_listen_addrs(vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()])
        .with_mdns(false)
        .with_relay_client(relay_client)
        .with_dcutr(false)
        .with_autonat(false)
        .with_kad_server_mode(true)
}

async fn start_helper_and_client(
    relay_client: bool,
) -> (
    NetClient<Ping, Pong>,
    NetClient<Ping, Pong>,
    swarm_p2p_core::EventReceiver<Ping>,
    PeerId,
    swarm_p2p_core::EventReceiver<Ping>,
    PeerId,
    swarm_p2p_core::libp2p::Multiaddr,
) {
    let helper_key = swarm_p2p_core::libp2p::identity::Keypair::generate_ed25519();
    let client_key = swarm_p2p_core::libp2p::identity::Keypair::generate_ed25519();
    let helper_id = PeerId::from_public_key(&helper_key.public());
    let client_id = PeerId::from_public_key(&client_key.public());

    let (helper_client, mut helper_events, _helper_dc) =
        start::<Ping, Pong>(helper_key, helper_config()).expect("start helper");
    let (client, mut client_events, _client_dc) =
        start::<Ping, Pong>(client_key, client_config(relay_client)).expect("start client");

    let helper_addr = wait_for_listen_addr(&mut helper_events).await;
    let _client_addr = wait_for_listen_addr(&mut client_events).await;

    (
        helper_client,
        client,
        client_events,
        client_id,
        helper_events,
        helper_id,
        helper_addr,
    )
}

async fn retry_register_infrastructure_until_connected(
    client: &NetClient<Ping, Pong>,
    helper_id: PeerId,
    helper_addr: swarm_p2p_core::libp2p::Multiaddr,
    roles: InfrastructureRoles,
) {
    timeout(TIMEOUT, async {
        loop {
            if client
                .is_connected(helper_id)
                .await
                .expect("is_connected command should complete")
            {
                return;
            }
            if let Err(err) = client
                .add_infrastructure_peer(helper_id, vec![helper_addr.clone()], roles)
                .await
            {
                eprintln!("[test] add_infrastructure_peer failed: {err}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("infrastructure peer should connect within timeout");
}

#[tokio::test(flavor = "multi_thread")]
async fn dynamic_infrastructure_peer_requests_relay_reservation() {
    let (
        _helper_client,
        client,
        _client_events,
        _client_id,
        mut helper_events,
        helper_id,
        helper_addr,
    ) = start_helper_and_client(true).await;

    retry_register_infrastructure_until_connected(
        &client,
        helper_id,
        helper_addr,
        InfrastructureRoles::kad_and_relay(),
    )
    .await;

    timeout(TIMEOUT, async {
        loop {
            match helper_events.recv().await {
                Some(NodeEvent::RelayServerReservationAccepted { src_peer_id, .. }) => {
                    assert_ne!(src_peer_id, helper_id);
                    return;
                }
                Some(_) => {}
                None => panic!("helper event stream closed"),
            }
        }
    })
    .await
    .expect("helper should accept dynamic relay reservation");
}

/// 等待针对指定 relay 的 reservation accepted 事件
async fn wait_reservation_accepted(
    events: &mut swarm_p2p_core::EventReceiver<Ping>,
    relay: PeerId,
    window: Duration,
) -> bool {
    timeout(window, async {
        loop {
            match events.recv().await {
                Some(NodeEvent::RelayReservationAccepted { relay_peer_id, .. })
                    if relay_peer_id == relay =>
                {
                    return;
                }
                Some(_) => {}
                None => panic!("event stream closed"),
            }
        }
    })
    .await
    .is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn ensure_relay_reservation_is_idempotent() {
    let (
        _helper_client,
        client,
        mut client_events,
        _client_id,
        _helper_events,
        helper_id,
        helper_addr,
    ) = start_helper_and_client(true).await;

    client
        .ensure_relay_reservation(helper_id, vec![helper_addr.clone()])
        .await
        .expect("ensure_relay_reservation");
    assert!(
        wait_reservation_accepted(&mut client_events, helper_id, TIMEOUT).await,
        "first ensure should establish reservation"
    );

    // 再次 ensure：已有活跃 listener，必须 no-op（不产生第二次 accepted）
    client
        .ensure_relay_reservation(helper_id, vec![helper_addr])
        .await
        .expect("second ensure");
    assert!(
        !wait_reservation_accepted(&mut client_events, helper_id, Duration::from_secs(2)).await,
        "second ensure must be a no-op (no new reservation request)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reservation_lost_event_and_reestablish() {
    let helper_key = swarm_p2p_core::libp2p::identity::Keypair::generate_ed25519();
    let helper_id = PeerId::from_public_key(&helper_key.public());
    let (helper_client, mut helper_events, _helper_dc) =
        start::<Ping, Pong>(helper_key, helper_config()).expect("start helper");
    let helper_addr = wait_for_listen_addr(&mut helper_events).await;

    let client_key = swarm_p2p_core::libp2p::identity::Keypair::generate_ed25519();
    let (client, mut client_events, _client_dc) =
        start::<Ping, Pong>(client_key, client_config(true)).expect("start client");
    let _ = wait_for_listen_addr(&mut client_events).await;

    client
        .ensure_relay_reservation(helper_id, vec![helper_addr])
        .await
        .expect("ensure_relay_reservation");
    assert!(
        wait_reservation_accepted(&mut client_events, helper_id, TIMEOUT).await,
        "reservation should be established"
    );

    // 杀掉 helper：连接断 → circuit listener 永久关闭 → 必须上抛 Lost 事件
    drop(helper_client);
    drop(helper_events);

    let lost = timeout(TIMEOUT, async {
        loop {
            match client_events.recv().await {
                Some(NodeEvent::RelayReservationLost { relay_peer_id })
                    if relay_peer_id == helper_id =>
                {
                    return;
                }
                Some(_) => {}
                None => panic!("event stream closed"),
            }
        }
    })
    .await;
    assert!(
        lost.is_ok(),
        "listener closed must surface RelayReservationLost"
    );

    // helper 重启（同 key、新端口）后，再次 ensure 可重建
    let helper_key2 = swarm_p2p_core::libp2p::identity::Keypair::generate_ed25519();
    let helper_id2 = PeerId::from_public_key(&helper_key2.public());
    let (_helper_client2, mut helper_events2, _dc2) =
        start::<Ping, Pong>(helper_key2, helper_config()).expect("restart helper");
    let helper_addr2 = wait_for_listen_addr(&mut helper_events2).await;

    client
        .ensure_relay_reservation(helper_id2, vec![helper_addr2])
        .await
        .expect("re-ensure after loss");
    assert!(
        wait_reservation_accepted(&mut client_events, helper_id2, TIMEOUT).await,
        "reservation should be re-established after loss"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dynamic_infrastructure_peer_skips_reservation_when_relay_client_disabled() {
    let (
        _helper_client,
        client,
        mut client_events,
        _client_id,
        _helper_events,
        helper_id,
        helper_addr,
    ) = start_helper_and_client(false).await;

    retry_register_infrastructure_until_connected(
        &client,
        helper_id,
        helper_addr,
        InfrastructureRoles::kad_and_relay(),
    )
    .await;

    let maybe_reservation = timeout(Duration::from_millis(300), async {
        loop {
            match client_events.recv().await {
                Some(NodeEvent::RelayReservationAccepted { relay_peer_id, .. })
                    if relay_peer_id == helper_id =>
                {
                    return Some(());
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        maybe_reservation.is_err(),
        "relay client disabled should not request reservation"
    );
}

/// 回归：配置层 bootstrap 节点不再自动建立 relay reservation——
/// 公网 reservation 属策略决策（public_reachability），统一由上层
/// ensure_relay_reservation / add_infrastructure_peer 显式发起。
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_peers_do_not_auto_reserve() {
    let helper_key = swarm_p2p_core::libp2p::identity::Keypair::generate_ed25519();
    let helper_id = PeerId::from_public_key(&helper_key.public());
    let (_helper_client, mut helper_events, _dc) =
        start::<Ping, Pong>(helper_key, helper_config()).expect("start helper");
    let helper_addr = wait_for_listen_addr(&mut helper_events).await;

    let client_key = swarm_p2p_core::libp2p::identity::Keypair::generate_ed25519();
    let config = client_config(true).with_bootstrap_peers(vec![(helper_id, helper_addr.clone())]);
    let (client, mut client_events, _cdc) =
        start::<Ping, Pong>(client_key, config).expect("start client");

    // 等待连接建立（bootstrap 自动 dial）
    timeout(TIMEOUT, async {
        loop {
            if client.is_connected(helper_id).await.unwrap_or(false) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("bootstrap peer should connect");

    // identify 完成后的时间窗内不得出现自动 reservation
    assert!(
        !wait_reservation_accepted(&mut client_events, helper_id, Duration::from_secs(3)).await,
        "bootstrap 连接不应自动建立 reservation"
    );

    // 显式 ensure 仍可建立（策略层的正常路径）
    client
        .ensure_relay_reservation(helper_id, vec![helper_addr])
        .await
        .expect("ensure");
    assert!(
        wait_reservation_accepted(&mut client_events, helper_id, TIMEOUT).await,
        "显式 ensure 应建立 reservation"
    );
}
