//! Coordinator advertisement handler
//!
//! Manages coordinator discovery and FOAF query routing

use crate::{AdvertCache, CoordinatorAdvert, FindCoordinatorQuery, FindCoordinatorResponse};
use saorsa_gossip_types::PeerId;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

/// Handler for coordinator advertisements and FOAF queries
pub struct CoordinatorHandler {
    /// Local peer ID
    peer_id: PeerId,
    /// Cache of known coordinators
    cache: AdvertCache,
    /// Recently seen query IDs (for deduplication)
    seen_queries: Arc<Mutex<HashSet<[u8; 32]>>>,
}

impl CoordinatorHandler {
    /// Create a new coordinator handler
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            cache: AdvertCache::default(),
            seen_queries: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn seen_queries_guard(&self) -> Option<MutexGuard<'_, HashSet<[u8; 32]>>> {
        self.seen_queries.lock().ok()
    }

    /// Get the local peer ID
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// Get a reference to the advert cache
    pub fn cache(&self) -> &AdvertCache {
        &self.cache
    }

    /// Handle receiving a coordinator advert
    ///
    /// Validates signature and adds to cache if valid.
    pub fn handle_advert(
        &self,
        advert: CoordinatorAdvert,
        public_key: &saorsa_pqc::MlDsaPublicKey,
    ) -> anyhow::Result<bool> {
        // Verify signature
        let valid = advert.verify(public_key)?;
        if !valid {
            return Ok(false);
        }

        // Add to cache if valid
        Ok(self.cache.insert(advert))
    }

    /// Handle a FIND_COORDINATOR query
    ///
    /// Returns a response with known coordinators if query is valid.
    /// Returns None if query should not be answered (duplicate, expired, TTL=0).
    pub fn handle_find_query(
        &self,
        mut query: FindCoordinatorQuery,
    ) -> Option<FindCoordinatorResponse> {
        // Check if we've seen this query before
        {
            let mut seen = self.seen_queries_guard()?;
            if seen.contains(&query.query_id) {
                return None; // Duplicate query
            }
            seen.insert(query.query_id);
        }

        // Check if query is expired
        if query.is_expired() {
            return None;
        }

        // Decrement TTL
        if !query.decrement_ttl() {
            return None; // TTL exhausted
        }

        // Get all coordinator adverts from cache
        let coordinators = self.cache.get_by_role(|advert| advert.roles.coordinator);

        // Return response with known coordinators
        Some(FindCoordinatorResponse::new(
            query.query_id,
            self.peer_id,
            coordinators,
        ))
    }

    /// Prune expired adverts and old query IDs
    ///
    /// Returns the number of expired adverts pruned.
    pub fn prune(&self) -> usize {
        let pruned = self.cache.prune_expired();

        // Clear seen queries periodically (they're only valid for 30s anyway)
        if let Some(mut seen) = self.seen_queries_guard() {
            seen.clear();
        }

        pruned
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{CoordinatorRoles, NatClass};
    use saorsa_pqc::{MlDsa65, MlDsaOperations};

    #[test]
    fn test_handler_creation() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        assert_eq!(handler.peer_id(), peer_id);
        assert_eq!(handler.cache().len(), 0);
    }

    #[test]
    fn test_handle_valid_advert() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        // Create and sign an advert
        let signer = MlDsa65::new();
        let (pk, sk) = signer.generate_keypair().expect("keypair");

        let coord_peer = PeerId::new([2u8; 32]);
        let mut advert = CoordinatorAdvert::new(
            coord_peer,
            CoordinatorRoles::default(),
            vec![],
            NatClass::Eim,
            10_000,
        );
        advert.sign(&sk).expect("signing");

        // Handle the advert
        let result = handler.handle_advert(advert, &pk).expect("handle advert");
        assert!(result, "Valid advert should be accepted");
        assert_eq!(handler.cache().len(), 1);
    }

    #[test]
    fn test_handle_invalid_signature() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        // Create advert signed with one key
        let signer = MlDsa65::new();
        let (_, sk1) = signer.generate_keypair().expect("keypair 1");
        let (pk2, _) = signer.generate_keypair().expect("keypair 2");

        let coord_peer = PeerId::new([2u8; 32]);
        let mut advert = CoordinatorAdvert::new(
            coord_peer,
            CoordinatorRoles::default(),
            vec![],
            NatClass::Eim,
            10_000,
        );
        advert.sign(&sk1).expect("signing");

        // Verify with different key
        let result = handler.handle_advert(advert, &pk2).expect("handle advert");
        assert!(!result, "Invalid signature should be rejected");
        assert_eq!(handler.cache().len(), 0);
    }

    #[test]
    fn test_handle_find_query_with_no_coordinators() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        let origin = PeerId::new([2u8; 32]);
        let query = FindCoordinatorQuery::new(origin);

        let response = handler.handle_find_query(query).expect("should respond");

        assert_eq!(response.responder, peer_id);
        assert!(response.adverts.is_empty(), "No coordinators known yet");
    }

    #[test]
    fn test_handle_find_query_with_coordinators() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        // Add a coordinator to cache
        let signer = MlDsa65::new();
        let (pk, sk) = signer.generate_keypair().expect("keypair");

        let coord_peer = PeerId::new([2u8; 32]);
        let mut advert = CoordinatorAdvert::new(
            coord_peer,
            CoordinatorRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
            vec![],
            NatClass::Eim,
            10_000,
        );
        advert.sign(&sk).expect("signing");
        handler.handle_advert(advert, &pk).expect("handle");

        // Query for coordinators
        let origin = PeerId::new([3u8; 32]);
        let query = FindCoordinatorQuery::new(origin);

        let response = handler.handle_find_query(query).expect("should respond");

        assert_eq!(response.responder, peer_id);
        assert_eq!(response.adverts.len(), 1, "Should return the coordinator");
        assert_eq!(response.adverts[0].peer, coord_peer);
    }

    #[test]
    fn test_handle_duplicate_query() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        let origin = PeerId::new([2u8; 32]);
        let query = FindCoordinatorQuery::new(origin);
        let query_id = query.query_id;

        // First query should succeed
        let response1 = handler.handle_find_query(query.clone());
        assert!(response1.is_some(), "First query should get response");

        // Duplicate query should be ignored
        let response2 = handler.handle_find_query(query.clone());
        assert!(response2.is_none(), "Duplicate query should be ignored");

        // Same query_id should be ignored
        let mut duplicate = FindCoordinatorQuery::new(origin);
        duplicate.query_id = query_id;
        let response3 = handler.handle_find_query(duplicate);
        assert!(response3.is_none(), "Same query_id should be ignored");
    }

    #[test]
    fn test_handle_expired_query() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        let origin = PeerId::new([2u8; 32]);
        let mut query = FindCoordinatorQuery::new(origin);

        // Make query expired
        query.created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_millis() as u64
            - 40_000; // 40 seconds ago

        let response = handler.handle_find_query(query);
        assert!(response.is_none(), "Expired query should be ignored");
    }

    #[test]
    fn test_handle_query_ttl_exhausted() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        let origin = PeerId::new([2u8; 32]);
        let mut query = FindCoordinatorQuery::new(origin);

        // Exhaust TTL
        query.ttl = 0;

        let response = handler.handle_find_query(query);
        assert!(response.is_none(), "Query with TTL=0 should be ignored");
    }

    #[test]
    fn test_prune() {
        let peer_id = PeerId::new([1u8; 32]);
        let handler = CoordinatorHandler::new(peer_id);

        // Add short-lived advert
        let signer = MlDsa65::new();
        let (pk, sk) = signer.generate_keypair().expect("keypair");

        let coord_peer = PeerId::new([2u8; 32]);
        let mut advert = CoordinatorAdvert::new(
            coord_peer,
            CoordinatorRoles::default(),
            vec![],
            NatClass::Eim,
            100, // 100ms validity (long enough to insert)
        );
        advert.sign(&sk).expect("signing");
        let inserted = handler.handle_advert(advert, &pk).expect("handle");
        assert!(inserted, "Advert should be inserted");

        assert_eq!(handler.cache().len(), 1);

        // Wait for expiry
        std::thread::sleep(std::time::Duration::from_millis(150));

        // Before pruning, len() should return 0 (filters valid adverts)
        assert_eq!(
            handler.cache().len(),
            0,
            "Expired adverts not counted by len()"
        );

        // Prune to actually remove from LRU
        let pruned = handler.prune();
        assert_eq!(pruned, 1, "Should have pruned 1 expired advert");
        assert_eq!(
            handler.cache().len(),
            0,
            "Cache should be empty after prune"
        );
    }
}
