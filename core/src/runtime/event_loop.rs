use futures::StreamExt;
use libp2p::swarm::SwarmEvent;
use tokio::sync::mpsc;
use tracing::{trace, warn};

use super::CoreBehaviourEvent;
use crate::command::{Command, CoreSwarm};
use crate::event::NodeEvent;

/// 事件循环
pub struct EventLoop {
    swarm: CoreSwarm,
    command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<NodeEvent>,
    active_commands: Vec<Command>,
}

impl EventLoop {
    pub fn new(
        swarm: CoreSwarm,
        command_rx: mpsc::Receiver<Command>,
        event_tx: mpsc::Sender<NodeEvent>,
    ) -> Self {
        Self {
            swarm,
            command_rx,
            event_tx,
            active_commands: Vec::new(),
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
                            trace!("Command channel closed, shutting down");
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
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                Some(NodeEvent::PeerConnected { peer_id: *peer_id })
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                Some(NodeEvent::PeerDisconnected { peer_id: *peer_id })
            }
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
            SwarmEvent::Behaviour(CoreBehaviourEvent::Identify(
                libp2p::identify::Event::Received { peer_id, info, .. },
            )) => Some(NodeEvent::IdentifyReceived {
                peer_id: *peer_id,
                agent_version: info.agent_version.clone(),
                protocol_version: info.protocol_version.clone(),
            }),
            _ => None,
        }
    }
}
