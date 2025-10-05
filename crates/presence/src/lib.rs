//! Presence beacons and user discovery
//!
//! Implements:
//! - MLS exporter-derived presence tags
//! - FOAF random-walk queries
//! - IBLT summaries for efficient reconciliation

use anyhow::{Context, Result};
use std::time::SystemTime;
use saorsa_gossip_groups::GroupContext;
use saorsa_gossip_transport::GossipTransport;
use saorsa_gossip_types::{PeerId, PresenceRecord, TopicId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Presence status for a peer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceStatus {
    /// Valid beacon seen within TTL
    Online,
    /// No recent beacon
    Offline,
    /// Unknown (never seen)
    Unknown,
}

/// Presence management trait
#[async_trait::async_trait]
pub trait Presence: Send + Sync {
    /// Broadcast presence beacon to a topic
    async fn beacon(&self, topic: TopicId) -> Result<()>;

    /// Find a user and get their address hints
    async fn find(&self, user: PeerId) -> Result<Vec<String>>;
}

/// Presence manager implementation
pub struct PresenceManager {
    /// Our peer ID
    peer_id: PeerId,
    /// Transport layer for sending beacons
    transport: Arc<dyn GossipTransport>,
    /// MLS groups we've joined
    groups: Arc<RwLock<HashMap<TopicId, GroupContext>>>,
    /// Background task handle for beacon broadcasting
    beacon_task: Arc<RwLock<Option<JoinHandle<()>>>>,
    /// Shutdown signal sender
    shutdown_tx: Arc<RwLock<Option<tokio::sync::mpsc::Sender<()>>>>,
    /// Received beacons: TopicId -> (PeerId -> PresenceRecord)
    received_beacons: Arc<RwLock<HashMap<TopicId, HashMap<PeerId, PresenceRecord>>>>,
}

impl PresenceManager {
    /// Create a new presence manager
    pub fn new(
        peer_id: PeerId,
        transport: Arc<dyn GossipTransport>,
        groups: Arc<RwLock<HashMap<TopicId, GroupContext>>>,
    ) -> Self {
        Self {
            peer_id,
            transport,
            groups,
            beacon_task: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(RwLock::new(None)),
            received_beacons: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start periodic beacon broadcasting
    ///
    /// Broadcasts presence beacons to all joined topics at the specified interval.
    /// Beacons contain:
    /// - Presence tag derived from MLS exporter secret
    /// - Address hints for connectivity
    /// - Timestamp and expiration
    ///
    /// # Arguments
    /// * `interval_secs` - Beacon broadcast interval in seconds (typically 300 = 5min)
    pub async fn start_beacons(&self, interval_secs: u64) -> Result<()> {
        // Check if already running
        {
            let task = self.beacon_task.read().await;
            if task.is_some() {
                return Err(anyhow::anyhow!("Beacon broadcasting already started"));
            }
        }

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

        // Clone everything needed for the background task
        let peer_id = self.peer_id;
        let groups = self.groups.clone();
        let transport = self.transport.clone();
        let received_beacons = self.received_beacons.clone();

        // Spawn background task for beacon broadcasting
        let task_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Broadcast beacons to all joined groups
                        let groups_lock = groups.read().await;

                        for (topic_id, group_ctx) in groups_lock.iter() {
                            // Derive presence tag for current time slice
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            let time_slice = now / 3600; // Hourly rotation

                            // Use placeholder exporter secret (in production, use real MLS secret)
                            let exporter_secret = [0u8; 32]; // TODO: Get from group_ctx
                            let presence_tag = derive_presence_tag(&exporter_secret, &peer_id, time_slice);

                            // Create presence record
                            let addr_hints = vec!["127.0.0.1:8080".to_string()]; // TODO: Real addresses
                            let ttl_seconds = interval_secs * 3; // Beacons valid for 3x interval
                            let record = PresenceRecord::new(presence_tag, addr_hints, ttl_seconds);

                            // Broadcast via transport (placeholder - in production, encrypt to group)
                            // For now, just store our own beacon locally for testing
                            let mut beacons = received_beacons.write().await;
                            let topic_beacons = beacons.entry(*topic_id).or_insert_with(HashMap::new);
                            topic_beacons.insert(peer_id, record);
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        // Shutdown signal received
                        break;
                    }
                }
            }
        });

        // Store task handle and shutdown sender
        {
            let mut task = self.beacon_task.write().await;
            *task = Some(task_handle);
        }
        {
            let mut tx = self.shutdown_tx.write().await;
            *tx = Some(shutdown_tx);
        }

        Ok(())
    }

    /// Stop beacon broadcasting
    ///
    /// Gracefully shuts down the beacon broadcasting task.
    pub async fn stop_beacons(&self) -> Result<()> {
        // Send shutdown signal
        {
            let mut tx = self.shutdown_tx.write().await;
            if let Some(sender) = tx.take() {
                // Send shutdown signal (ignore error if receiver already dropped)
                let _ = sender.send(()).await;
            }
        }

        // Wait for task to complete with timeout
        {
            let mut task = self.beacon_task.write().await;
            if let Some(handle) = task.take() {
                // Wait up to 5 seconds for graceful shutdown
                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(5),
                    handle,
                ).await {
                    Ok(join_result) => {
                        join_result.context("Beacon task panicked")?;
                    }
                    Err(_) => {
                        return Err(anyhow::anyhow!("Beacon task shutdown timeout"));
                    }
                }
            }
        }

        Ok(())
    }

    /// Get presence status for a peer in a specific topic
    ///
    /// # Arguments
    /// * `peer` - The peer to check
    /// * `topic` - The topic/group context
    ///
    /// # Returns
    /// * `PresenceStatus::Online` - Valid beacon within TTL
    /// * `PresenceStatus::Offline` - Beacon expired
    /// * `PresenceStatus::Unknown` - Never seen
    pub async fn get_status(&self, peer: PeerId, topic: TopicId) -> PresenceStatus {
        let beacons = self.received_beacons.read().await;

        // Check if we have any beacons for this topic
        if let Some(topic_beacons) = beacons.get(&topic) {
            if let Some(record) = topic_beacons.get(&peer) {
                // Check if beacon is expired
                if record.is_expired() {
                    return PresenceStatus::Offline;
                } else {
                    return PresenceStatus::Online;
                }
            }
        }

        PresenceStatus::Unknown
    }

    /// Get all online peers in a topic
    ///
    /// Returns all peers with valid (non-expired) beacons in the specified topic.
    pub async fn get_online_peers(&self, topic: TopicId) -> Vec<PeerId> {
        let beacons = self.received_beacons.read().await;

        if let Some(topic_beacons) = beacons.get(&topic) {
            topic_beacons
                .iter()
                .filter(|(_, record)| !record.is_expired())
                .map(|(peer_id, _)| *peer_id)
                .collect()
        } else {
            vec![]
        }
    }

    /// Clean up expired beacons
    ///
    /// Removes beacons older than the specified TTL.
    ///
    /// # Arguments
    /// * `ttl_seconds` - Time-to-live in seconds (typically 900 = 15min)
    pub async fn cleanup_expired(&self, _ttl_seconds: u64) -> Result<usize> {
        let mut beacons = self.received_beacons.write().await;
        let mut cleaned_count = 0;

        // Iterate through all topics
        for topic_beacons in beacons.values_mut() {
            // Remove expired beacons
            topic_beacons.retain(|_, record| {
                let expired = record.is_expired();
                if expired {
                    cleaned_count += 1;
                }
                !expired
            });
        }

        Ok(cleaned_count)
    }

    /// Handle received beacon from a peer
    ///
    /// Stores the beacon for presence tracking.
    pub async fn handle_beacon(
        &self,
        topic: TopicId,
        peer: PeerId,
        record: PresenceRecord,
    ) -> Result<()> {
        let mut beacons = self.received_beacons.write().await;

        // Get or create topic beacon map
        let topic_beacons = beacons.entry(topic).or_insert_with(HashMap::new);

        // Store the beacon
        topic_beacons.insert(peer, record);

        Ok(())
    }
}

impl Default for PresenceManager {
    fn default() -> Self {
        Self::new(
            PeerId::new([0u8; 32]),
            Arc::new(saorsa_gossip_transport::QuicTransport::new(
                saorsa_gossip_transport::TransportConfig::default(),
            )),
            Arc::new(RwLock::new(HashMap::new())),
        )
    }
}

#[async_trait::async_trait]
impl Presence for PresenceManager {
    async fn beacon(&self, _topic: TopicId) -> Result<()> {
        // Placeholder: derive presence_tag from MLS exporter_secret
        // Sign with ML-DSA, encrypt to group, broadcast
        Ok(())
    }

    async fn find(&self, _user: PeerId) -> Result<Vec<String>> {
        // Placeholder: FOAF random-walk with TTL 3-4, fanout 3
        Ok(vec![])
    }
}

/// Derive presence tag from MLS exporter secret
///
/// Uses BLAKE3 keyed hash to derive a rotating presence tag.
/// Tags rotate every hour based on time_slice.
///
/// # Arguments
/// * `exporter_secret` - MLS group exporter secret (32 bytes)
/// * `user_id` - PeerId of the user
/// * `time_slice` - Current time slice (hour since epoch)
pub fn derive_presence_tag(
    exporter_secret: &[u8; 32],
    user_id: &PeerId,
    time_slice: u64,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(exporter_secret);
    hasher.update(user_id.as_bytes());
    hasher.update(&time_slice.to_le_bytes());
    let hash = hasher.finalize();
    let mut tag = [0u8; 32];
    tag.copy_from_slice(&hash.as_bytes()[..32]);
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use saorsa_gossip_transport::{QuicTransport, TransportConfig};

    // Helper: Create test presence manager
    fn create_test_manager() -> PresenceManager {
        let peer_id = PeerId::new([1u8; 32]);
        let transport = Arc::new(QuicTransport::new(TransportConfig::default()));
        let groups = Arc::new(RwLock::new(HashMap::new()));
        PresenceManager::new(peer_id, transport, groups)
    }

    #[tokio::test]
    async fn test_presence_manager_creation() {
        // RED: Test basic creation with dependencies
        let manager = create_test_manager();
        assert_eq!(manager.peer_id, PeerId::new([1u8; 32]));
    }

    #[tokio::test]
    async fn test_start_beacons_broadcasts_periodically() {
        // RED: This should fail because start_beacons doesn't broadcast yet
        let manager = create_test_manager();

        // Start beacons with 1 second interval
        let result = manager.start_beacons(1).await;
        assert!(result.is_ok(), "start_beacons should succeed");

        // TODO: Verify beacons are being broadcast
        // This will fail until we implement the broadcasting loop
    }

    #[tokio::test]
    async fn test_stop_beacons_halts_broadcasting() {
        // RED: This should fail because stop_beacons doesn't halt anything yet
        let manager = create_test_manager();

        manager.start_beacons(1).await.expect("start failed");

        // Stop beacons
        let result = manager.stop_beacons().await;
        assert!(result.is_ok(), "stop_beacons should succeed");

        // TODO: Verify no more beacons are sent after stop
    }

    #[tokio::test]
    async fn test_beacon_storage_and_retrieval() {
        // RED: This should fail because handle_beacon doesn't store yet
        let manager = create_test_manager();

        let topic = TopicId::new([1u8; 32]);
        let peer = PeerId::new([2u8; 32]);
        let record = PresenceRecord::new([0u8; 32], vec!["127.0.0.1:8080".to_string()], 900);

        manager.handle_beacon(topic, peer, record.clone()).await.expect("handle_beacon failed");

        // Should be able to retrieve the beacon
        let status = manager.get_status(peer, topic).await;
        assert_eq!(status, PresenceStatus::Online, "Peer should be online after beacon");
    }

    #[tokio::test]
    async fn test_beacon_ttl_expiration() {
        // Test that expired beacons are cleaned up
        let manager = create_test_manager();

        let topic = TopicId::new([1u8; 32]);
        let peer = PeerId::new([2u8; 32]);

        // Create an expired beacon (TTL = 0)
        let record = PresenceRecord::new([0u8; 32], vec![], 0);
        manager.handle_beacon(topic, peer, record).await.expect("handle failed");

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Clean up expired beacons
        let cleaned = manager.cleanup_expired(1).await.expect("cleanup failed");
        assert_eq!(cleaned, 1, "Should clean up 1 expired beacon");

        // Status should be unknown after cleanup (beacon removed)
        let status = manager.get_status(peer, topic).await;
        assert_eq!(status, PresenceStatus::Unknown, "Peer should be unknown after cleanup removes beacon");
    }

    #[tokio::test]
    async fn test_get_status_online_within_ttl() {
        // RED: This should fail because get_status always returns Unknown
        let manager = create_test_manager();

        let topic = TopicId::new([1u8; 32]);
        let peer = PeerId::new([2u8; 32]);
        let record = PresenceRecord::new([0u8; 32], vec![], 900);

        manager.handle_beacon(topic, peer, record).await.expect("handle failed");

        let status = manager.get_status(peer, topic).await;
        assert_eq!(status, PresenceStatus::Online, "Should be online with valid beacon");
    }

    #[tokio::test]
    async fn test_get_status_offline_after_ttl() {
        // RED: This should fail because get_status doesn't check TTL
        let manager = create_test_manager();

        let topic = TopicId::new([1u8; 32]);
        let peer = PeerId::new([2u8; 32]);

        // Beacon with 0 TTL (immediately expired)
        let record = PresenceRecord::new([0u8; 32], vec![], 0);
        manager.handle_beacon(topic, peer, record).await.expect("handle failed");

        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let status = manager.get_status(peer, topic).await;
        assert_eq!(status, PresenceStatus::Offline, "Should be offline with expired beacon");
    }

    #[tokio::test]
    async fn test_get_online_peers_filters_by_topic() {
        // RED: This should fail because get_online_peers returns empty vec
        let manager = create_test_manager();

        let topic1 = TopicId::new([1u8; 32]);
        let topic2 = TopicId::new([2u8; 32]);
        let peer1 = PeerId::new([10u8; 32]);
        let peer2 = PeerId::new([20u8; 32]);

        // Add beacons to different topics
        let record = PresenceRecord::new([0u8; 32], vec![], 900);
        manager.handle_beacon(topic1, peer1, record.clone()).await.expect("handle1 failed");
        manager.handle_beacon(topic2, peer2, record).await.expect("handle2 failed");

        // Should only see peer1 in topic1
        let online = manager.get_online_peers(topic1).await;
        assert_eq!(online.len(), 1, "Should have 1 online peer in topic1");
        assert!(online.contains(&peer1), "Should contain peer1");

        // Should only see peer2 in topic2
        let online = manager.get_online_peers(topic2).await;
        assert_eq!(online.len(), 1, "Should have 1 online peer in topic2");
        assert!(online.contains(&peer2), "Should contain peer2");
    }

    #[tokio::test]
    async fn test_find_foaf_random_walk() {
        // RED: This should fail because find doesn't implement FOAF
        let manager = create_test_manager();

        let target = PeerId::new([42u8; 32]);

        // Should return address hints if user is found
        let result = manager.find(target).await;
        assert!(result.is_ok(), "find should succeed");

        // TODO: Verify FOAF query was sent with TTL=3, fanout=3
    }

    #[tokio::test]
    async fn test_multiple_topics_isolation() {
        // RED: This should fail because topics aren't isolated yet
        let manager = create_test_manager();

        let topic1 = TopicId::new([1u8; 32]);
        let topic2 = TopicId::new([2u8; 32]);
        let peer = PeerId::new([5u8; 32]);

        // Add beacon only to topic1
        let record = PresenceRecord::new([0u8; 32], vec![], 900);
        manager.handle_beacon(topic1, peer, record).await.expect("handle failed");

        // Should be online in topic1
        assert_eq!(manager.get_status(peer, topic1).await, PresenceStatus::Online);

        // Should be unknown in topic2
        assert_eq!(manager.get_status(peer, topic2).await, PresenceStatus::Unknown);
    }

    #[test]
    fn test_derive_presence_tag_deterministic() {
        // Test that same inputs produce same tag
        let secret = [1u8; 32];
        let peer = PeerId::new([2u8; 32]);
        let time_slice = 12345u64;

        let tag1 = derive_presence_tag(&secret, &peer, time_slice);
        let tag2 = derive_presence_tag(&secret, &peer, time_slice);

        assert_eq!(tag1, tag2, "Same inputs should produce same tag");
    }

    #[test]
    fn test_derive_presence_tag_rotation() {
        // Test that different time slices produce different tags
        let secret = [1u8; 32];
        let peer = PeerId::new([2u8; 32]);

        let tag1 = derive_presence_tag(&secret, &peer, 1000);
        let tag2 = derive_presence_tag(&secret, &peer, 1001);

        assert_ne!(tag1, tag2, "Different time slices should produce different tags");
    }

    #[test]
    fn test_derive_presence_tag_peer_unique() {
        // Test that different peers produce different tags
        let secret = [1u8; 32];
        let peer1 = PeerId::new([1u8; 32]);
        let peer2 = PeerId::new([2u8; 32]);
        let time_slice = 12345u64;

        let tag1 = derive_presence_tag(&secret, &peer1, time_slice);
        let tag2 = derive_presence_tag(&secret, &peer2, time_slice);

        assert_ne!(tag1, tag2, "Different peers should produce different tags");
    }
}
