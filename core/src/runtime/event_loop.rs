use futures::StreamExt;
use libp2p::{autonat, ping};
use libp2p::swarm::SwarmEvent;
use tokio::sync::mpsc;
use tracing::{info, trace, warn};

use super::CoreBehaviourEvent;
use crate::command::{Command, CoreSwarm};
use crate::event::{NatStatus, NodeEvent};

/// 事件循环
pub struct EventLoop {
    swarm: CoreSwarm,
    command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<NodeEvent>,
    active_commands: Vec<Command>,
    /// 本机的协议版本，用于判断是否加入 Kad
    protocol_version: String,
}

impl EventLoop {
    pub fn new(
        swarm: CoreSwarm,
        command_rx: mpsc::Receiver<Command>,
        event_tx: mpsc::Sender<NodeEvent>,
        protocol_version: String,
    ) -> Self {
        Self {
            swarm,
            command_rx,
            event_tx,
            active_commands: Vec::new(),
            protocol_version,
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

    async fn handle_command(&mut self, mut cmd: Command) {
        cmd.run_boxed(&mut self.swarm).await;
        self.active_commands.push(cmd);
    }

    async fn handle_swarm_event(&mut self, event: SwarmEvent<CoreBehaviourEvent>) {
        // 通知所有活跃命令
        let mut i = 0;
        while i < self.active_commands.len() {
            let keep = self.active_commands[i].on_event_boxed(&event).await;
            if keep {
                i += 1;
            } else {
                self.active_commands.swap_remove(i);
            }
        }

        // 转换并发送对外事件
        if let Some(node_event) = self.convert_to_node_event(&event) {
            let _ = self.event_tx.send(node_event).await;
        }

        trace!("Swarm event: {:?}", event);
    }

    /// 将 swarm 事件转换为对外事件
    fn convert_to_node_event(
        &mut self,
        event: &SwarmEvent<CoreBehaviourEvent>,
    ) -> Option<NodeEvent> {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => Some(NodeEvent::Listening {
                addr: address.clone(),
            }),
            // 只在第一个连接建立时通知（peer 级别聚合）
            SwarmEvent::ConnectionEstablished {
                peer_id,
                num_established,
                ..
            } if num_established.get() == 1 => Some(NodeEvent::PeerConnected { peer_id: *peer_id }),
            SwarmEvent::ConnectionEstablished { .. } => None,
            // 只在最后一个连接关闭时通知（peer 级别聚合）
            SwarmEvent::ConnectionClosed {
                peer_id,
                num_established,
                ..
            } if *num_established == 0 => Some(NodeEvent::PeerDisconnected { peer_id: *peer_id }),
            SwarmEvent::ConnectionClosed { .. } => None,
            SwarmEvent::Behaviour(CoreBehaviourEvent::Mdns(libp2p::mdns::Event::Discovered(
                peers,
            ))) => {
                for (peer_id, _addr) in peers {
                    // 避免重复连接
                    if !self.swarm.is_connected(peer_id) {
                        if let Err(e) = self.swarm.dial(*peer_id) {
                            warn!("Failed to dial discovered peer {}: {}", peer_id, e);
                        }
                    }
                }
                Some(NodeEvent::PeersDiscovered {
                    peers: peers.iter().map(|(p, a)| (*p, a.clone())).collect(),
                })
            }
            SwarmEvent::Behaviour(CoreBehaviourEvent::Ping(ping::Event {
                peer,
                result: Ok(rtt),
                ..
            })) => Some(NodeEvent::PingSuccess {
                peer_id: *peer,
                rtt_ms: rtt.as_millis() as u64,
            }),
            SwarmEvent::Behaviour(CoreBehaviourEvent::Ping(_)) => None,
            SwarmEvent::Behaviour(CoreBehaviourEvent::Identify(
                libp2p::identify::Event::Received { peer_id, info, .. },
            )) => {
                // 如果协议版本匹配，自动加入 Kad
                if info.protocol_version == self.protocol_version {
                    for addr in &info.listen_addrs {
                        self.swarm
                            .behaviour_mut()
                            .kad
                            .add_address(peer_id, addr.clone());
                    }
                    info!("Added peer {} to Kad (protocol: {})", peer_id, info.protocol_version);
                }
                Some(NodeEvent::IdentifyReceived {
                    peer_id: *peer_id,
                    agent_version: info.agent_version.clone(),
                    protocol_version: info.protocol_version.clone(),
                })
            }
            SwarmEvent::Behaviour(CoreBehaviourEvent::Kad(e))=>{
                None
            }
            SwarmEvent::Behaviour(CoreBehaviourEvent::Autonat(autonat::Event::StatusChanged {
                new,
                ..
            })) => {
                let (status, public_addr) = match new {
                    autonat::NatStatus::Public(addr) => (NatStatus::Public, Some(addr.clone())),
                    autonat::NatStatus::Private => (NatStatus::Private, None),
                    autonat::NatStatus::Unknown => (NatStatus::Unknown, None),
                };
                Some(NodeEvent::NatStatusChanged { status, public_addr })
            }
            _ => None,
        }
    }
}
