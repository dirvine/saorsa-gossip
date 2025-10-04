//! Membership management using HyParView + SWIM
//!
//! Provides:
//! - HyParView for partial views (active + passive)
//! - SWIM for failure detection
//! - Periodic shuffling and anti-entropy

use anyhow::Result;
use saorsa_gossip_types::PeerId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default active view degree (8-12 peers)
pub const DEFAULT_ACTIVE_DEGREE: usize = 8;
/// Default passive view degree (64-128 peers)
pub const DEFAULT_PASSIVE_DEGREE: usize = 64;
/// Shuffle period in seconds
pub const SHUFFLE_PERIOD_SECS: u64 = 30;

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

/// SWIM failure detector
pub struct SwimDetector {
    /// Peer states
    states: Arc<RwLock<HashMap<PeerId, PeerState>>>,
    /// Probe period in seconds
    probe_period: u64,
    /// Suspect timeout in seconds
    suspect_timeout: u64,
}

impl SwimDetector {
    /// Create a new SWIM detector
    pub fn new(probe_period: u64, suspect_timeout: u64) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            probe_period,
            suspect_timeout,
        }
    }

    /// Mark a peer as alive
    pub async fn mark_alive(&self, peer: PeerId) {
        let mut states = self.states.write().await;
        states.insert(peer, PeerState::Alive);
    }

    /// Mark a peer as suspect
    pub async fn mark_suspect(&self, peer: PeerId) {
        let mut states = self.states.write().await;
        states.insert(peer, PeerState::Suspect);
    }

    /// Mark a peer as dead
    pub async fn mark_dead(&self, peer: PeerId) {
        let mut states = self.states.write().await;
        states.insert(peer, PeerState::Dead);
    }

    /// Get the state of a peer
    pub async fn get_state(&self, peer: &PeerId) -> Option<PeerState> {
        let states = self.states.read().await;
        states.get(peer).copied()
    }

    /// Get the probe period
    pub fn probe_period(&self) -> u64 {
        self.probe_period
    }

    /// Get the suspect timeout
    pub fn suspect_timeout(&self) -> u64 {
        self.suspect_timeout
    }
}

/// HyParView membership implementation
pub struct HyParViewMembership {
    /// Active view (for routing)
    active: Arc<RwLock<HashSet<PeerId>>>,
    /// Passive view (for healing)
    passive: Arc<RwLock<HashSet<PeerId>>>,
    /// SWIM failure detector
    swim: SwimDetector,
    /// Active view degree
    active_degree: usize,
    /// Passive view degree
    #[allow(dead_code)]
    passive_degree: usize,
}

impl HyParViewMembership {
    /// Create a new HyParView membership manager
    pub fn new(active_degree: usize, passive_degree: usize) -> Self {
        Self {
            active: Arc::new(RwLock::new(HashSet::new())),
            passive: Arc::new(RwLock::new(HashSet::new())),
            swim: SwimDetector::new(1, 3), // 1s probe, 3s suspect timeout
            active_degree,
            passive_degree,
        }
    }

    /// Get the SWIM detector
    pub fn swim(&self) -> &SwimDetector {
        &self.swim
    }

    /// Shuffle the passive view with a random peer
    pub async fn shuffle(&self) -> Result<()> {
        // Implementation would select random peers and exchange views
        // Placeholder for now
        Ok(())
    }
}

impl Default for HyParViewMembership {
    fn default() -> Self {
        Self::new(DEFAULT_ACTIVE_DEGREE, DEFAULT_PASSIVE_DEGREE)
    }
}

#[async_trait::async_trait]
impl Membership for HyParViewMembership {
    async fn join(&self, seeds: Vec<String>) -> Result<()> {
        // Parse seed addresses and add to active view
        for seed in seeds {
            // In a real implementation, we would:
            // 1. Parse the seed address
            // 2. Create a PeerId from it
            // 3. Establish connection
            // 4. Add to active view
            // Placeholder for now
            let _ = seed;
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

        // If active view is full, remove a random peer
        if active.len() >= self.active_degree {
            if let Some(&to_remove) = active.iter().next() {
                active.remove(&to_remove);
                // Move to passive view
                let mut passive = self.passive.write().await;
                passive.insert(to_remove);
            }
        }

        active.insert(peer);
        self.swim.mark_alive(peer).await;
        Ok(())
    }

    async fn remove_active(&self, peer: PeerId) -> Result<()> {
        let mut active = self.active.write().await;
        active.remove(&peer);
        self.swim.mark_dead(peer).await;
        Ok(())
    }

    async fn promote(&self, peer: PeerId) -> Result<()> {
        let mut passive = self.passive.write().await;
        if passive.remove(&peer) {
            drop(passive); // Release lock before calling add_active
            self.add_active(peer).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hyparview_creation() {
        let membership = HyParViewMembership::default();
        assert_eq!(membership.active_view().len(), 0);
        assert_eq!(membership.passive_view().len(), 0);
    }

    #[tokio::test]
    async fn test_add_active_peer() {
        let membership = HyParViewMembership::default();
        let peer = PeerId::new([1u8; 32]);

        membership.add_active(peer).await.ok();
        let active = membership.active_view();
        assert_eq!(active.len(), 1);
        assert!(active.contains(&peer));
    }

    #[tokio::test]
    async fn test_remove_active_peer() {
        let membership = HyParViewMembership::default();
        let peer = PeerId::new([1u8; 32]);

        membership.add_active(peer).await.ok();
        membership.remove_active(peer).await.ok();

        let active = membership.active_view();
        assert_eq!(active.len(), 0);
    }

    #[tokio::test]
    async fn test_swim_states() {
        let swim = SwimDetector::new(1, 3);
        let peer = PeerId::new([1u8; 32]);

        swim.mark_alive(peer).await;
        assert_eq!(swim.get_state(&peer).await, Some(PeerState::Alive));

        swim.mark_suspect(peer).await;
        assert_eq!(swim.get_state(&peer).await, Some(PeerState::Suspect));

        swim.mark_dead(peer).await;
        assert_eq!(swim.get_state(&peer).await, Some(PeerState::Dead));
    }

    #[tokio::test]
    async fn test_promote_from_passive() {
        let membership = HyParViewMembership::default();
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
}
