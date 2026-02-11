use std::sync::atomic::{AtomicU64, Ordering};

use futures::StreamExt;
use libp2p::request_response::{Event as ReqRespEvent, Message};
use libp2p::swarm::SwarmEvent;
use libp2p::{autonat, dcutr, ping};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::{CborMessage, CoreBehaviourEvent};
use crate::command::{Command, CoreSwarm};
use crate::event::{NatStatus, NodeEvent};
use crate::pending_map::PendingMap;

/// 事件循环
pub struct EventLoop<Req, Resp>
where
    Req: CborMessage,
    Resp: CborMessage,
{
    swarm: CoreSwarm<Req, Resp>,
    command_rx: mpsc::Receiver<Command<Req, Resp>>,
    event_tx: mpsc::Sender<NodeEvent<Req>>,
    active_commands: Vec<Command<Req, Resp>>,
    /// 本机的协议版本，用于判断是否加入 Kad
    protocol_version: String,
    /// 暂存 inbound request 的 ResponseChannel，等待前端回复
    pending_channels: PendingMap<u64, libp2p::request_response::ResponseChannel<Resp>>,
    /// pending_id 自增计数器
    pending_id_counter: AtomicU64,
}

impl<Req, Resp> EventLoop<Req, Resp>
where
    Req: CborMessage,
    Resp: CborMessage,
{
    pub fn new(
        swarm: CoreSwarm<Req, Resp>,
        command_rx: mpsc::Receiver<Command<Req, Resp>>,
        event_tx: mpsc::Sender<NodeEvent<Req>>,
        pending_channels: PendingMap<u64, libp2p::request_response::ResponseChannel<Resp>>,
        protocol_version: String,
    ) -> Self {
        Self {
            swarm,
            command_rx,
            event_tx,
            active_commands: Vec::new(),
            protocol_version,
            pending_channels,
            pending_id_counter: AtomicU64::new(0),
        }
    }

    /// 启动监听
    pub fn start_listen(&mut self, addrs: &[libp2p::Multiaddr]) -> crate::Result<()> {
        for addr in addrs {
            self.swarm
                .listen_on(addr.clone())
                .map_err(|e| crate::error::Error::Listen(e.to_string()))?;
        }
        Ok(())
    }

    /// 连接引导节点：注册地址到 Kad 路由表并 dial
    pub fn connect_bootstrap_peers(&mut self, peers: &[(libp2p::PeerId, libp2p::Multiaddr)]) {
        for (peer_id, addr) in peers {
            self.swarm
                .behaviour_mut()
                .kad
                .add_address(peer_id, addr.clone());
            self.swarm.add_peer_address(*peer_id, addr.clone());
            if let Err(e) = self.swarm.dial(*peer_id) {
                warn!("Failed to dial bootstrap peer {}: {}", peer_id, e);
            } else {
                info!("Dialing bootstrap peer {} at {}", peer_id, addr);
            }
        }
    }

    /// 运行事件循环
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                // 处理外部命令
                cmd = self.command_rx.recv() => {
                    match cmd {
                        Some(cmd) => self.handle_command(cmd).await,
                        None => {
                            info!("Command channel closed, shutting down");
                            return;
                        }
                    }
                }
                // 处理 swarm 事件
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }
            }
        }
    }

    async fn handle_command(&mut self, mut cmd: Command<Req, Resp>) {
        cmd.run_boxed(&mut self.swarm).await;
        self.active_commands.push(cmd);
    }

    async fn handle_swarm_event(&mut self, event: SwarmEvent<CoreBehaviourEvent<Req, Resp>>) {
        // 命令链：依次传递 owned event，命令可选择消费或传递
        let mut remaining = Some(event);
        let mut i = 0;
        while i < self.active_commands.len() {
            let Some(event) = remaining.take() else {
                break; // 事件已被消费，后续命令不再处理
            };
            let (keep, returned) = self.active_commands[i].on_event_boxed(event).await;
            remaining = returned;
            if keep {
                i += 1;
            } else {
                self.active_commands.swap_remove(i);
            }
        }

        // 未被命令消费的事件，转换为前端事件
        let Some(event) = remaining else {
            return;
        };

        if let Some(evt) = self.convert_to_node_event(event) {
            let _ = self.event_tx.send(evt).await;
        }
    }

    fn next_pending_id(&self) -> u64 {
        self.pending_id_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// 将 swarm 事件转换为对外事件
    fn convert_to_node_event(
        &mut self,
        event: SwarmEvent<CoreBehaviourEvent<Req, Resp>>,
    ) -> Option<NodeEvent<Req>> {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                Some(NodeEvent::Listening { addr: address })
            }
            // 只在第一个连接建立时通知（peer 级别聚合）
            SwarmEvent::ConnectionEstablished {
                peer_id,
                num_established,
                ..
            } if num_established.get() == 1 => Some(NodeEvent::PeerConnected { peer_id }),
            SwarmEvent::ConnectionEstablished { .. } => None,
            // 只在最后一个连接关闭时通知（peer 级别聚合）
            SwarmEvent::ConnectionClosed {
                peer_id,
                num_established,
                ..
            } if num_established == 0 => Some(NodeEvent::PeerDisconnected { peer_id }),
            // Inbound request: 取出 ResponseChannel 暂存，通知前端
            SwarmEvent::Behaviour(CoreBehaviourEvent::ReqResp(ReqRespEvent::Message {
                peer,
                message:
                    Message::Request {
                        request, channel, ..
                    },
                ..
            })) => {
                let pending_id = self.next_pending_id();
                info!(
                    "Inbound request from {}, assigned pending_id={}",
                    peer, pending_id
                );
                self.pending_channels.insert(pending_id, channel);
                Some(NodeEvent::InboundRequest {
                    peer_id: peer,
                    pending_id,
                    request,
                })
            }
            SwarmEvent::Behaviour(CoreBehaviourEvent::Dcutr(dcutr::Event {
                remote_peer_id,
                result,
            })) => match result {
                Ok(_connection_id) => {
                    info!("DCUtR hole-punch succeeded with {}", remote_peer_id);
                    Some(NodeEvent::HolePunchSucceeded {
                        peer_id: remote_peer_id,
                    })
                }
                Err(e) => {
                    warn!("DCUtR hole-punch failed with {}: {}", remote_peer_id, e);
                    Some(NodeEvent::HolePunchFailed {
                        peer_id: remote_peer_id,
                        error: e.to_string(),
                    })
                }
            },
            SwarmEvent::Behaviour(CoreBehaviourEvent::Mdns(libp2p::mdns::Event::Discovered(
                peers,
            ))) => {
                // 先注册所有地址，再 dial（dial by PeerId 会使用所有已知地址）
                for (peer_id, addr) in &peers {
                    self.swarm.add_peer_address(*peer_id, addr.clone());
                }

                let dialed: std::collections::HashSet<_> =
                    peers.iter().map(|(id, _)| *id).collect();

                for peer_id in &dialed {
                    if !self.swarm.is_connected(peer_id) {
                        info!("mDNS: dialing peer {}", peer_id);
                        if let Err(e) = self.swarm.dial(*peer_id) {
                            warn!("Failed to dial discovered peer {}: {}", peer_id, e);
                        }
                    }
                }
                Some(NodeEvent::PeersDiscovered { peers })
            }
            SwarmEvent::Behaviour(CoreBehaviourEvent::Ping(ping::Event {
                peer,
                result: Ok(rtt),
                ..
            })) => Some(NodeEvent::PingSuccess {
                peer_id: peer,
                rtt_ms: rtt.as_millis() as u64,
            }),
            SwarmEvent::Behaviour(CoreBehaviourEvent::Identify(
                libp2p::identify::Event::Received { peer_id, info, .. },
            )) => {
                // 如果协议版本匹配，自动加入 Kad 并注册地址到 Swarm
                if info.protocol_version == self.protocol_version {
                    for addr in &info.listen_addrs {
                        self.swarm
                            .behaviour_mut()
                            .kad
                            .add_address(&peer_id, addr.clone());
                        self.swarm.add_peer_address(peer_id, addr.clone());
                    }
                    info!(
                        "Added peer {} to Kad + Swarm (protocol: {})",
                        peer_id, info.protocol_version
                    );
                }
                Some(NodeEvent::IdentifyReceived {
                    peer_id,
                    agent_version: info.agent_version,
                    protocol_version: info.protocol_version,
                })
            }
            SwarmEvent::Behaviour(CoreBehaviourEvent::Autonat(autonat::v2::client::Event {
                tested_addr,
                server,
                result,
                ..
            })) => match result {
                Ok(()) => {
                    info!(
                        "AutoNAT: address {} confirmed reachable by {}",
                        tested_addr, server
                    );
                    Some(NodeEvent::NatStatusChanged {
                        status: NatStatus::Public,
                        public_addr: Some(tested_addr),
                    })
                }
                Err(e) => {
                    debug!(
                        "AutoNAT: address {} not reachable via {}: {}",
                        tested_addr, server, e
                    );
                    None
                }
            },
            // Kad 路由表更新：将学到的地址同步到 Swarm 地址簿，
            // 确保后续 dial(peer_id) 能找到地址（跨网络 DHT 查询场景）
            SwarmEvent::Behaviour(CoreBehaviourEvent::Kad(
                libp2p::kad::Event::RoutingUpdated {
                    peer, addresses, ..
                },
            )) => {
                for addr in addresses.iter() {
                    self.swarm.add_peer_address(peer, addr.clone());
                }
                debug!(
                    "Kad routing updated for {}, synced {} addrs to swarm",
                    peer,
                    addresses.len()
                );
                None
            }
            _ => None,
        }
    }
}
