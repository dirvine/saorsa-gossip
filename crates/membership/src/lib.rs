//! Membership management using HyParView + SWIM
//!
//! Provides:
//! - HyParView for partial views (active + passive)
//! - SWIM for failure detection
//! - Periodic shuffling and anti-entropy

use anyhow::{anyhow, Result};
use saorsa_gossip_transport::{GossipTransport, StreamType};
use saorsa_gossip_types::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time;
use tracing::{debug, trace, warn};

/// Default active view degree (8-12 peers)
pub const DEFAULT_ACTIVE_DEGREE: usize = 8;
/// Maximum active view degree
pub const MAX_ACTIVE_DEGREE: usize = 12;
/// Default passive view degree (64-128 peers)
pub const DEFAULT_PASSIVE_DEGREE: usize = 64;
/// Maximum passive view degree
pub const MAX_PASSIVE_DEGREE: usize = 128;
/// Shuffle period in seconds (per SPEC.md)
pub const SHUFFLE_PERIOD_SECS: u64 = 30;
/// SWIM probe interval (per SPEC.md)
pub const SWIM_PROBE_INTERVAL_SECS: u64 = 1;
/// SWIM suspect timeout (per SPEC.md)
pub const SWIM_SUSPECT_TIMEOUT_SECS: u64 = 3;

/// SWIM protocol messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SwimMessage {
    /// Ping message to probe peer
    Ping,
    /// Ack response to ping
    Ack,
}

/// HyParView protocol messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum HyParViewMessage {
    /// Join request
    Join(PeerId),
    /// Shuffle request with peer list
    Shuffle(Vec<PeerId>),
    /// ForwardJoin request
    ForwardJoin(PeerId, usize),
    /// Disconnect notification
    Disconnect,
}

/// Membership management trait
#[async_trait::async_trait]
pub trait Membership: Send + Sync {
    /// Join the overlay network with seed peers
    async fn join(&self, seeds: Vec<String>) -> Result<()>;

    /// Get the active view (peers for routing)
    fn active_view(&self) -> Vec<PeerId>;

    /// Get the passive view (peers for healing)
    fn passive_view(&self) -> Vec<PeerId>;

    /// Add a peer to the active view
    async fn add_active(&self, peer: PeerId) -> Result<()>;

    /// Remove a peer from the active view
    async fn remove_active(&self, peer: PeerId) -> Result<()>;

    /// Promote a peer from passive to active view
    async fn promote(&self, peer: PeerId) -> Result<()>;
}

/// Peer state for SWIM failure detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// Peer is alive and responding
    Alive,
    /// Peer is suspected of failure
    Suspect,
    /// Peer is confirmed dead
    Dead,
}

/// SWIM peer entry with timestamp
#[derive(Clone, Debug)]
struct SwimPeerEntry {
    state: PeerState,
    last_update: Instant,
}

/// SWIM failure detector
pub struct SwimDetector<T: GossipTransport + 'static> {
    /// Peer states with timestamps
    states: Arc<RwLock<HashMap<PeerId, SwimPeerEntry>>>,
    /// Probe period in seconds
    probe_period: u64,
    /// Suspect timeout in seconds
    suspect_timeout: u64,
    /// Transport layer for sending probes
    transport: Arc<T>,
}

impl<T: GossipTransport + 'static> SwimDetector<T> {
    /// Create a new SWIM detector
    pub fn new(probe_period: u64, suspect_timeout: u64, transport: Arc<T>) -> Self {
        let detector = Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            probe_period,
            suspect_timeout,
            transport,
        };

        // Start background probing task
        detector.spawn_probe_task();
        detector.spawn_suspect_timeout_task();

        detector
    }

    /// Mark a peer as alive
    pub async fn mark_alive(&self, peer: PeerId) {
        let mut states = self.states.write().await;
        states.insert(
            peer,
            SwimPeerEntry {
                state: PeerState::Alive,
                last_update: Instant::now(),
            },
        );
        trace!(peer_id = %peer, "SWIM: Marked peer as alive");
    }

    /// Mark a peer as suspect
    pub async fn mark_suspect(&self, peer: PeerId) {
        let mut states = self.states.write().await;
        if let Some(entry) = states.get_mut(&peer) {
            if entry.state == PeerState::Alive {
                entry.state = PeerState::Suspect;
                entry.last_update = Instant::now();
                debug!(peer_id = %peer, "SWIM: Marked peer as suspect");
            }
        }
    }

    /// Mark a peer as dead
    pub async fn mark_dead(&self, peer: PeerId) {
        let mut states = self.states.write().await;
        states.insert(
            peer,
            SwimPeerEntry {
                state: PeerState::Dead,
                last_update: Instant::now(),
            },
        );
        warn!(peer_id = %peer, "SWIM: Marked peer as dead");
    }

    /// Get the state of a peer
    pub async fn get_state(&self, peer: &PeerId) -> Option<PeerState> {
        let states = self.states.read().await;
        states.get(peer).map(|entry| entry.state)
    }

    /// Get all peers in a specific state
    pub async fn get_peers_in_state(&self, state: PeerState) -> Vec<PeerId> {
        let states = self.states.read().await;
        states
            .iter()
            .filter(|(_, entry)| entry.state == state)
            .map(|(peer, _)| *peer)
            .collect()
    }

    /// Remove a peer from tracking
    pub async fn remove_peer(&self, peer: &PeerId) {
        let mut states = self.states.write().await;
        states.remove(peer);
    }

    /// Get the probe period
    pub fn probe_period(&self) -> u64 {
        self.probe_period
    }

    /// Get the suspect timeout
    pub fn suspect_timeout(&self) -> u64 {
        self.suspect_timeout
    }

    /// Spawn background task to probe random peers
    fn spawn_probe_task(&self) {
        let states = self.states.clone();
        let probe_period = self.probe_period;
        let transport = self.transport.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(probe_period));

            loop {
                interval.tick().await;

                let states_guard = states.read().await;
                let alive_peers: Vec<PeerId> = states_guard
                    .iter()
                    .filter(|(_, entry)| entry.state == PeerState::Alive)
                    .map(|(peer, _)| *peer)
                    .collect();
                drop(states_guard);

                if let Some(&peer) = alive_peers.first() {
                    // Send PING to peer via transport
                    trace!(peer_id = %peer, "SWIM: Probing peer");
                    let ping_msg = SwimMessage::Ping;
                    if let Ok(bytes) = bincode::serialize(&ping_msg) {
                        let _ = transport
                            .send_to_peer(peer, StreamType::Membership, bytes.into())
                            .await;
                    }
                    // Note: Response handling would mark peer alive/suspect
                    // For now, we'll rely on manual state updates
                }
            }
        });
    }

    /// Spawn background task to check suspect timeouts
    fn spawn_suspect_timeout_task(&self) {
        let states = self.states.clone();
        let suspect_timeout = self.suspect_timeout;

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1));

            loop {
                interval.tick().await;

                let mut states_guard = states.write().await;
                let now = Instant::now();

                // Find suspects that have timed out
                let mut to_mark_dead = Vec::new();
                for (peer, entry) in states_guard.iter() {
                    if entry.state == PeerState::Suspect {
                        let elapsed = now.duration_since(entry.last_update);
                        if elapsed > Duration::from_secs(suspect_timeout) {
                            to_mark_dead.push(*peer);
                        }
                    }
                }

                // Mark timed-out suspects as dead
                for peer in to_mark_dead {
                    states_guard.insert(
                        peer,
                        SwimPeerEntry {
                            state: PeerState::Dead,
                            last_update: now,
                        },
                    );
                    warn!(peer_id = %peer, "SWIM: Suspect timeout → marked dead");
                }
            }
        });
    }
}

/// HyParView membership implementation
pub struct HyParViewMembership<T: GossipTransport + 'static> {
    /// Active view (for routing)
    active: Arc<RwLock<HashSet<PeerId>>>,
    /// Passive view (for healing)
    passive: Arc<RwLock<HashSet<PeerId>>>,
    /// SWIM failure detector
    swim: SwimDetector<T>,
    /// Active view degree
    active_degree: usize,
    /// Passive view degree
    passive_degree: usize,
    /// Transport layer for sending messages
    transport: Arc<T>,
}

impl<T: GossipTransport + 'static> HyParViewMembership<T> {
    /// Create a new HyParView membership manager
    pub fn new(active_degree: usize, passive_degree: usize, transport: Arc<T>) -> Self {
        let membership = Self {
            active: Arc::new(RwLock::new(HashSet::new())),
            passive: Arc::new(RwLock::new(HashSet::new())),
            swim: SwimDetector::new(
                SWIM_PROBE_INTERVAL_SECS,
                SWIM_SUSPECT_TIMEOUT_SECS,
                transport.clone(),
            ),
            active_degree,
            passive_degree,
            transport,
        };

        // Start background shuffle task
        membership.spawn_shuffle_task();
        membership.spawn_degree_maintenance_task();

        membership
    }

    /// Get the SWIM detector
    pub fn swim(&self) -> &SwimDetector<T> {
        &self.swim
    }

    /// Shuffle the passive view with a random peer
    pub async fn shuffle(&self) -> Result<()> {
        let active = self.active.read().await;
        let passive = self.passive.read().await;

        if active.is_empty() {
            return Ok(());
        }

        // Select random active peer for shuffle
        let target = *active
            .iter()
            .next()
            .ok_or_else(|| anyhow!("No active peers"))?;

        // Select random subset of passive view to exchange
        let exchange_size = (self.passive_degree / 4).max(1);
        let to_exchange: Vec<PeerId> = passive.iter().take(exchange_size).copied().collect();

        drop(active);
        drop(passive);

        debug!(
            peer_id = %target,
            exchange_count = to_exchange.len(),
            "HyParView: Shuffling passive view"
        );

        // Send SHUFFLE message to target peer via transport
        let shuffle_msg = HyParViewMessage::Shuffle(to_exchange);
        if let Ok(bytes) = bincode::serialize(&shuffle_msg) {
            self.transport
                .send_to_peer(target, StreamType::Membership, bytes.into())
                .await?;
        }
        // Note: Peer will respond with their own passive view subset
        // We'll merge responses into our passive view via handle_shuffle_response()

        Ok(())
    }

    /// Maintain active and passive view degrees
    #[cfg(test)]
    async fn maintain_degrees(&self) {
        let mut active = self.active.write().await;
        let mut passive = self.passive.write().await;

        // Enforce active degree limits (8-12)
        if active.len() < DEFAULT_ACTIVE_DEGREE && !passive.is_empty() {
            // Promote from passive
            let to_promote = DEFAULT_ACTIVE_DEGREE - active.len();
            let peers: Vec<PeerId> = passive.iter().take(to_promote).copied().collect();

            for peer in peers {
                passive.remove(&peer);
                active.insert(peer);
                debug!(peer_id = %peer, "Promoted from passive to active");
            }
        } else if active.len() > MAX_ACTIVE_DEGREE {
            // Demote to passive
            let to_demote = active.len() - MAX_ACTIVE_DEGREE;
            let peers: Vec<PeerId> = active.iter().take(to_demote).copied().collect();

            for peer in peers {
                active.remove(&peer);
                if passive.len() < MAX_PASSIVE_DEGREE {
                    passive.insert(peer);
                    debug!(peer_id = %peer, "Demoted from active to passive");
                }
            }
        }

        // Enforce passive degree limit (max 128)
        if passive.len() > MAX_PASSIVE_DEGREE {
            let to_remove = passive.len() - MAX_PASSIVE_DEGREE;
            let peers: Vec<PeerId> = passive.iter().take(to_remove).copied().collect();

            for peer in peers {
                passive.remove(&peer);
                trace!(peer_id = %peer, "Removed from passive view (over capacity)");
            }
        }
    }

    /// Spawn background task for periodic shuffling
    fn spawn_shuffle_task(&self) {
        let active = self.active.clone();
        let passive = self.passive.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(SHUFFLE_PERIOD_SECS));

            loop {
                interval.tick().await;

                let active_guard = active.read().await;
                let passive_guard = passive.read().await;

                if !active_guard.is_empty() {
                    debug!(
                        active_count = active_guard.len(),
                        passive_count = passive_guard.len(),
                        "HyParView: Periodic shuffle tick"
                    );
                }

                // TODO: Actual shuffle implementation requires transport
                drop(active_guard);
                drop(passive_guard);
            }
        });
    }

    /// Spawn background task for degree maintenance
    fn spawn_degree_maintenance_task(&self) {
        let active = self.active.clone();
        let passive = self.passive.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(10));

            loop {
                interval.tick().await;

                let mut active_guard = active.write().await;
                let mut passive_guard = passive.write().await;

                let active_count = active_guard.len();
                let passive_count = passive_guard.len();

                // Promote from passive if active is low
                if active_count < DEFAULT_ACTIVE_DEGREE && !passive_guard.is_empty() {
                    let to_promote = DEFAULT_ACTIVE_DEGREE - active_count;
                    let peers: Vec<PeerId> =
                        passive_guard.iter().take(to_promote).copied().collect();

                    for peer in peers {
                        passive_guard.remove(&peer);
                        active_guard.insert(peer);
                        debug!(peer_id = %peer, "Degree maintenance: promoted to active");
                    }
                }

                // Demote to passive if active is high
                if active_count > MAX_ACTIVE_DEGREE {
                    let to_demote = active_count - MAX_ACTIVE_DEGREE;
                    let peers: Vec<PeerId> = active_guard.iter().take(to_demote).copied().collect();

                    for peer in peers {
                        active_guard.remove(&peer);
                        if passive_guard.len() < MAX_PASSIVE_DEGREE {
                            passive_guard.insert(peer);
                            debug!(peer_id = %peer, "Degree maintenance: demoted to passive");
                        }
                    }
                }

                // Trim passive if over capacity
                if passive_count > MAX_PASSIVE_DEGREE {
                    let to_remove = passive_count - MAX_PASSIVE_DEGREE;
                    let peers: Vec<PeerId> =
                        passive_guard.iter().take(to_remove).copied().collect();

                    for peer in peers {
                        passive_guard.remove(&peer);
                        trace!(peer_id = %peer, "Degree maintenance: removed from passive");
                    }
                }
            }
        });
    }
}

#[async_trait::async_trait]
impl<T: GossipTransport + 'static> Membership for HyParViewMembership<T> {
    async fn join(&self, seeds: Vec<String>) -> Result<()> {
        // Parse seed addresses and add to active view
        for seed in seeds {
            // In a real implementation, we would:
            // 1. Parse the seed address (SocketAddr)
            // 2. Connect via transport
            // 3. Send JOIN message
            // 4. Receive FORWARDJOIN response with peer list
            // 5. Add peers to active/passive views

            debug!(seed = %seed, "JOIN: Attempting to join via seed (TODO: transport)");
        }
        Ok(())
    }

    fn active_view(&self) -> Vec<PeerId> {
        // Try to get read lock, return empty vec if unavailable
        match self.active.try_read() {
            Ok(active) => active.iter().copied().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn passive_view(&self) -> Vec<PeerId> {
        // Try to get read lock, return empty vec if unavailable
        match self.passive.try_read() {
            Ok(passive) => passive.iter().copied().collect(),
            Err(_) => Vec::new(),
        }
    }

    async fn add_active(&self, peer: PeerId) -> Result<()> {
        let mut active = self.active.write().await;

        // If active view is full, demote one peer to passive
        if active.len() >= self.active_degree {
            if let Some(&to_demote) = active.iter().next() {
                active.remove(&to_demote);
                // Move to passive view
                let mut passive = self.passive.write().await;
                if passive.len() < self.passive_degree {
                    passive.insert(to_demote);
                    debug!(peer_id = %to_demote, "Demoted to passive (active view full)");
                }
            }
        }

        active.insert(peer);
        drop(active); // Release lock before async call

        self.swim.mark_alive(peer).await;
        debug!(peer_id = %peer, "Added to active view");

        Ok(())
    }

    async fn remove_active(&self, peer: PeerId) -> Result<()> {
        let mut active = self.active.write().await;
        let removed = active.remove(&peer);
        drop(active);

        if removed {
            self.swim.mark_dead(peer).await;
            debug!(peer_id = %peer, "Removed from active view");
        }

        Ok(())
    }

    async fn promote(&self, peer: PeerId) -> Result<()> {
        let mut passive = self.passive.write().await;
        let was_passive = passive.remove(&peer);
        drop(passive); // Release lock before calling add_active

        if was_passive {
            self.add_active(peer).await?;
            debug!(peer_id = %peer, "Promoted from passive to active");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saorsa_gossip_transport::{QuicTransport, TransportConfig};

    fn test_transport() -> Arc<QuicTransport> {
        Arc::new(QuicTransport::new(TransportConfig::default()))
    }

    fn test_membership() -> HyParViewMembership<QuicTransport> {
        HyParViewMembership::new(
            DEFAULT_ACTIVE_DEGREE,
            DEFAULT_PASSIVE_DEGREE,
            test_transport(),
        )
    }

    #[tokio::test]
    async fn test_hyparview_creation() {
        let membership = test_membership();
        assert_eq!(membership.active_view().len(), 0);
        assert_eq!(membership.passive_view().len(), 0);
    }

    #[tokio::test]
    async fn test_add_active_peer() {
        let membership = test_membership();
        let peer = PeerId::new([1u8; 32]);

        membership.add_active(peer).await.ok();
        let active = membership.active_view();
        assert_eq!(active.len(), 1);
        assert!(active.contains(&peer));
    }

    #[tokio::test]
    async fn test_remove_active_peer() {
        let membership = test_membership();
        let peer = PeerId::new([1u8; 32]);

        membership.add_active(peer).await.ok();
        membership.remove_active(peer).await.ok();

        let active = membership.active_view();
        assert_eq!(active.len(), 0);
    }

    #[tokio::test]
    async fn test_active_view_capacity() {
        let transport = test_transport();
        let membership = HyParViewMembership::new(3, 10, transport);

        // Add 5 peers (more than capacity)
        for i in 0..5 {
            let peer = PeerId::new([i; 32]);
            membership.add_active(peer).await.ok();
        }

        // Should only have 3 in active (capacity limit)
        let active = membership.active_view();
        assert_eq!(active.len(), 3);

        // Others should be in passive
        let passive = membership.passive_view();
        assert_eq!(passive.len(), 2);
    }

    #[tokio::test]
    async fn test_swim_states() {
        let transport = test_transport();
        let swim = SwimDetector::new(1, 3, transport);
        let peer = PeerId::new([1u8; 32]);

        swim.mark_alive(peer).await;
        assert_eq!(swim.get_state(&peer).await, Some(PeerState::Alive));

        swim.mark_suspect(peer).await;
        assert_eq!(swim.get_state(&peer).await, Some(PeerState::Suspect));

        swim.mark_dead(peer).await;
        assert_eq!(swim.get_state(&peer).await, Some(PeerState::Dead));
    }

    #[tokio::test]
    async fn test_swim_suspect_timeout() {
        let transport = test_transport();
        let swim = SwimDetector::new(1, 1, transport); // 1s timeout
        let peer = PeerId::new([1u8; 32]);

        swim.mark_alive(peer).await;
        swim.mark_suspect(peer).await;

        // Wait for timeout
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Should be marked dead automatically
        assert_eq!(swim.get_state(&peer).await, Some(PeerState::Dead));
    }

    #[tokio::test]
    async fn test_promote_from_passive() {
        let membership = test_membership();
        let peer = PeerId::new([1u8; 32]);

        // Add to passive
        {
            let mut passive = membership.passive.write().await;
            passive.insert(peer);
        }

        // Promote to active
        membership.promote(peer).await.ok();

        let active = membership.active_view();
        let passive = membership.passive_view();

        assert!(active.contains(&peer));
        assert!(!passive.contains(&peer));
    }

    #[tokio::test]
    async fn test_degree_maintenance() {
        let transport = test_transport();
        let membership = HyParViewMembership::new(5, 20, transport);

        // Add many peers to passive
        for i in 0..15 {
            let peer = PeerId::new([i; 32]);
            let mut passive = membership.passive.write().await;
            passive.insert(peer);
        }

        // Run maintenance
        membership.maintain_degrees().await;

        // Should have promoted some to active
        let active = membership.active_view();
        assert!(active.len() >= 5);
        assert!(active.len() <= 12);
    }

    #[tokio::test]
    async fn test_get_peers_in_state() {
        let transport = test_transport();
        let swim = SwimDetector::new(1, 100, transport); // Long timeout so background task doesn't interfere

        let peer1 = PeerId::new([1u8; 32]);
        let peer2 = PeerId::new([2u8; 32]);
        let peer3 = PeerId::new([3u8; 32]);

        swim.mark_alive(peer1).await;
        swim.mark_alive(peer2).await; // Start as alive
        swim.mark_suspect(peer2).await; // Then mark suspect
        swim.mark_dead(peer3).await;

        let alive = swim.get_peers_in_state(PeerState::Alive).await;
        let suspects = swim.get_peers_in_state(PeerState::Suspect).await;
        let dead = swim.get_peers_in_state(PeerState::Dead).await;

        assert_eq!(alive.len(), 1);
        assert_eq!(suspects.len(), 1);
        assert_eq!(dead.len(), 1);

        assert!(alive.contains(&peer1));
        assert!(suspects.contains(&peer2));
        assert!(dead.contains(&peer3));
    }
}
