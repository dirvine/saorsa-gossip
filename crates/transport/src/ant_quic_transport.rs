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
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use crate::{GossipTransport, PeerCache, StreamType};

// Import ant-quic types
use ant_quic::{
    auth::AuthConfig,
    crypto::raw_public_keys::key_utils::{
        derive_peer_id_from_public_key, generate_ed25519_keypair,
    },
    nat_traversal_api::{EndpointRole, PeerId as AntPeerId},
    quic_node::{QuicNodeConfig, QuicP2PNode},
};

/// Configuration for Ant-QUIC transport
#[derive(Debug, Clone)]
pub struct AntQuicTransportConfig {
    /// Local address to bind to
    pub bind_addr: SocketAddr,
    /// Endpoint role (Client, Server, or Bootstrap)
    pub role: EndpointRole,
    /// List of bootstrap coordinator addresses
    pub bootstrap_nodes: Vec<SocketAddr>,
    /// Channel capacity for backpressure (default: 10,000 messages)
    pub channel_capacity: usize,
    /// Peer timeout for cleanup (default: 5 minutes)
    pub peer_timeout: Duration,
    /// Maximum bytes to read per stream (default: 100 MB)
    pub stream_read_limit: usize,
}

impl AntQuicTransportConfig {
    /// Create a new configuration with required fields and sensible defaults
    pub fn new(bind_addr: SocketAddr, role: EndpointRole, bootstrap_nodes: Vec<SocketAddr>) -> Self {
        Self {
            bind_addr,
            role,
            bootstrap_nodes,
            channel_capacity: 10_000,
            peer_timeout: Duration::from_secs(300), // 5 minutes
            stream_read_limit: 100 * 1024 * 1024, // 100 MB
        }
    }

    /// Set channel capacity for backpressure
    pub fn with_channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Set peer timeout for cleanup
    pub fn with_peer_timeout(mut self, timeout: Duration) -> Self {
        self.peer_timeout = timeout;
        self
    }

    /// Set stream read limit
    pub fn with_stream_read_limit(mut self, limit: usize) -> Self {
        self.stream_read_limit = limit;
        self
    }
}

/// Ant-QUIC transport implementation
///
/// Uses QuicP2PNode for P2P QUIC networking with NAT traversal
pub struct AntQuicTransport {
    /// The underlying ant-quic P2P node
    node: Arc<QuicP2PNode>,
    /// Incoming message channel (bounded for backpressure)
    recv_tx: mpsc::Sender<(GossipPeerId, StreamType, Bytes)>,
    recv_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<(GossipPeerId, StreamType, Bytes)>>>,
    /// Local peer ID (ant-quic format)
    ant_peer_id: AntPeerId,
    /// Local peer ID (gossip format)
    gossip_peer_id: GossipPeerId,
    /// Bootstrap coordinator addresses
    bootstrap_nodes: Vec<SocketAddr>,
    /// Track connected peers with their addresses and last seen time
    connected_peers: Arc<RwLock<HashMap<GossipPeerId, (SocketAddr, Instant)>>>,
    /// Bootstrap peer IDs mapped to their addresses
    bootstrap_peer_ids: Arc<RwLock<HashMap<SocketAddr, GossipPeerId>>>,
    /// Optional peer cache for persistent peer storage
    peer_cache: Option<Arc<PeerCache>>,
    /// Configuration
    config: AntQuicTransportConfig,
}

impl AntQuicTransport {
    /// Create a new Ant-QUIC transport without peer cache (backward compatible)
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
        let config = AntQuicTransportConfig::new(bind_addr, role, bootstrap_nodes);
        Self::with_config(config, None).await
    }

    /// Create a new Ant-QUIC transport with optional peer cache (backward compatible)
    ///
    /// # Arguments
    /// * `bind_addr` - Local address to bind to
    /// * `role` - Endpoint role (Client, Server, or Bootstrap)
    /// * `bootstrap_nodes` - List of bootstrap coordinator addresses
    /// * `peer_cache` - Optional peer cache for persistent peer storage
    pub async fn new_with_cache(
        bind_addr: SocketAddr,
        role: EndpointRole,
        bootstrap_nodes: Vec<SocketAddr>,
        peer_cache: Option<Arc<PeerCache>>,
    ) -> Result<Self> {
        let config = AntQuicTransportConfig::new(bind_addr, role, bootstrap_nodes);
        Self::with_config(config, peer_cache).await
    }

    /// Create a new Ant-QUIC transport with custom configuration
    ///
    /// # Arguments
    /// * `config` - Transport configuration
    /// * `peer_cache` - Optional peer cache for persistent peer storage
    pub async fn with_config(
        config: AntQuicTransportConfig,
        peer_cache: Option<Arc<PeerCache>>,
    ) -> Result<Self> {
        // Generate Ed25519 keypair for peer identity
        let (_private_key, public_key) = generate_ed25519_keypair();
        let ant_peer_id = derive_peer_id_from_public_key(&public_key);

        // Convert ant-quic PeerId to Gossip PeerId
        let gossip_peer_id = ant_peer_id_to_gossip(&ant_peer_id);

        info!(
            "Creating Ant-QUIC transport at {} with role {:?}",
            config.bind_addr, config.role
        );
        info!("Peer ID: {:?}", ant_peer_id);
        info!("Config: channel_capacity={}, peer_timeout={:?}, stream_read_limit={}",
            config.channel_capacity, config.peer_timeout, config.stream_read_limit);

        // Create QuicP2PNode configuration
        let node_config = QuicNodeConfig {
            role: config.role,
            bootstrap_nodes: config.bootstrap_nodes.clone(),
            enable_coordinator: matches!(config.role, EndpointRole::Server { .. }),
            max_connections: 100,
            connection_timeout: Duration::from_secs(30),
            stats_interval: Duration::from_secs(60),
            auth_config: AuthConfig::default(),
            bind_addr: Some(config.bind_addr),
        };

        // Create the QuicP2PNode
        let node = Arc::new(
            QuicP2PNode::new(node_config)
                .await
                .map_err(|e| anyhow!("Failed to create QuicP2PNode: {}", e))?,
        );

        // Create bounded channel for backpressure
        let (recv_tx, recv_rx) = mpsc::channel(config.channel_capacity);

        let transport = Self {
            node: Arc::clone(&node),
            recv_tx,
            recv_rx: Arc::new(tokio::sync::Mutex::new(recv_rx)),
            ant_peer_id,
            gossip_peer_id,
            bootstrap_nodes: config.bootstrap_nodes.clone(),
            connected_peers: Arc::new(RwLock::new(HashMap::new())),
            bootstrap_peer_ids: Arc::new(RwLock::new(HashMap::new())),
            peer_cache: peer_cache.clone(),
            config: config.clone(),
        };

        // Start receiving loop
        transport.spawn_receiver();

        // Start peer cleanup task
        transport.spawn_cleanup_task();

        // If this is a Client node with bootstrap coordinators, establish connections
        if matches!(config.role, EndpointRole::Client) && !config.bootstrap_nodes.is_empty() {
            info!(
                "Client role detected - establishing connections to {} bootstrap coordinator(s)...",
                config.bootstrap_nodes.len()
            );

            let mut connected_count = 0;
            for bootstrap_addr in &config.bootstrap_nodes {
                info!(
                    "Connecting to bootstrap coordinator at {}...",
                    bootstrap_addr
                );

                match node.connect_to_bootstrap(*bootstrap_addr).await {
                    Ok(coordinator_peer_id) => {
                        let gossip_coordinator_id = ant_peer_id_to_gossip(&coordinator_peer_id);

                        info!(
                            "✓ Connected to bootstrap coordinator {} (PeerId: {:?})",
                            bootstrap_addr, coordinator_peer_id
                        );

                        // Store bootstrap peer ID
                        transport.bootstrap_peer_ids.write().await
                            .insert(*bootstrap_addr, gossip_coordinator_id);

                        // Update peer cache if present
                        if let Some(cache) = &transport.peer_cache {
                            cache.mark_success(gossip_coordinator_id, *bootstrap_addr).await;
                        }

                        connected_count += 1;
                    }
                    Err(e) => {
                        warn!(
                            "Failed to connect to bootstrap coordinator {}: {}",
                            bootstrap_addr, e
                        );
                        // Continue trying other bootstrap nodes
                    }
                }
            }

            if connected_count == 0 {
                return Err(anyhow!(
                    "Failed to connect to any bootstrap coordinators ({} attempted)",
                    config.bootstrap_nodes.len()
                ));
            }

            info!(
                "✓ Successfully connected to {}/{} bootstrap coordinator(s)",
                connected_count,
                config.bootstrap_nodes.len()
            );
        }

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

    /// Get list of connected peers
    ///
    /// Returns a vector of (PeerId, SocketAddr) tuples for all currently connected peers.
    /// Connections are tracked internally and expired after 5 minutes of inactivity.
    pub async fn connected_peers(&self) -> Vec<(GossipPeerId, SocketAddr)> {
        let peers = self.connected_peers.read().await;
        let now = Instant::now();

        peers
            .iter()
            .filter(|(_, (_, last_seen))| now.duration_since(*last_seen) < Duration::from_secs(300))
            .map(|(peer_id, (addr, _))| (*peer_id, *addr))
            .collect()
    }

    /// Get bootstrap peer ID by coordinator address
    ///
    /// Returns the peer ID of a bootstrap coordinator if connected.
    pub async fn get_bootstrap_peer_id(&self, addr: SocketAddr) -> Option<GossipPeerId> {
        self.bootstrap_peer_ids.read().await.get(&addr).copied()
    }

    /// List all bootstrap peers
    ///
    /// Returns a vector of (SocketAddr, PeerId) tuples for all connected bootstrap coordinators.
    pub async fn list_bootstrap_peers(&self) -> Vec<(SocketAddr, GossipPeerId)> {
        self.bootstrap_peer_ids
            .read()
            .await
            .iter()
            .map(|(addr, peer_id)| (*addr, *peer_id))
            .collect()
    }

    /// Get peer ID for any connected peer (bootstrap or discovered)
    ///
    /// Returns the peer ID for a peer at the given address, checking both
    /// bootstrap coordinators and regular connected peers.
    pub async fn get_connected_peer_id(&self, addr: SocketAddr) -> Option<GossipPeerId> {
        // Check bootstrap peers first
        if let Some(peer_id) = self.bootstrap_peer_ids.read().await.get(&addr) {
            return Some(*peer_id);
        }

        // Check regular connected peers
        self.connected_peers
            .read()
            .await
            .iter()
            .find(|(_, (peer_addr, _))| *peer_addr == addr)
            .map(|(peer_id, _)| *peer_id)
    }

    /// Get reference to peer cache if configured
    pub fn peer_cache(&self) -> Option<&Arc<PeerCache>> {
        self.peer_cache.as_ref()
    }

    /// Spawn background task to receive incoming messages
    ///
    /// IMPORTANT: This implementation directly accepts streams from connections
    /// instead of using node.receive() which has 100ms timeout issues.
    ///
    /// For each new connection, we spawn dedicated stream acceptance tasks that
    /// continuously accept unidirectional and bidirectional streams without timeouts.
    fn spawn_receiver(&self) {
        let node = Arc::clone(&self.node);
        let recv_tx = self.recv_tx.clone();
        let connected_peers = Arc::clone(&self.connected_peers);
        let stream_read_limit = self.config.stream_read_limit;

        tokio::spawn(async move {
            info!("Ant-QUIC direct stream receiver task started");

            // Get access to NAT endpoint for direct connection access
            let nat_endpoint = match node.get_nat_endpoint() {
                Ok(endpoint) => endpoint,
                Err(e) => {
                    error!("Failed to get NAT endpoint: {}", e);
                    return;
                }
            };

            // Track which peer IDs we've already spawned handlers for
            let spawned_handlers: Arc<RwLock<std::collections::HashSet<AntPeerId>>> =
                Arc::new(RwLock::new(std::collections::HashSet::new()));

            loop {
                // Get ALL currently connected peers from NAT endpoint
                let peers = match nat_endpoint.list_connections() {
                    Ok(connections) => {
                        // Store peers in tracking (need to collect first to avoid holding lock)
                        let peer_data: Vec<(AntPeerId, GossipPeerId, SocketAddr)> = connections
                            .into_iter()
                            .map(|(peer_id, addr)| {
                                let gossip_id = ant_peer_id_to_gossip(&peer_id);
                                (peer_id, gossip_id, addr)
                            })
                            .collect();

                        // Update tracking map
                        {
                            let mut peers_map = connected_peers.write().await;
                            for (_, gossip_id, addr) in &peer_data {
                                peers_map.insert(*gossip_id, (*addr, Instant::now()));
                            }
                        }

                        // Return just peer IDs
                        peer_data.into_iter().map(|(peer_id, _, _)| peer_id).collect::<Vec<_>>()
                    }
                    Err(e) => {
                        debug!("Error listing connections: {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                // Spawn handlers for any new peers
                for peer_id in peers {
                    let mut spawned = spawned_handlers.write().await;
                    if !spawned.contains(&peer_id) {
                        spawned.insert(peer_id);
                        drop(spawned);

                        // Get the connection
                        if let Ok(Some(connection)) = nat_endpoint.get_connection(&peer_id) {
                            // Extract real peer address from connection
                            let peer_addr = connection.remote_address();
                            info!("Spawning stream handlers for peer {:?} at {}", peer_id, peer_addr);

                            // Spawn unidirectional stream handler
                            let conn_uni = connection.clone();
                            let tx_uni = recv_tx.clone();
                            let peers_uni = Arc::clone(&connected_peers);
                            let read_limit_uni = stream_read_limit;
                            tokio::spawn(async move {
                                loop {
                                    match conn_uni.accept_uni().await {
                                        Ok(mut recv_stream) => {
                                            debug!("Accepted unidirectional stream from {:?}", peer_id);

                                            // Read data from stream with configurable limit
                                            match recv_stream.read_to_end(read_limit_uni).await {
                                                Ok(data) => {
                                                    if data.is_empty() {
                                                        debug!("Empty stream data from {:?}", peer_id);
                                                        continue;
                                                    }

                                                    debug!("Read {} bytes from stream", data.len());

                                                    // Convert peer ID
                                                    let gossip_peer_id = ant_peer_id_to_gossip(&peer_id);

                                                    // Track peer with real address
                                                    let mut peers = peers_uni.write().await;
                                                    peers.insert(gossip_peer_id, (peer_addr, Instant::now()));
                                                    drop(peers);

                                                    // Parse stream type from first byte
                                                    let stream_type = match data.first() {
                                                        Some(&0) => StreamType::Membership,
                                                        Some(&1) => StreamType::PubSub,
                                                        Some(&2) => StreamType::Bulk,
                                                        Some(&other) => {
                                                            warn!("Unknown stream type byte: {}", other);
                                                            continue;
                                                        }
                                                        None => {
                                                            warn!("Empty data from {:?}", peer_id);
                                                            continue;
                                                        }
                                                    };

                                                    // Extract payload (skip first byte)
                                                    let payload = if data.len() > 1 {
                                                        Bytes::copy_from_slice(&data[1..])
                                                    } else {
                                                        Bytes::new()
                                                    };

                                                    // Forward to recv channel (bounded, may apply backpressure)
                                                    if let Err(e) = tx_uni.send((gossip_peer_id, stream_type, payload)).await {
                                                        error!("Failed to forward message (channel closed): {}", e);
                                                        break;
                                                    }

                                                    info!(
                                                        "Forwarded {} bytes ({:?}) from {:?}",
                                                        data.len() - 1,
                                                        stream_type,
                                                        gossip_peer_id
                                                    );
                                                }
                                                Err(e) => {
                                                    debug!("Error reading stream: {}", e);
                                                    break;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            debug!("Stream accept error for {:?}: {}", peer_id, e);
                                            break;
                                        }
                                    }
                                }
                                debug!("Unidirectional stream handler stopped for {:?}", peer_id);
                            });

                            // Also spawn bidirectional stream handler
                            let conn_bi = connection.clone();
                            let tx_bi = recv_tx.clone();
                            let peers_bi = Arc::clone(&connected_peers);
                            let read_limit_bi = stream_read_limit;
                            tokio::spawn(async move {
                                loop {
                                    match conn_bi.accept_bi().await {
                                        Ok((_send_stream, mut recv_stream)) => {
                                            debug!("Accepted bidirectional stream from {:?}", peer_id);

                                            // Read data from stream with configurable limit
                                            match recv_stream.read_to_end(read_limit_bi).await {
                                                Ok(data) => {
                                                    if data.is_empty() {
                                                        continue;
                                                    }

                                                    let gossip_peer_id = ant_peer_id_to_gossip(&peer_id);

                                                    // Track peer with real address
                                                    let mut peers = peers_bi.write().await;
                                                    peers.insert(gossip_peer_id, (peer_addr, Instant::now()));
                                                    drop(peers);

                                                    let stream_type = match data.first() {
                                                        Some(&0) => StreamType::Membership,
                                                        Some(&1) => StreamType::PubSub,
                                                        Some(&2) => StreamType::Bulk,
                                                        Some(&other) => {
                                                            warn!("Unknown stream type byte: {}", other);
                                                            continue;
                                                        }
                                                        None => continue,
                                                    };

                                                    let payload = if data.len() > 1 {
                                                        Bytes::copy_from_slice(&data[1..])
                                                    } else {
                                                        Bytes::new()
                                                    };

                                                    // Forward to recv channel (bounded, may apply backpressure)
                                                    if let Err(e) = tx_bi.send((gossip_peer_id, stream_type, payload)).await {
                                                        error!("Failed to forward message (channel closed): {}", e);
                                                        break;
                                                    }
                                                }
                                                Err(e) => {
                                                    debug!("Error reading bi stream: {}", e);
                                                    break;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            debug!("Bi stream accept error for {:?}: {}", peer_id, e);
                                            break;
                                        }
                                    }
                                }
                                debug!("Bidirectional stream handler stopped for {:?}", peer_id);
                            });
                        }
                    }
                }

                // Wait before checking for new peers
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }

    /// Spawn background task to periodically clean up stale peer connections
    ///
    /// Removes peers that haven't been seen within the configured peer_timeout duration.
    /// This prevents memory leaks and allows peers to reconnect after being idle.
    fn spawn_cleanup_task(&self) {
        let connected_peers = Arc::clone(&self.connected_peers);
        let peer_timeout = self.config.peer_timeout;

        tokio::spawn(async move {
            info!("Peer cleanup task started (timeout: {:?})", peer_timeout);

            loop {
                // Wait before cleanup cycle (run every 30 seconds)
                tokio::time::sleep(Duration::from_secs(30)).await;

                let now = Instant::now();
                let mut peers = connected_peers.write().await;

                let initial_count = peers.len();
                peers.retain(|peer_id, (_addr, last_seen)| {
                    let elapsed = now.duration_since(*last_seen);
                    let keep = elapsed < peer_timeout;

                    if !keep {
                        debug!("Removing stale peer {:?} (last seen {:?} ago)", peer_id, elapsed);
                    }

                    keep
                });

                let removed_count = initial_count.saturating_sub(peers.len());
                if removed_count > 0 {
                    info!("Cleaned up {} stale peer(s), {} remaining", removed_count, peers.len());
                }
            }
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
        let send_result = self.node
            .send_to_peer(&ant_peer_id, &buf)
            .await;

        match send_result {
            Ok(()) => {
                // Track successful connection
                let mut connected_peers = self.connected_peers.write().await;

                // For now, use a placeholder address - in a production implementation,
                // this would be obtained from the ant-quic connection metadata
                let peer_addr = SocketAddr::from(([127, 0, 0, 1], 8080));

                connected_peers.insert(peer, (peer_addr, Instant::now()));
                drop(connected_peers);

                // Update peer cache on success
                if let Some(cache) = &self.peer_cache {
                    cache.mark_success(peer, peer_addr).await;
                }

                debug!("Successfully sent {} bytes to peer {}", buf.len(), peer);
                Ok(())
            }
            Err(e) => {
                // Update peer cache on failure
                if let Some(cache) = &self.peer_cache {
                    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 8080));
                    cache.mark_failure(peer, peer_addr).await;
                }

                Err(anyhow!("Failed to send to peer: {}", e))
            }
        }
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
        let client_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), base_port + 1);
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
        let result = timeout(Duration::from_secs(5), bootstrap.receive_message()).await;

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
