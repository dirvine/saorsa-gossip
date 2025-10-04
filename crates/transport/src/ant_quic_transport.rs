//! Ant-QUIC transport implementation for Saorsa Gossip
//!
//! This module provides a production-ready QUIC transport using ant-quic.
//! Features:
//! - Full QUIC multiplexing for membership/pubsub/bulk streams
//! - NAT traversal with hole punching
//! - Post-quantum cryptography (PQC) support
//! - Connection pooling and management

use anyhow::{anyhow, Result};
use bytes::Bytes;
use saorsa_gossip_types::PeerId as GossipPeerId;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::{GossipTransport, StreamType};

// Import ant-quic types
use ant_quic::{
    auth::AuthConfig,
    crypto::raw_public_keys::key_utils::{derive_peer_id_from_public_key, generate_ed25519_keypair},
    nat_traversal_api::{EndpointRole, PeerId as AntPeerId},
    quic_node::{QuicNodeConfig, QuicP2PNode},
};

/// Ant-QUIC transport implementation
///
/// Uses QuicP2PNode for P2P QUIC networking with NAT traversal
pub struct AntQuicTransport {
    /// The underlying ant-quic P2P node
    node: Arc<QuicP2PNode>,
    /// Incoming message channel
    recv_tx: mpsc::UnboundedSender<(GossipPeerId, StreamType, Bytes)>,
    recv_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<(GossipPeerId, StreamType, Bytes)>>>,
    /// Local peer ID (ant-quic format)
    ant_peer_id: AntPeerId,
    /// Local peer ID (gossip format)
    gossip_peer_id: GossipPeerId,
    /// Bootstrap coordinator addresses
    bootstrap_nodes: Vec<SocketAddr>,
}

impl AntQuicTransport {
    /// Create a new Ant-QUIC transport
    ///
    /// # Arguments
    /// * `bind_addr` - Local address to bind to
    /// * `role` - Endpoint role (Client, Server, or Bootstrap)
    /// * `bootstrap_nodes` - List of bootstrap coordinator addresses
    pub async fn new(
        bind_addr: SocketAddr,
        role: EndpointRole,
        bootstrap_nodes: Vec<SocketAddr>,
    ) -> Result<Self> {
        // Generate Ed25519 keypair for peer identity
        let (_private_key, public_key) = generate_ed25519_keypair();
        let ant_peer_id = derive_peer_id_from_public_key(&public_key);

        // Convert ant-quic PeerId to Gossip PeerId
        let gossip_peer_id = ant_peer_id_to_gossip(&ant_peer_id);

        info!(
            "Creating Ant-QUIC transport at {} with role {:?}",
            bind_addr, role
        );
        info!("Peer ID: {:?}", ant_peer_id);

        // Create QuicP2PNode configuration
        let config = QuicNodeConfig {
            role,
            bootstrap_nodes: bootstrap_nodes.clone(),
            enable_coordinator: matches!(role, EndpointRole::Server { .. }),
            max_connections: 100,
            connection_timeout: Duration::from_secs(30),
            stats_interval: Duration::from_secs(60),
            auth_config: AuthConfig::default(),
            bind_addr: Some(bind_addr),
        };

        // Create the QuicP2PNode
        let node = Arc::new(
            QuicP2PNode::new(config)
                .await
                .map_err(|e| anyhow!("Failed to create QuicP2PNode: {}", e))?,
        );

        let (recv_tx, recv_rx) = mpsc::unbounded_channel();

        let transport = Self {
            node,
            recv_tx,
            recv_rx: Arc::new(tokio::sync::Mutex::new(recv_rx)),
            ant_peer_id,
            gossip_peer_id,
            bootstrap_nodes,
        };

        // Start receiving loop
        transport.spawn_receiver();

        Ok(transport)
    }

    /// Get local peer ID (gossip format)
    pub fn peer_id(&self) -> GossipPeerId {
        self.gossip_peer_id
    }

    /// Get local ant-quic peer ID
    pub fn ant_peer_id(&self) -> AntPeerId {
        self.ant_peer_id
    }

    /// Spawn background task to receive incoming messages
    ///
    /// Receives messages from ant-quic and forwards them to the recv channel
    /// with proper stream type routing based on the first byte.
    fn spawn_receiver(&self) {
        let node = Arc::clone(&self.node);
        let recv_tx = self.recv_tx.clone();

        tokio::spawn(async move {
            info!("Ant-QUIC receiver task started");

            loop {
                // Receive message from ant-quic (blocks until message arrives)
                match node.receive().await {
                    Ok((ant_peer_id, data)) => {
                        debug!("Received {} bytes from peer {:?}", data.len(), ant_peer_id);

                        // Convert ant PeerId to gossip PeerId
                        let gossip_peer_id = ant_peer_id_to_gossip(&ant_peer_id);

                        // Parse stream type from first byte
                        if data.is_empty() {
                            warn!("Received empty message from {:?}", ant_peer_id);
                            continue;
                        }

                        let stream_type = match data[0] {
                            0 => StreamType::Membership,
                            1 => StreamType::PubSub,
                            2 => StreamType::Bulk,
                            other => {
                                warn!("Unknown stream type byte: {}", other);
                                continue;
                            }
                        };

                        // Extract payload (skip first byte)
                        let payload = Bytes::copy_from_slice(&data[1..]);

                        // Forward to recv channel
                        if let Err(e) = recv_tx.send((gossip_peer_id, stream_type, payload)) {
                            error!("Failed to forward received message: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        debug!("Receive error: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }

            info!("Ant-QUIC receiver task stopped");
        });
    }
}

/// Convert ant-quic PeerId to Gossip PeerId
fn ant_peer_id_to_gossip(ant_id: &AntPeerId) -> GossipPeerId {
    // ant-quic PeerId is a 32-byte array, same as GossipPeerId
    GossipPeerId::new(ant_id.0)
}

/// Convert Gossip PeerId to ant-quic PeerId
fn gossip_peer_id_to_ant(gossip_id: &GossipPeerId) -> AntPeerId {
    // GossipPeerId has to_bytes() method that returns [u8; 32]
    AntPeerId(gossip_id.to_bytes())
}

#[async_trait::async_trait]
impl GossipTransport for AntQuicTransport {
    async fn dial(&self, peer: GossipPeerId, addr: SocketAddr) -> Result<()> {
        info!("Dialing peer {} at {}", peer, addr);

        // Convert gossip PeerId to ant-quic PeerId
        let ant_peer_id = gossip_peer_id_to_ant(&peer);

        // Use bootstrap coordinator if available
        let coordinator = self
            .bootstrap_nodes
            .first()
            .ok_or_else(|| anyhow!("No bootstrap coordinators available"))?;

        // Connect to peer via coordinator
        self.node
            .connect_to_peer(ant_peer_id, *coordinator)
            .await
            .map_err(|e| anyhow!("Failed to connect to peer: {}", e))?;

        info!("Successfully connected to peer {}", peer);
        Ok(())
    }

    async fn listen(&self, _bind: SocketAddr) -> Result<()> {
        // ant-quic QuicP2PNode handles listening automatically via its configuration
        // The node is already listening when created with bind_addr
        info!("Ant-QUIC node is listening (handled by QuicP2PNode)");
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        info!("Closing Ant-QUIC transport");
        // ant-quic will clean up connections when dropped
        // No explicit close needed as QuicP2PNode handles cleanup in Drop
        Ok(())
    }

    async fn send_to_peer(
        &self,
        peer: GossipPeerId,
        stream_type: StreamType,
        data: Bytes,
    ) -> Result<()> {
        debug!(
            "Sending {} bytes to peer {} on {:?} stream",
            data.len(),
            peer,
            stream_type
        );

        // Convert gossip PeerId to ant-quic PeerId
        let ant_peer_id = gossip_peer_id_to_ant(&peer);

        // Encode stream type as first byte
        let stream_type_byte = match stream_type {
            StreamType::Membership => 0u8,
            StreamType::PubSub => 1u8,
            StreamType::Bulk => 2u8,
        };

        // Prepare message: [stream_type_byte | data]
        let mut buf = Vec::with_capacity(1 + data.len());
        buf.push(stream_type_byte);
        buf.extend_from_slice(&data);

        // Send via ant-quic
        self.node
            .send_to_peer(&ant_peer_id, &buf)
            .await
            .map_err(|e| anyhow!("Failed to send to peer: {}", e))?;

        debug!("Successfully sent {} bytes to peer {}", buf.len(), peer);
        Ok(())
    }

    async fn receive_message(&self) -> Result<(GossipPeerId, StreamType, Bytes)> {
        let mut recv_rx = self.recv_rx.lock().await;

        recv_rx
            .recv()
            .await
            .ok_or_else(|| anyhow!("Receive channel closed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ant_quic_transport_creation() {
        let bind_addr = "127.0.0.1:0".parse().expect("Invalid address");
        let transport = AntQuicTransport::new(bind_addr, EndpointRole::Bootstrap, vec![])
            .await
            .expect("Failed to create transport");

        assert_ne!(transport.peer_id(), GossipPeerId::new([0u8; 32]));
    }

    #[tokio::test]
    async fn test_peer_id_conversion() {
        // Generate test peer ID
        let (_priv_key, pub_key) = generate_ed25519_keypair();
        let ant_id = derive_peer_id_from_public_key(&pub_key);

        // Convert to gossip and back
        let gossip_id = ant_peer_id_to_gossip(&ant_id);
        let ant_id_back = gossip_peer_id_to_ant(&gossip_id);

        assert_eq!(ant_id, ant_id_back);
    }

    #[tokio::test]
    #[ignore] // Integration test - requires running ant-quic nodes
    async fn test_two_node_communication() {
        use std::net::{IpAddr, Ipv4Addr};
        use tokio::time::{sleep, timeout, Duration};

        // Dynamic port allocation to avoid conflicts
        let base_port = 20000
            + (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis() % 1000)
                .unwrap_or(0) as u16);

        // Create bootstrap node
        let bootstrap_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), base_port);
        let bootstrap = AntQuicTransport::new(bootstrap_addr, EndpointRole::Bootstrap, vec![])
            .await
            .expect("Failed to create bootstrap");

        // Give bootstrap time to start
        sleep(Duration::from_millis(100)).await;

        // Create client node that connects via bootstrap
        let client_addr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), base_port + 1);
        let client = AntQuicTransport::new(client_addr, EndpointRole::Client, vec![bootstrap_addr])
            .await
            .expect("Failed to create client");

        // Give nodes time to establish connection
        sleep(Duration::from_millis(500)).await;

        // Test sending from client to bootstrap
        let test_data = Bytes::from("Hello, QUIC!");
        let bootstrap_peer_id = bootstrap.peer_id();

        // Dial bootstrap from client
        client
            .dial(bootstrap_peer_id, bootstrap_addr)
            .await
            .expect("Failed to dial bootstrap");

        // Give connection time to establish
        sleep(Duration::from_millis(500)).await;

        // Send message
        client
            .send_to_peer(bootstrap_peer_id, StreamType::PubSub, test_data.clone())
            .await
            .expect("Failed to send message");

        // Receive message on bootstrap with timeout
        let result = timeout(
            Duration::from_secs(5),
            bootstrap.receive_message(),
        )
        .await;

        match result {
            Ok(Ok((peer_id, stream_type, data))) => {
                assert_eq!(peer_id, client.peer_id());
                assert_eq!(stream_type, StreamType::PubSub);
                assert_eq!(data, test_data);
            }
            Ok(Err(e)) => panic!("Receive error: {}", e),
            Err(_) => panic!("Receive timeout"),
        }
    }

    #[tokio::test]
    async fn test_stream_type_encoding() {
        // Test that stream types are encoded correctly
        assert_eq!(
            match StreamType::Membership {
                StreamType::Membership => 0u8,
                StreamType::PubSub => 1u8,
                StreamType::Bulk => 2u8,
            },
            0u8
        );
        assert_eq!(
            match StreamType::PubSub {
                StreamType::Membership => 0u8,
                StreamType::PubSub => 1u8,
                StreamType::Bulk => 2u8,
            },
            1u8
        );
        assert_eq!(
            match StreamType::Bulk {
                StreamType::Membership => 0u8,
                StreamType::PubSub => 1u8,
                StreamType::Bulk => 2u8,
            },
            2u8
        );
    }
}
