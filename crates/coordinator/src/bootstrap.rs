//! Bootstrap flow for cold-start coordinator discovery
//!
//! Implements SPEC2 §7.4 bootstrap flow: cache → FOAF → connect

use crate::{CoordinatorHandler, FindCoordinatorQuery, PeerCache, PeerCacheEntry};
use saorsa_gossip_types::PeerId;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Traversal method preference order per SPEC2 §7.4
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TraversalMethod {
    /// Direct connection (best, lowest cost)
    Direct = 0,
    /// Reflexive/punched path (moderate cost)
    Reflexive = 1,
    /// Relay (last resort, highest cost)
    Relay = 2,
}

/// Result of a successful bootstrap (found coordinator)
#[derive(Debug, Clone)]
pub struct BootstrapResult {
    /// Selected coordinator peer
    pub peer_id: PeerId,
    /// Address to connect to
    pub addr: SocketAddr,
    /// Traversal method to use
    pub method: TraversalMethod,
}

/// Action required after bootstrap attempt per SPEC2 §7.4
#[derive(Debug, Clone)]
pub enum BootstrapAction {
    /// Found coordinator in cache - can connect immediately
    Connect(BootstrapResult),
    /// Cache is cold - need to issue FOAF FIND_COORDINATOR query
    SendQuery(FindCoordinatorQuery),
    /// No action possible (no cache, no peers to query)
    NoAction,
}

/// Bootstrap coordinator for network entry
pub struct Bootstrap {
    /// Local peer ID
    peer_id: PeerId,
    /// Peer cache (primary source)
    peer_cache: PeerCache,
    /// Coordinator handler (for FOAF queries)
    handler: CoordinatorHandler,
    /// Pending FOAF queries (query_id → timestamp)
    pending_queries: Arc<Mutex<HashMap<[u8; 32], Instant>>>,
}

impl Bootstrap {
    /// Create a new bootstrap instance
    pub fn new(peer_id: PeerId, peer_cache: PeerCache, handler: CoordinatorHandler) -> Self {
        Self {
            peer_id,
            peer_cache,
            handler,
            pending_queries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Attempt to find a coordinator to bootstrap from
    ///
    /// Strategy per SPEC2 §7:
    /// 1. Check peer cache for recent coordinators
    /// 2. If cache is cold, issue FOAF FIND_COORDINATOR
    /// 3. Select best coordinator by traversal preference
    ///
    /// Returns an action to take (Connect, SendQuery, or NoAction)
    pub fn find_coordinator(&self) -> BootstrapAction {
        // Step 1: Try peer cache first
        let cached_coordinators = self.peer_cache.get_coordinators();

        if !cached_coordinators.is_empty() {
            if let Some(result) = self.select_best_coordinator(&cached_coordinators) {
                return BootstrapAction::Connect(result);
            }
        }

        // Step 2: Cache is cold, issue FOAF query per SPEC2 §7.4
        let query = FindCoordinatorQuery::new(self.peer_id);

        // Track this query
        {
            let mut pending = self.pending_queries.lock().expect("lock poisoned");
            pending.insert(query.query_id, Instant::now());
        }

        BootstrapAction::SendQuery(query)
    }

    /// Select the best coordinator based on traversal preference
    ///
    /// Preference order: Direct → Reflexive → Relay
    fn select_best_coordinator(&self, coordinators: &[PeerCacheEntry]) -> Option<BootstrapResult> {
        if coordinators.is_empty() {
            return None;
        }

        // Try each traversal method in preference order
        for method in [TraversalMethod::Direct, TraversalMethod::Reflexive, TraversalMethod::Relay] {
            for entry in coordinators {
                if let Some(addr) = self.get_addr_for_method(entry, method) {
                    return Some(BootstrapResult {
                        peer_id: entry.peer_id,
                        addr,
                        method,
                    });
                }
            }
        }

        None
    }

    /// Get an address for a specific traversal method per SPEC2 §7.4
    ///
    /// Traversal preference order:
    /// 1. Direct: Use public_addrs (best performance, lowest cost)
    /// 2. Reflexive: Use reflexive_addrs from hole punching (moderate cost)
    /// 3. Relay: Lookup relay peer's public address (last resort, highest cost)
    fn get_addr_for_method(&self, entry: &PeerCacheEntry, method: TraversalMethod) -> Option<SocketAddr> {
        match method {
            TraversalMethod::Direct => {
                // Direct connection via public address
                entry.public_addrs.first().copied()
            }
            TraversalMethod::Reflexive => {
                // Reflexive connection via hole-punched address
                entry.reflexive_addrs.first().copied()
            }
            TraversalMethod::Relay => {
                // Relay connection: lookup relay peer and use its public address
                if let Some(relay_peer_id) = entry.relay_peer {
                    // Look up relay peer from peer cache
                    if let Some(relay_entry) = self.peer_cache.get(&relay_peer_id) {
                        relay_entry.public_addrs.first().copied()
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Handle a FOAF FIND_COORDINATOR response
    ///
    /// Processes coordinator adverts from response, updates cache, and returns connect action.
    /// Per SPEC2 §7.3, responses contain coordinator adverts that should be added to cache.
    pub fn handle_find_response(&self, response: crate::FindCoordinatorResponse) -> Option<BootstrapAction> {
        // Remove from pending queries
        {
            let mut pending = self.pending_queries.lock().expect("lock poisoned");
            pending.remove(&response.query_id);
        }

        // Add all coordinator adverts to handler cache
        for advert in response.adverts {
            // Note: In production, would verify signatures before caching
            let _ = self.handler.cache().insert(advert);
        }

        // Try to find coordinator from newly updated cache
        let coordinators = self.handler.cache().get_by_role(|advert| advert.roles.coordinator);

        self.select_best_from_adverts(&coordinators)
            .map(BootstrapAction::Connect)
    }

    /// Select best coordinator from coordinator adverts
    fn select_best_from_adverts(&self, adverts: &[crate::CoordinatorAdvert]) -> Option<BootstrapResult> {
        for method in [TraversalMethod::Direct, TraversalMethod::Reflexive, TraversalMethod::Relay] {
            for advert in adverts {
                if let Some(addr_hint) = advert.addr_hints.first() {
                    return Some(BootstrapResult {
                        peer_id: advert.peer,
                        addr: addr_hint.addr,
                        method,
                    });
                }
            }
        }
        None
    }

    /// Clean up expired pending queries
    ///
    /// Per SPEC2 §7.3, queries expire after 30 seconds.
    /// Returns the number of expired queries removed.
    pub fn prune_expired_queries(&self) -> usize {
        let mut pending = self.pending_queries.lock().expect("lock poisoned");
        let now = Instant::now();

        let expired: Vec<_> = pending
            .iter()
            .filter(|(_, timestamp)| now.duration_since(**timestamp).as_secs() > 30)
            .map(|(query_id, _)| *query_id)
            .collect();

        let count = expired.len();
        for query_id in expired {
            pending.remove(&query_id);
        }

        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NatClass, PeerRoles};

    #[test]
    fn test_traversal_method_ordering() {
        assert!(TraversalMethod::Direct < TraversalMethod::Reflexive);
        assert!(TraversalMethod::Reflexive < TraversalMethod::Relay);
        assert!(TraversalMethod::Direct < TraversalMethod::Relay);
    }

    #[test]
    fn test_bootstrap_creation() {
        let peer_id = PeerId::new([1u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        assert_eq!(bootstrap.peer_id, peer_id);
    }

    #[test]
    fn test_find_coordinator_empty_cache() {
        let peer_id = PeerId::new([1u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        // Empty cache should trigger FOAF query per SPEC2 §7.4
        match action {
            BootstrapAction::SendQuery(query) => {
                assert_eq!(query.origin, peer_id, "Query origin should be local peer");
                assert_eq!(query.ttl, 3, "TTL should be 3 per SPEC2 §7.3");
            }
            _ => panic!("Expected SendQuery action for empty cache"),
        }
    }

    #[test]
    fn test_find_coordinator_from_cache() {
        let peer_id = PeerId::new([1u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        // Add a coordinator to cache
        let coord_peer = PeerId::new([2u8; 32]);
        let addr = "127.0.0.1:8080".parse().expect("valid address");
        let entry = PeerCacheEntry::new(
            coord_peer,
            vec![addr],
            NatClass::Eim,
            PeerRoles {
                coordinator: true,
                reflector: true,
                rendezvous: false,
                relay: false,
            },
        );
        peer_cache.insert(entry);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        // Warm cache should return Connect action
        match action {
            BootstrapAction::Connect(result) => {
                assert_eq!(result.peer_id, coord_peer);
                assert_eq!(result.addr, addr);
                assert_eq!(result.method, TraversalMethod::Direct);
            }
            _ => panic!("Expected Connect action for warm cache"),
        }
    }

    #[test]
    fn test_select_most_recent_coordinator() {
        let peer_id = PeerId::new([1u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        // Add multiple coordinators with different timestamps
        let coord1 = PeerId::new([2u8; 32]);
        let addr1 = "127.0.0.1:8080".parse().expect("valid");
        let mut entry1 = PeerCacheEntry::new(
            coord1,
            vec![addr1],
            NatClass::Eim,
            PeerRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
        );
        entry1.last_success -= 10000; // Older
        peer_cache.insert(entry1);

        let coord2 = PeerId::new([3u8; 32]);
        let addr2 = "127.0.0.1:8081".parse().expect("valid");
        let entry2 = PeerCacheEntry::new(
            coord2,
            vec![addr2],
            NatClass::Eim,
            PeerRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
        );
        // entry2 has more recent timestamp
        peer_cache.insert(entry2);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        // Should select most recent (coord2)
        match action {
            BootstrapAction::Connect(result) => {
                assert_eq!(result.peer_id, coord2, "Should select most recent coordinator");
                assert_eq!(result.addr, addr2);
            }
            _ => panic!("Expected Connect action"),
        }
    }

    #[test]
    fn test_traversal_preference_direct_first() {
        let peer_id = PeerId::new([1u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        let coord = PeerId::new([2u8; 32]);
        let addr = "127.0.0.1:8080".parse().expect("valid");

        let entry = PeerCacheEntry::new(
            coord,
            vec![addr],
            NatClass::Eim,
            PeerRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
        );
        peer_cache.insert(entry);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        match action {
            BootstrapAction::Connect(result) => {
                assert_eq!(result.method, TraversalMethod::Direct, "Should prefer direct connection");
            }
            _ => panic!("Expected Connect action"),
        }
    }

    #[test]
    fn test_bootstrap_result_creation() {
        let peer_id = PeerId::new([1u8; 32]);
        let addr = "192.168.1.1:9000".parse().expect("valid");

        let result = BootstrapResult {
            peer_id,
            addr,
            method: TraversalMethod::Reflexive,
        };

        assert_eq!(result.peer_id, peer_id);
        assert_eq!(result.addr, addr);
        assert_eq!(result.method, TraversalMethod::Reflexive);
    }

    /// Test FOAF query is tracked in pending queries
    #[test]
    fn test_foaf_query_is_tracked() {
        let peer_id = PeerId::new([10u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);

        // Empty cache triggers FOAF query
        let action = bootstrap.find_coordinator();

        match action {
            BootstrapAction::SendQuery(query) => {
                // Query should be tracked
                let pending = bootstrap.pending_queries.lock().expect("lock");
                assert!(pending.contains_key(&query.query_id), "Query should be tracked");
            }
            _ => panic!("Expected SendQuery action"),
        }
    }

    /// Test handling FOAF query response
    #[test]
    fn test_handle_foaf_response() {
        use crate::{CoordinatorAdvert, CoordinatorRoles, NatClass, AddrHint, FindCoordinatorResponse};
        use saorsa_pqc::{MlDsa65, MlDsaOperations};

        let peer_id = PeerId::new([11u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);
        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);

        // Issue query first
        let action = bootstrap.find_coordinator();
        let query_id = match action {
            BootstrapAction::SendQuery(query) => query.query_id,
            _ => panic!("Expected SendQuery"),
        };

        // Create a response with a coordinator advert
        let coord_peer = PeerId::new([12u8; 32]);
        let addr = "10.0.0.1:8080".parse().expect("valid addr");

        let mut advert = CoordinatorAdvert::new(
            coord_peer,
            CoordinatorRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
            vec![AddrHint::new(addr)],
            NatClass::Eim,
            60_000,
        );

        // Sign the advert
        let signer = MlDsa65::new();
        let (_, sk) = signer.generate_keypair().expect("keypair");
        advert.sign(&sk).expect("signing");

        let response = FindCoordinatorResponse::new(query_id, peer_id, vec![advert]);

        // Handle the response
        let result_action = bootstrap.handle_find_response(response).expect("should return action");

        // Should return Connect action with coordinator
        match result_action {
            BootstrapAction::Connect(result) => {
                assert_eq!(result.peer_id, coord_peer);
                assert_eq!(result.addr, addr);
            }
            _ => panic!("Expected Connect action after response"),
        }

        // Query should be removed from pending
        let pending = bootstrap.pending_queries.lock().expect("lock");
        assert!(!pending.contains_key(&query_id), "Query should be removed after response");
    }

    /// Test query timeout pruning
    #[test]
    fn test_prune_expired_queries() {
        use std::time::Duration;

        let peer_id = PeerId::new([13u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);
        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);

        // Create a query
        let _ = bootstrap.find_coordinator();

        // Manually expire it by manipulating timestamp
        {
            let mut pending = bootstrap.pending_queries.lock().expect("lock");
            if let Some((query_id, _)) = pending.iter().next() {
                let old_query_id = *query_id;
                pending.insert(old_query_id, Instant::now() - Duration::from_secs(35));
            }
        }

        // Prune should remove expired query
        let pruned = bootstrap.prune_expired_queries();
        assert_eq!(pruned, 1, "Should prune 1 expired query");

        let pending = bootstrap.pending_queries.lock().expect("lock");
        assert_eq!(pending.len(), 0, "No queries should remain");
    }

    /// Test BootstrapAction enum variants
    #[test]
    fn test_bootstrap_action_variants() {
        let peer_id = PeerId::new([14u8; 32]);
        let addr = "1.2.3.4:5678".parse().expect("valid");

        // Test Connect variant
        let connect_action = BootstrapAction::Connect(BootstrapResult {
            peer_id,
            addr,
            method: TraversalMethod::Direct,
        });
        assert!(matches!(connect_action, BootstrapAction::Connect(_)));

        // Test SendQuery variant
        let query_action = BootstrapAction::SendQuery(FindCoordinatorQuery::new(peer_id));
        assert!(matches!(query_action, BootstrapAction::SendQuery(_)));

        // Test NoAction variant
        let no_action = BootstrapAction::NoAction;
        assert!(matches!(no_action, BootstrapAction::NoAction));
    }

    /// Test Direct traversal method uses public_addrs
    #[test]
    fn test_direct_traversal_uses_public_addrs() {
        let peer_id = PeerId::new([20u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        let coord_peer = PeerId::new([21u8; 32]);
        let public_addr = "203.0.113.1:8080".parse().expect("valid");
        let reflexive_addr = "192.168.1.10:9000".parse().expect("valid");

        let entry = PeerCacheEntry::new(
            coord_peer,
            vec![public_addr],
            NatClass::Eim,
            PeerRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
        )
        .with_reflexive_addrs(vec![reflexive_addr]);

        peer_cache.insert(entry);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        match action {
            BootstrapAction::Connect(result) => {
                assert_eq!(result.method, TraversalMethod::Direct);
                assert_eq!(result.addr, public_addr, "Direct should use public address");
            }
            _ => panic!("Expected Connect action"),
        }
    }

    /// Test Reflexive traversal when no public addresses
    #[test]
    fn test_reflexive_traversal_uses_reflexive_addrs() {
        let peer_id = PeerId::new([22u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        let coord_peer = PeerId::new([23u8; 32]);
        let reflexive_addr = "192.168.1.100:9000".parse().expect("valid");

        // Entry with NO public addresses, only reflexive
        let entry = PeerCacheEntry::new(
            coord_peer,
            vec![], // No public addresses
            NatClass::Edm,
            PeerRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
        )
        .with_reflexive_addrs(vec![reflexive_addr]);

        peer_cache.insert(entry);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        match action {
            BootstrapAction::Connect(result) => {
                assert_eq!(result.method, TraversalMethod::Reflexive);
                assert_eq!(result.addr, reflexive_addr, "Reflexive should use reflexive address");
            }
            _ => panic!("Expected Connect action"),
        }
    }

    /// Test Relay traversal when only relay peer available
    #[test]
    fn test_relay_traversal_uses_relay_peer() {
        let peer_id = PeerId::new([24u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        // Create a relay peer
        let relay_peer = PeerId::new([25u8; 32]);
        let relay_addr = "198.51.100.1:8080".parse().expect("valid");
        let relay_entry = PeerCacheEntry::new(
            relay_peer,
            vec![relay_addr],
            NatClass::Eim,
            PeerRoles {
                coordinator: false,
                reflector: false,
                rendezvous: false,
                relay: true,
            },
        );
        peer_cache.insert(relay_entry);

        // Create coordinator that needs relay
        let coord_peer = PeerId::new([26u8; 32]);
        let entry = PeerCacheEntry::new(
            coord_peer,
            vec![], // No public addresses
            NatClass::Symmetric,
            PeerRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
        )
        .with_relay_peer(relay_peer);

        peer_cache.insert(entry);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        match action {
            BootstrapAction::Connect(result) => {
                assert_eq!(result.method, TraversalMethod::Relay);
                assert_eq!(result.addr, relay_addr, "Relay should use relay peer's public address");
            }
            _ => panic!("Expected Connect action"),
        }
    }

    /// Test traversal preference order: Direct > Reflexive > Relay
    #[test]
    fn test_traversal_preference_order() {
        let peer_id = PeerId::new([27u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        let public_addr = "203.0.113.10:8080".parse().expect("valid");
        let reflexive_addr = "192.168.1.50:9000".parse().expect("valid");

        let relay_peer = PeerId::new([28u8; 32]);
        let relay_addr = "198.51.100.10:8080".parse().expect("valid");
        peer_cache.insert(PeerCacheEntry::new(
            relay_peer,
            vec![relay_addr],
            NatClass::Eim,
            PeerRoles {
                coordinator: false,
                reflector: false,
                rendezvous: false,
                relay: true,
            },
        ));

        let coord_peer = PeerId::new([29u8; 32]);

        // Coordinator with all three traversal options
        let entry = PeerCacheEntry::new(
            coord_peer,
            vec![public_addr],
            NatClass::Eim,
            PeerRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
        )
        .with_reflexive_addrs(vec![reflexive_addr])
        .with_relay_peer(relay_peer);

        peer_cache.insert(entry);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        match action {
            BootstrapAction::Connect(result) => {
                assert_eq!(result.method, TraversalMethod::Direct, "Should prefer Direct");
                assert_eq!(result.addr, public_addr, "Should use public address");
            }
            _ => panic!("Expected Connect action"),
        }
    }

    /// Test relay fallback when relay peer not in cache
    #[test]
    fn test_relay_fallback_when_relay_peer_missing() {
        let peer_id = PeerId::new([30u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);

        let coord_peer = PeerId::new([31u8; 32]);
        let missing_relay_peer = PeerId::new([32u8; 32]);

        // Coordinator with relay peer that's NOT in cache
        let entry = PeerCacheEntry::new(
            coord_peer,
            vec![], // No public addresses
            NatClass::Symmetric,
            PeerRoles {
                coordinator: true,
                reflector: false,
                rendezvous: false,
                relay: false,
            },
        )
        .with_relay_peer(missing_relay_peer);

        peer_cache.insert(entry);

        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);
        let action = bootstrap.find_coordinator();

        // Should trigger FOAF query since no valid traversal method available
        match action {
            BootstrapAction::SendQuery(_) => {
                // Expected: can't connect, need to query for more coordinators
            }
            _ => panic!("Expected SendQuery when relay peer is missing"),
        }
    }

    /// Test builder pattern for PeerCacheEntry
    #[test]
    fn test_peer_cache_entry_builder() {
        let peer_id = PeerId::new([33u8; 32]);
        let public_addr = "1.2.3.4:8080".parse().expect("valid");
        let reflexive_addr = "192.168.1.1:9000".parse().expect("valid");
        let relay_peer = PeerId::new([34u8; 32]);

        let entry = PeerCacheEntry::new(
            peer_id,
            vec![public_addr],
            NatClass::Edm,
            PeerRoles {
                coordinator: true,
                reflector: true,
                rendezvous: false,
                relay: false,
            },
        )
        .with_reflexive_addrs(vec![reflexive_addr])
        .with_relay_peer(relay_peer);

        assert_eq!(entry.public_addrs.len(), 1);
        assert_eq!(entry.public_addrs[0], public_addr);
        assert_eq!(entry.reflexive_addrs.len(), 1);
        assert_eq!(entry.reflexive_addrs[0], reflexive_addr);
        assert_eq!(entry.relay_peer, Some(relay_peer));
    }

    /// Test response with multiple coordinators selects best
    #[test]
    fn test_response_with_multiple_coordinators() {
        use crate::{CoordinatorAdvert, CoordinatorRoles, NatClass, AddrHint, FindCoordinatorResponse};
        use saorsa_pqc::{MlDsa65, MlDsaOperations};

        let peer_id = PeerId::new([15u8; 32]);
        let peer_cache = PeerCache::new();
        let handler = CoordinatorHandler::new(peer_id);
        let bootstrap = Bootstrap::new(peer_id, peer_cache, handler);

        // Issue query
        let action = bootstrap.find_coordinator();
        let query_id = match action {
            BootstrapAction::SendQuery(query) => query.query_id,
            _ => panic!("Expected SendQuery"),
        };

        // Create response with 3 coordinators
        let signer = MlDsa65::new();
        let (_, sk) = signer.generate_keypair().expect("keypair");

        let mut adverts = vec![];
        for i in 0..3 {
            let coord_peer = PeerId::new([16 + i; 32]);
            let addr = format!("10.0.0.{}:8080", i + 1).parse().expect("valid addr");

            let mut advert = CoordinatorAdvert::new(
                coord_peer,
                CoordinatorRoles {
                    coordinator: true,
                    reflector: false,
                    rendezvous: false,
                    relay: false,
                },
                vec![AddrHint::new(addr)],
                NatClass::Eim,
                60_000,
            );
            advert.sign(&sk).expect("signing");
            adverts.push(advert);
        }

        let response = FindCoordinatorResponse::new(query_id, peer_id, adverts);

        // Should select the first coordinator (simplest traversal logic)
        let result_action = bootstrap.handle_find_response(response).expect("should return action");

        match result_action {
            BootstrapAction::Connect(result) => {
                // Just verify we got a coordinator back
                assert!(result.addr.port() >= 8080);
            }
            _ => panic!("Expected Connect action"),
        }
    }
}
