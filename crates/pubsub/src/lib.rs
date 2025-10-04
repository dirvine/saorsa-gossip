//! Plumtree-based pub/sub dissemination
//!
//! Implements:
//! - EAGER push along spanning tree
//! - IHAVE lazy digests to non-tree links
//! - IWANT pull on demand
//! - PRUNE/GRAFT for tree optimization
//! - Anti-entropy reconciliation (placeholder for future)
//!
//! # Architecture
//!
//! Each topic maintains two sets of peers:
//! - **Eager peers** (tree): Forward full messages immediately
//! - **Lazy peers** (gossip): Send only message IDs (IHAVE)
//!
//! The tree self-optimizes via duplicate detection (PRUNE) and pull requests (GRAFT).

use anyhow::{anyhow, Result};
use bytes::Bytes;
use lru::LruCache;
use saorsa_gossip_transport::{GossipTransport, StreamType};
use saorsa_gossip_types::{MessageHeader, MessageKind, PeerId, TopicId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::time;
use tracing::{debug, trace, warn};

/// Maximum message cache size per topic (10,000 messages)
const MAX_CACHE_SIZE: usize = 10_000;

/// Message cache TTL (5 minutes)
const CACHE_TTL_SECS: u64 = 300;

/// Maximum IHAVE batch size (per SPEC.md)
const MAX_IHAVE_BATCH_SIZE: usize = 1024;

/// IHAVE flush interval (100ms)
const IHAVE_FLUSH_INTERVAL_MS: u64 = 100;

/// Target eager peer degree (6-8)
const MIN_EAGER_DEGREE: usize = 6;
const MAX_EAGER_DEGREE: usize = 12;

/// IWANT timeout (2 seconds) - TODO: implement timeout tracking
#[allow(dead_code)]
const IWANT_TIMEOUT_SECS: u64 = 2;

/// Message ID type alias
type MessageIdType = [u8; 32];

/// Gossip message wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GossipMessage {
    /// Message header
    pub header: MessageHeader,
    /// Optional payload (None for IHAVE)
    pub payload: Option<Bytes>,
    /// Signature (ML-DSA - placeholder for now)
    pub signature: Vec<u8>,
}

/// Cached message entry
#[derive(Clone)]
struct CachedMessage {
    /// Message payload
    payload: Bytes,
    /// Timestamp when cached
    timestamp: Instant,
    /// Message header
    header: MessageHeader,
}

/// Per-topic state
struct TopicState {
    /// Spanning tree peers (forward EAGER)
    eager_peers: HashSet<PeerId>,
    /// Non-tree peers (send IHAVE only)
    lazy_peers: HashSet<PeerId>,
    /// Message cache: msg_id -> cached message
    message_cache: LruCache<MessageIdType, CachedMessage>,
    /// Pending IHAVE batch (≤1024 message IDs)
    pending_ihave: Vec<MessageIdType>,
    /// Outstanding IWANT requests: msg_id -> (peer, timestamp)
    outstanding_iwants: HashMap<MessageIdType, (PeerId, Instant)>,
    /// Local subscribers
    subscribers: Vec<mpsc::UnboundedSender<(PeerId, Bytes)>>,
}

impl TopicState {
    fn new() -> Self {
        Self {
            eager_peers: HashSet::new(),
            lazy_peers: HashSet::new(),
            message_cache: LruCache::new(
                NonZeroUsize::new(MAX_CACHE_SIZE).expect("MAX_CACHE_SIZE is non-zero"),
            ),
            pending_ihave: Vec::new(),
            outstanding_iwants: HashMap::new(),
            subscribers: Vec::new(),
        }
    }

    /// Check if message is in cache
    fn has_message(&self, msg_id: &MessageIdType) -> bool {
        self.message_cache.contains(msg_id)
    }

    /// Add message to cache
    fn cache_message(&mut self, msg_id: MessageIdType, payload: Bytes, header: MessageHeader) {
        let cached = CachedMessage {
            payload,
            timestamp: Instant::now(),
            header,
        };
        self.message_cache.put(msg_id, cached);
    }

    /// Get cached message
    fn get_message(&mut self, msg_id: &MessageIdType) -> Option<CachedMessage> {
        self.message_cache.get(msg_id).cloned()
    }

    /// Clean expired cache entries
    fn clean_cache(&mut self) {
        let now = Instant::now();
        let ttl = Duration::from_secs(CACHE_TTL_SECS);

        // Collect expired keys
        let mut expired = Vec::new();
        for (msg_id, cached) in self.message_cache.iter() {
            if now.duration_since(cached.timestamp) > ttl {
                expired.push(*msg_id);
            }
        }

        // Remove expired entries
        for msg_id in expired {
            self.message_cache.pop(&msg_id);
        }
    }

    /// Move peer from eager to lazy
    fn prune_peer(&mut self, peer: PeerId) {
        if self.eager_peers.remove(&peer) {
            self.lazy_peers.insert(peer);
            debug!(peer_id = %peer, "PRUNE: moved peer from eager to lazy");
        }
    }

    /// Move peer from lazy to eager
    fn graft_peer(&mut self, peer: PeerId) {
        if self.lazy_peers.remove(&peer) {
            self.eager_peers.insert(peer);
            debug!(peer_id = %peer, "GRAFT: moved peer from lazy to eager");
        }
    }

    /// Maintain eager peer degree (6-12)
    fn maintain_degree(&mut self) {
        let eager_count = self.eager_peers.len();

        if eager_count < MIN_EAGER_DEGREE && !self.lazy_peers.is_empty() {
            // Promote random lazy peers
            let to_promote = MIN_EAGER_DEGREE - eager_count;
            let peers: Vec<PeerId> = self.lazy_peers.iter().take(to_promote).copied().collect();
            for peer in peers {
                self.graft_peer(peer);
            }
        } else if eager_count > MAX_EAGER_DEGREE {
            // Demote random eager peers
            let to_demote = eager_count - MAX_EAGER_DEGREE;
            let peers: Vec<PeerId> = self.eager_peers.iter().take(to_demote).copied().collect();
            for peer in peers {
                self.prune_peer(peer);
            }
        }
    }
}

/// Pub/sub trait for message dissemination
#[async_trait::async_trait]
pub trait PubSub: Send + Sync {
    /// Publish a message to a topic
    async fn publish(&self, topic: TopicId, data: Bytes) -> Result<()>;

    /// Subscribe to a topic and receive messages
    fn subscribe(&self, topic: TopicId) -> mpsc::UnboundedReceiver<(PeerId, Bytes)>;

    /// Unsubscribe from a topic
    async fn unsubscribe(&self, topic: TopicId) -> Result<()>;
}

/// Plumtree pub/sub implementation
pub struct PlumtreePubSub<T: GossipTransport + 'static> {
    /// Per-topic state
    topics: Arc<RwLock<HashMap<TopicId, TopicState>>>,
    /// Local peer ID
    peer_id: PeerId,
    /// Epoch for message IDs (system time in seconds)
    epoch_start: std::time::SystemTime,
    /// Transport layer for sending messages
    transport: Arc<T>,
}

impl<T: GossipTransport + 'static> PlumtreePubSub<T> {
    /// Create a new Plumtree pub/sub instance
    pub fn new(peer_id: PeerId, transport: Arc<T>) -> Self {
        let pubsub = Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            peer_id,
            epoch_start: std::time::SystemTime::UNIX_EPOCH,
            transport,
        };

        // Start background tasks
        pubsub.spawn_ihave_flusher();
        pubsub.spawn_cache_cleaner();
        pubsub.spawn_degree_maintainer();

        pubsub
    }

    /// Get current epoch (seconds since UNIX_EPOCH)
    fn current_epoch(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(self.epoch_start)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Calculate message ID
    fn calculate_msg_id(&self, topic: &TopicId, payload: &Bytes) -> MessageIdType {
        let epoch = self.current_epoch();
        let payload_hash = blake3::hash(payload.as_ref());
        MessageHeader::calculate_msg_id(topic, epoch, &self.peer_id, payload_hash.as_bytes())
    }

    /// Create placeholder signature (TODO: integrate ML-DSA)
    fn sign_message(&self, _header: &MessageHeader) -> Vec<u8> {
        // Placeholder: return empty signature
        // TODO: Integrate saorsa-pqc ML-DSA signing
        Vec::new()
    }

    /// Verify placeholder signature (TODO: integrate ML-DSA)
    fn verify_signature(&self, _header: &MessageHeader, _signature: &[u8]) -> bool {
        // Placeholder: always return true
        // TODO: Integrate saorsa-pqc ML-DSA verification
        true
    }

    /// Publish a message (local origin)
    pub async fn publish_local(&self, topic: TopicId, payload: Bytes) -> Result<()> {
        let msg_id = self.calculate_msg_id(&topic, &payload);

        let header = MessageHeader {
            version: 1,
            topic,
            msg_id,
            kind: MessageKind::Eager,
            hop: 0,
            ttl: 10,
        };

        let signature = self.sign_message(&header);

        let _message = GossipMessage {
            header: header.clone(),
            payload: Some(payload.clone()),
            signature,
        };

        let mut topics = self.topics.write().await;
        let state = topics.entry(topic).or_insert_with(TopicState::new);

        // Add to cache
        state.cache_message(msg_id, payload.clone(), header);

        // Send EAGER to eager_peers
        let eager_peers: Vec<PeerId> = state.eager_peers.iter().copied().collect();
        drop(topics); // Release lock before network I/O

        for peer in eager_peers {
            trace!(peer_id = %peer, msg_id = ?msg_id, "Sending EAGER");
            let bytes = bincode::serialize(&_message)
                .map_err(|e| anyhow!("Serialization failed: {}", e))?;
            self.transport
                .send_to_peer(peer, StreamType::PubSub, bytes.into())
                .await?;
        }

        // Batch msg_id to pending_ihave
        let mut topics = self.topics.write().await;
        if let Some(state) = topics.get_mut(&topic) {
            state.pending_ihave.push(msg_id);

            // Deliver to local subscribers
            let data = (self.peer_id, payload);
            state.subscribers.retain(|tx| tx.send(data.clone()).is_ok());
        }

        Ok(())
    }

    /// Handle incoming EAGER message
    pub async fn handle_eager(
        &self,
        from: PeerId,
        topic: TopicId,
        message: GossipMessage,
    ) -> Result<()> {
        let msg_id = message.header.msg_id;

        // Verify signature
        if !self.verify_signature(&message.header, &message.signature) {
            warn!(peer_id = %from, msg_id = ?msg_id, "Invalid signature, dropping");
            return Err(anyhow!("Invalid signature"));
        }

        let mut topics = self.topics.write().await;
        let state = topics.entry(topic).or_insert_with(TopicState::new);

        // Check for duplicate
        if state.has_message(&msg_id) {
            // PRUNE: move sender from eager to lazy
            state.prune_peer(from);
            return Ok(());
        }

        // New message - add to cache
        let payload = message.payload.clone().ok_or_else(|| anyhow!("EAGER missing payload"))?;
        state.cache_message(msg_id, payload.clone(), message.header.clone());

        // Deliver to local subscribers
        let data = (from, payload.clone());
        state.subscribers.retain(|tx| tx.send(data.clone()).is_ok());

        // Forward to eager_peers (except sender)
        let eager_peers: Vec<PeerId> = state
            .eager_peers
            .iter()
            .filter(|&&p| p != from)
            .copied()
            .collect();

        // Batch msg_id to pending_ihave for lazy_peers
        state.pending_ihave.push(msg_id);

        drop(topics); // Release lock

        // Forward EAGER
        for peer in eager_peers {
            trace!(peer_id = %peer, msg_id = ?msg_id, "Forwarding EAGER");
            let bytes = bincode::serialize(&message)
                .map_err(|e| anyhow!("Serialization failed: {}", e))?;
            self.transport
                .send_to_peer(peer, StreamType::PubSub, bytes.into())
                .await?;
        }

        Ok(())
    }

    /// Handle incoming IHAVE message
    pub async fn handle_ihave(&self, from: PeerId, topic: TopicId, msg_ids: Vec<MessageIdType>) -> Result<()> {
        let mut topics = self.topics.write().await;
        let state = topics.entry(topic).or_insert_with(TopicState::new);

        let mut requested = Vec::new();

        for msg_id in msg_ids {
            // Skip if we have it
            if state.has_message(&msg_id) {
                continue;
            }

            // Skip if already requested
            if state.outstanding_iwants.contains_key(&msg_id) {
                continue;
            }

            // Request it
            requested.push(msg_id);
            state.outstanding_iwants.insert(msg_id, (from, Instant::now()));
        }

        drop(topics); // Release lock

        if !requested.is_empty() {
            debug!(peer_id = %from, count = requested.len(), "Sending IWANT");
            // Create IWANT message
            let iwant_header = MessageHeader {
                version: 1,
                topic,
                msg_id: requested[0], // Use first ID as header
                kind: MessageKind::IWant,
                hop: 0,
                ttl: 10,
            };
            let iwant_msg = GossipMessage {
                header: iwant_header,
                payload: Some(bincode::serialize(&requested).map_err(|e| anyhow!("Serialization failed: {}", e))?.into()),
                signature: vec![], // TODO: Sign
            };
            let bytes = bincode::serialize(&iwant_msg)
                .map_err(|e| anyhow!("Serialization failed: {}", e))?;
            self.transport
                .send_to_peer(from, StreamType::PubSub, bytes.into())
                .await?;
        }

        Ok(())
    }

    /// Handle incoming IWANT message
    pub async fn handle_iwant(&self, from: PeerId, topic: TopicId, msg_ids: Vec<MessageIdType>) -> Result<()> {
        let mut topics = self.topics.write().await;
        let state = topics.entry(topic).or_insert_with(TopicState::new);

        let mut to_send = Vec::new();

        for msg_id in msg_ids {
            if let Some(cached) = state.get_message(&msg_id) {
                to_send.push((msg_id, cached));
                // GRAFT: move peer from lazy to eager
                state.graft_peer(from);
            } else {
                warn!(msg_id = ?msg_id, "IWANT for unknown message");
            }
        }

        drop(topics); // Release lock

        // Send EAGER with payloads
        for (msg_id, cached) in to_send {
            debug!(peer_id = %from, msg_id = ?msg_id, "Sending EAGER in response to IWANT");

            let _message = GossipMessage {
                header: cached.header.clone(),
                payload: Some(cached.payload.clone()),
                signature: self.sign_message(&cached.header),
            };

            let bytes = bincode::serialize(&_message)
                .map_err(|e| anyhow!("Serialization failed: {}", e))?;
            self.transport
                .send_to_peer(from, StreamType::PubSub, bytes.into())
                .await?;
        }

        Ok(())
    }

    /// Spawn background task to flush IHAVE batches
    fn spawn_ihave_flusher(&self) {
        let topics = self.topics.clone();
        let transport = self.transport.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(IHAVE_FLUSH_INTERVAL_MS));

            loop {
                interval.tick().await;

                let mut topics_guard = topics.write().await;

                for (topic_id, state) in topics_guard.iter_mut() {
                    if state.pending_ihave.is_empty() {
                        continue;
                    }

                    // Take up to MAX_IHAVE_BATCH_SIZE
                    let batch: Vec<MessageIdType> = state
                        .pending_ihave
                        .drain(..state.pending_ihave.len().min(MAX_IHAVE_BATCH_SIZE))
                        .collect();

                    let lazy_peers: Vec<PeerId> = state.lazy_peers.iter().copied().collect();

                    trace!(topic = ?topic_id, batch_size = batch.len(), peer_count = lazy_peers.len(), "Flushing IHAVE batch");

                    // Send IHAVE to each lazy peer
                    for peer in lazy_peers {
                        let ihave_header = MessageHeader {
                            version: 1,
                            topic: *topic_id,
                            msg_id: batch[0], // Use first ID as header
                            kind: MessageKind::IHave,
                            hop: 0,
                            ttl: 10,
                        };
                        let ihave_msg = GossipMessage {
                            header: ihave_header,
                            payload: Some(bincode::serialize(&batch).unwrap_or_default().into()),
                            signature: vec![], // TODO: Sign
                        };
                        if let Ok(bytes) = bincode::serialize(&ihave_msg) {
                            let _ = transport.send_to_peer(peer, StreamType::PubSub, bytes.into()).await;
                        }
                    }
                }
            }
        });
    }

    /// Spawn background task to clean expired cache entries
    fn spawn_cache_cleaner(&self) {
        let topics = self.topics.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await;

                let mut topics_guard = topics.write().await;

                for state in topics_guard.values_mut() {
                    state.clean_cache();
                }
            }
        });
    }

    /// Spawn background task to maintain eager peer degree
    fn spawn_degree_maintainer(&self) {
        let topics = self.topics.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(30));

            loop {
                interval.tick().await;

                let mut topics_guard = topics.write().await;

                for state in topics_guard.values_mut() {
                    state.maintain_degree();
                }
            }
        });
    }

    /// Initialize peers for a topic from membership layer
    pub async fn initialize_topic_peers(&self, topic: TopicId, peers: Vec<PeerId>) {
        let mut topics = self.topics.write().await;
        let state = topics.entry(topic).or_insert_with(TopicState::new);

        // Start with all peers as eager (tree will optimize via PRUNE)
        for peer in peers {
            state.eager_peers.insert(peer);
        }

        debug!(topic = ?topic, peer_count = state.eager_peers.len(), "Initialized topic peers");
    }
}

#[async_trait::async_trait]
impl<T: GossipTransport + 'static> PubSub for PlumtreePubSub<T> {
    async fn publish(&self, topic: TopicId, data: Bytes) -> Result<()> {
        self.publish_local(topic, data).await
    }

    fn subscribe(&self, topic: TopicId) -> mpsc::UnboundedReceiver<(PeerId, Bytes)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let topics = self.topics.clone();

        tokio::spawn(async move {
            let mut topics_guard = topics.write().await;
            let state = topics_guard.entry(topic).or_insert_with(TopicState::new);
            state.subscribers.push(tx);
        });

        rx
    }

    async fn unsubscribe(&self, topic: TopicId) -> Result<()> {
        let mut topics = self.topics.write().await;
        topics.remove(&topic);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saorsa_gossip_transport::{QuicTransport, TransportConfig};

    fn test_peer_id(id: u8) -> PeerId {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        PeerId::new(bytes)
    }

    fn test_transport() -> Arc<QuicTransport> {
        Arc::new(QuicTransport::new(TransportConfig::default()))
    }

    #[tokio::test]
    async fn test_pubsub_creation() {
        let peer_id = test_peer_id(1);
        let transport = test_transport();
        let _pubsub = PlumtreePubSub::new(peer_id, transport);
    }

    #[tokio::test]
    async fn test_publish_and_subscribe() {
        let peer_id = test_peer_id(1);
        let transport = test_transport();
        let pubsub = PlumtreePubSub::new(peer_id, transport);
        let topic = TopicId::new([1u8; 32]);

        let mut rx = pubsub.subscribe(topic);
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let data = Bytes::from("test message");
        pubsub.publish(topic, data.clone()).await.ok();

        let received = tokio::time::timeout(
            tokio::time::Duration::from_millis(100),
            rx.recv()
        ).await;

        assert!(received.is_ok());
        let (_, payload) = received.unwrap().unwrap();
        assert_eq!(payload, data);
    }

    #[tokio::test]
    async fn test_message_caching() {
        let peer_id = test_peer_id(1);
        let transport = test_transport();
        let pubsub = PlumtreePubSub::new(peer_id, transport);
        let topic = TopicId::new([1u8; 32]);

        let payload = Bytes::from("test");
        let msg_id = pubsub.calculate_msg_id(&topic, &payload);

        pubsub.publish(topic, payload.clone()).await.ok();

        // Check cache
        let topics = pubsub.topics.read().await;
        let state = topics.get(&topic).unwrap();
        assert!(state.has_message(&msg_id));
    }

    #[tokio::test]
    async fn test_duplicate_detection_prune() {
        let peer_id = test_peer_id(1);
        let transport = test_transport();
        let pubsub = PlumtreePubSub::new(peer_id, transport);
        let topic = TopicId::new([1u8; 32]);
        let from_peer = test_peer_id(2);

        // Initialize peer as eager
        pubsub.initialize_topic_peers(topic, vec![from_peer]).await;

        let payload = Bytes::from("test");
        let msg_id = pubsub.calculate_msg_id(&topic, &payload);

        let header = MessageHeader {
            version: 1,
            topic,
            msg_id,
            kind: MessageKind::Eager,
            hop: 0,
            ttl: 10,
        };

        let message = GossipMessage {
            header,
            payload: Some(payload.clone()),
            signature: Vec::new(),
        };

        // First EAGER - should be accepted
        pubsub.handle_eager(from_peer, topic, message.clone()).await.ok();

        // Second EAGER - should trigger PRUNE
        pubsub.handle_eager(from_peer, topic, message).await.ok();

        // Verify peer was moved to lazy
        let topics = pubsub.topics.read().await;
        let state = topics.get(&topic).unwrap();
        assert!(!state.eager_peers.contains(&from_peer));
        assert!(state.lazy_peers.contains(&from_peer));
    }

    #[tokio::test]
    async fn test_ihave_handling() {
        let peer_id = test_peer_id(1);
        let transport = test_transport();
        let pubsub = PlumtreePubSub::new(peer_id, transport);
        let topic = TopicId::new([1u8; 32]);
        let from_peer = test_peer_id(2);

        let unknown_msg_id = [42u8; 32];

        pubsub.handle_ihave(from_peer, topic, vec![unknown_msg_id]).await.ok();

        // Verify IWANT was tracked
        let topics = pubsub.topics.read().await;
        let state = topics.get(&topic).unwrap();
        assert!(state.outstanding_iwants.contains_key(&unknown_msg_id));
    }

    #[tokio::test]
    async fn test_iwant_graft() {
        let peer_id = test_peer_id(1);
        let transport = test_transport();
        let pubsub = PlumtreePubSub::new(peer_id, transport);
        let topic = TopicId::new([1u8; 32]);
        let from_peer = test_peer_id(2);

        // Initialize peer as lazy
        {
            let mut topics = pubsub.topics.write().await;
            let state = topics.entry(topic).or_insert_with(TopicState::new);
            state.lazy_peers.insert(from_peer);
        }

        // Publish a message to cache it
        let payload = Bytes::from("test");
        pubsub.publish(topic, payload.clone()).await.ok();

        let msg_id = pubsub.calculate_msg_id(&topic, &payload);

        // Handle IWANT from lazy peer
        pubsub.handle_iwant(from_peer, topic, vec![msg_id]).await.ok();

        // Verify peer was grafted to eager
        let topics = pubsub.topics.read().await;
        let state = topics.get(&topic).unwrap();
        assert!(state.eager_peers.contains(&from_peer));
        assert!(!state.lazy_peers.contains(&from_peer));
    }

    #[tokio::test]
    async fn test_degree_maintenance() {
        let peer_id = test_peer_id(1);
        let transport = test_transport();
        let pubsub = PlumtreePubSub::new(peer_id, transport);
        let topic = TopicId::new([1u8; 32]);

        // Add many peers to lazy
        let mut peers = Vec::new();
        for i in 2..20 {
            peers.push(test_peer_id(i));
        }

        {
            let mut topics = pubsub.topics.write().await;
            let state = topics.entry(topic).or_insert_with(TopicState::new);
            for peer in &peers {
                state.lazy_peers.insert(*peer);
            }

            // Maintain degree (should promote to reach MIN_EAGER_DEGREE)
            state.maintain_degree();

            assert!(state.eager_peers.len() >= MIN_EAGER_DEGREE);
        }
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let peer_id = test_peer_id(1);
        let transport = test_transport();
        let pubsub = PlumtreePubSub::new(peer_id, transport);
        let topic = TopicId::new([1u8; 32]);

        let payload = Bytes::from("test");
        pubsub.publish(topic, payload).await.ok();

        // Manually expire cache entry
        {
            let mut topics = pubsub.topics.write().await;
            let state = topics.get_mut(&topic).unwrap();

            // Modify timestamp to simulate expiry
            for (_, cached) in state.message_cache.iter_mut() {
                cached.timestamp = Instant::now() - Duration::from_secs(CACHE_TTL_SECS + 10);
            }

            state.clean_cache();

            assert_eq!(state.message_cache.len(), 0);
        }
    }
}
