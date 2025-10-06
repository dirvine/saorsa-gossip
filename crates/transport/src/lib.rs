//! QUIC transport adapter for Saorsa Gossip
//!
//! Provides QUIC transport with:
//! - Three control streams: `mship`, `pubsub`, `bulk`
//! - 0-RTT resumption where safe
//! - Path migration by default
//! - PQC handshake with ant-quic

mod ant_quic_transport;
mod peer_cache;

pub use ant_quic_transport::AntQuicTransport;
pub use peer_cache::{PeerCache, PeerCacheConfig, PeerCacheStats};

use anyhow::Result;
use saorsa_gossip_types::PeerId;
use std::net::SocketAddr;
use tokio::sync::mpsc;

/// Stream type identifiers for QUIC streams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    /// Membership stream for HyParView+SWIM
    Membership,
    /// Pub/sub stream for Plumtree control
    PubSub,
    /// Bulk stream for payloads and CRDT deltas
    Bulk,
}

/// QUIC transport trait for dial/listen operations
#[async_trait::async_trait]
pub trait GossipTransport: Send + Sync {
    /// Dial a peer and establish QUIC connection
    async fn dial(&self, peer: PeerId, addr: SocketAddr) -> Result<()>;

    /// Listen on a socket address for incoming connections
    async fn listen(&self, bind: SocketAddr) -> Result<()>;

    /// Close the transport
    async fn close(&self) -> Result<()>;

    /// Send data to a specific peer on a specific stream type
    async fn send_to_peer(
        &self,
        peer: PeerId,
        stream_type: StreamType,
        data: bytes::Bytes,
    ) -> Result<()>;

    /// Receive a message from any peer on any stream
    async fn receive_message(&self) -> Result<(PeerId, StreamType, bytes::Bytes)>;
}

/// Transport configuration
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Enable 0-RTT resumption
    pub enable_0rtt: bool,
    /// Enable path migration
    pub enable_migration: bool,
    /// Maximum idle timeout in seconds
    pub max_idle_timeout: u64,
    /// Keep-alive interval in seconds
    pub keep_alive_interval: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            enable_0rtt: true,
            enable_migration: true,
            max_idle_timeout: 30,
            keep_alive_interval: 10,
        }
    }
}

/// Mock QUIC transport implementation (placeholder for ant-quic)
pub struct QuicTransport {
    #[allow(dead_code)]
    config: TransportConfig,
    connection_tx: mpsc::UnboundedSender<(PeerId, SocketAddr)>,
    connection_rx: mpsc::UnboundedReceiver<(PeerId, SocketAddr)>,
    /// Channel for sending messages to peers
    send_tx: mpsc::UnboundedSender<(PeerId, StreamType, bytes::Bytes)>,
    #[allow(dead_code)]
    send_rx: mpsc::UnboundedReceiver<(PeerId, StreamType, bytes::Bytes)>,
    /// Channel for receiving messages from peers
    recv_tx: mpsc::UnboundedSender<(PeerId, StreamType, bytes::Bytes)>,
    #[allow(dead_code)]
    recv_rx: mpsc::UnboundedReceiver<(PeerId, StreamType, bytes::Bytes)>,
}

impl QuicTransport {
    /// Create a new QUIC transport with the given configuration
    pub fn new(config: TransportConfig) -> Self {
        let (connection_tx, connection_rx) = mpsc::unbounded_channel();
        let (send_tx, send_rx) = mpsc::unbounded_channel();
        let (recv_tx, recv_rx) = mpsc::unbounded_channel();
        Self {
            config,
            connection_tx,
            connection_rx,
            send_tx,
            send_rx,
            recv_tx,
            recv_rx,
        }
    }

    /// Get a receiver for incoming connections
    pub fn connection_receiver(&mut self) -> &mut mpsc::UnboundedReceiver<(PeerId, SocketAddr)> {
        &mut self.connection_rx
    }

    /// Get a sender for simulating received messages (for testing)
    pub fn get_recv_tx(&self) -> mpsc::UnboundedSender<(PeerId, StreamType, bytes::Bytes)> {
        self.recv_tx.clone()
    }
}

#[async_trait::async_trait]
impl GossipTransport for QuicTransport {
    async fn dial(&self, peer: PeerId, addr: SocketAddr) -> Result<()> {
        // Placeholder implementation - will integrate with ant-quic
        self.connection_tx
            .send((peer, addr))
            .map_err(|e| anyhow::anyhow!("Failed to send connection: {}", e))?;
        Ok(())
    }

    async fn listen(&self, _bind: SocketAddr) -> Result<()> {
        // Placeholder implementation - will integrate with ant-quic
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        // Placeholder implementation
        Ok(())
    }

    async fn send_to_peer(
        &self,
        peer: PeerId,
        stream_type: StreamType,
        data: bytes::Bytes,
    ) -> Result<()> {
        // Placeholder implementation - will integrate with ant-quic
        // In real implementation, this would open a QUIC stream to the peer
        self.send_tx
            .send((peer, stream_type, data))
            .map_err(|e| anyhow::anyhow!("Failed to send to peer: {}", e))?;
        Ok(())
    }

    async fn receive_message(&self) -> Result<(PeerId, StreamType, bytes::Bytes)> {
        // Placeholder implementation - will integrate with ant-quic
        // In real implementation, this would receive from QUIC streams
        self.recv_tx
            .send((
                PeerId::new([0u8; 32]),
                StreamType::PubSub,
                bytes::Bytes::new(),
            ))
            .ok();
        Err(anyhow::anyhow!("No messages available"))
    }
}

/// Stream multiplexer for QUIC streams
pub struct StreamMultiplexer {
    membership_tx: mpsc::UnboundedSender<bytes::Bytes>,
    pubsub_tx: mpsc::UnboundedSender<bytes::Bytes>,
    bulk_tx: mpsc::UnboundedSender<bytes::Bytes>,
}

impl StreamMultiplexer {
    /// Create a new stream multiplexer
    pub fn new() -> (Self, StreamReceivers) {
        let (membership_tx, membership_rx) = mpsc::unbounded_channel();
        let (pubsub_tx, pubsub_rx) = mpsc::unbounded_channel();
        let (bulk_tx, bulk_rx) = mpsc::unbounded_channel();

        let mux = Self {
            membership_tx,
            pubsub_tx,
            bulk_tx,
        };

        let receivers = StreamReceivers {
            membership_rx,
            pubsub_rx,
            bulk_rx,
        };

        (mux, receivers)
    }

    /// Send data on the specified stream type
    pub fn send(&self, stream_type: StreamType, data: bytes::Bytes) -> Result<()> {
        let tx = match stream_type {
            StreamType::Membership => &self.membership_tx,
            StreamType::PubSub => &self.pubsub_tx,
            StreamType::Bulk => &self.bulk_tx,
        };

        tx.send(data)
            .map_err(|e| anyhow::anyhow!("Failed to send on {:?} stream: {}", stream_type, e))
    }
}

impl Default for StreamMultiplexer {
    fn default() -> Self {
        Self::new().0
    }
}

/// Stream receivers for each stream type
pub struct StreamReceivers {
    /// Membership stream receiver
    pub membership_rx: mpsc::UnboundedReceiver<bytes::Bytes>,
    /// Pub/sub stream receiver
    pub pubsub_rx: mpsc::UnboundedReceiver<bytes::Bytes>,
    /// Bulk stream receiver
    pub bulk_rx: mpsc::UnboundedReceiver<bytes::Bytes>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_quic_transport_creation() {
        let config = TransportConfig::default();
        let _transport = QuicTransport::new(config);
    }

    #[tokio::test]
    async fn test_stream_multiplexer() {
        let (mux, mut receivers) = StreamMultiplexer::new();

        let test_data = bytes::Bytes::from("test");
        mux.send(StreamType::Membership, test_data.clone()).ok();

        let received = receivers.membership_rx.recv().await;
        assert!(received.is_some());
        assert_eq!(
            received.as_ref().map(|b| b.as_ref()),
            Some(test_data.as_ref())
        );
    }

    #[tokio::test]
    async fn test_transport_dial() {
        let config = TransportConfig::default();
        let transport = QuicTransport::new(config);

        let peer_id = PeerId::new([1u8; 32]);
        let addr = "127.0.0.1:8080".parse().ok();

        if let Some(addr) = addr {
            let result = transport.dial(peer_id, addr).await;
            assert!(result.is_ok());
        }
    }
}
