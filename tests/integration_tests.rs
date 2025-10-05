//! Integration tests for Saorsa Gossip overlay network
//!
//! Tests the interaction between multiple components to ensure
//! they work together correctly in a production-like scenario.

use anyhow::Result;
use saorsa_gossip_coordinator::{Coordinator, CoordinatorConfig};
use saorsa_gossip_identity::{Identity, Keystore};
use saorsa_gossip_membership::{HyParViewMembership, Membership};
use saorsa_gossip_pubsub::{PlumtreePubSub, PubSub};
use saorsa_gossip_types::{MessageHeader, MessageKind, PeerId, TopicId};
use std::time::Duration;
use tokio::time::sleep;

/// Test end-to-end message flow through the gossip overlay
#[tokio::test]
async fn test_end_to_end_message_flow() -> Result<()> {
    // Create identities for two peers
    let identity1 = Identity::generate()?;
    let identity2 = Identity::generate()?;
    
    let peer_id1 = PeerId::from_pubkey(&identity1.public_key());
    let peer_id2 = PeerId::from_pubkey(&identity2.public_key());
    
    // Create a shared topic
    let topic = TopicId::new([42u8; 32]);
    
    // Initialize membership layers
    let membership1 = HyParViewMembership::new(peer_id1);
    let membership2 = HyParViewMembership::new(peer_id2);
    
    // Initialize pub/sub layers
    let pubsub1 = PlumtreePubSub::new();
    let pubsub2 = PlumtreePubSub::new();
    
    // Subscribe to topic
    let mut rx1 = pubsub1.subscribe(topic);
    let mut rx2 = pubsub2.subscribe(topic);
    
    // Simulate peer discovery (normally done via coordinator)
    membership1.add_peer(peer_id2, vec!["127.0.0.1:8081".to_string()]).await?;
    membership2.add_peer(peer_id1, vec!["127.0.0.1:8080".to_string()]).await?;
    
    // Publish a message from peer1
    let message = b"Hello from peer1!";
    pubsub1.publish(topic, bytes::Bytes::copy_from_slice(message)).await?;
    
    // Give some time for message propagation
    sleep(Duration::from_millis(100)).await;
    
    // Both peers should receive the message
    let received1 = rx1.try_recv()?;
    let received2 = rx2.try_recv()?;
    
    assert_eq!(received1.1, message);
    assert_eq!(received2.1, message);
    
    Ok(())
}

/// Test coordinator bootstrap and peer discovery
#[tokio::test]
async fn test_coordinator_bootstrap() -> Result<()> {
    // Create coordinator identity
    let coordinator_identity = Identity::generate()?;
    let coordinator_config = CoordinatorConfig {
        bind_addr: "127.0.0.1:7000".to_string(),
        roles: vec!["coordinator".to_string(), "reflector".to_string()],
        identity_path: None,
        publish_interval: Duration::from_secs(60),
    };
    
    // Start coordinator
    let coordinator = Coordinator::new(coordinator_identity, coordinator_config);
    let coordinator_handle = tokio::spawn(async move {
        coordinator.start().await
    });
    
    // Give coordinator time to start
    sleep(Duration::from_millis(100)).await;
    
    // Create peer identity
    let peer_identity = Identity::generate()?;
    
    // Connect to coordinator for bootstrap
    let bootstrap_addrs = vec!["127.0.0.1:7000".to_string()];
    let discovered_peers = peer_identity
        .bootstrap_from_coordinators(&bootstrap_addrs)
        .await?;
    
    // Should discover at least the coordinator
    assert!(!discovered_peers.is_empty());
    
    // Shutdown coordinator
    coordinator_handle.abort();
    
    Ok(())
}

/// Test FOAF (Friend-of-a-Friend) query propagation
#[tokio::test]
async fn test_foaf_query_propagation() -> Result<()> {
    // Create three peers in a line: A -> B -> C
    let identity_a = Identity::generate()?;
    let identity_b = Identity::generate()?;
    let identity_c = Identity::generate()?;
    
    let peer_a = PeerId::from_pubkey(&identity_a.public_key());
    let peer_b = PeerId::from_pubkey(&identity_b.public_key());
    let peer_c = PeerId::from_pubkey(&identity_c.public_key());
    
    // Initialize membership
    let membership_a = HyParViewMembership::new(peer_a);
    let membership_b = HyParViewMembership::new(peer_b);
    let membership_c = HyParViewMembership::new(peer_c);
    
    // Connect peers: A knows B, B knows C
    membership_a.add_peer(peer_b, vec!["127.0.0.1:8081".to_string()]).await?;
    membership_b.add_peer(peer_a, vec!["127.0.0.1:8080".to_string()]).await?;
    membership_b.add_peer(peer_c, vec!["127.0.0.1:8082".to_string()]).await?;
    membership_c.add_peer(peer_b, vec!["127.0.0.1:8081".to_string()]).await?;
    
    // Peer A searches for Peer C using FOAF
    let target_four_words = "velvet-quantum-nexus-dawn".to_string();
    let query_results = membership_a.foaf_search(target_four_words, 2).await?;
    
    // Should find Peer C through B
    assert!(!query_results.is_empty());
    
    Ok(())
}

/// Test CRDT synchronization between peers
#[tokio::test]
async fn test_crdt_synchronization() -> Result<()> {
    use saorsa_gossip_crdt_sync::{OrSet, DeltaCrdt};
    
    // Create two OR-Set instances
    let mut orset1 = OrSet::<String>::new();
    let mut orset2 = OrSet::<String>::new();
    
    // Peer 1 adds an element
    let peer1_id = PeerId::new([1u8; 32]);
    orset1.add("item1".to_string(), (peer1_id, 1)).await?;
    
    // Generate delta from peer 1
    let delta1 = orset1.delta(0).unwrap();
    
    // Peer 2 merges the delta
    orset2.merge(&delta1).await?;
    
    // Both should have the element
    assert!(orset1.contains("item1"));
    assert!(orset2.contains("item1"));
    
    // Peer 2 adds another element
    let peer2_id = PeerId::new([2u8; 32]);
    orset2.add("item2".to_string(), (peer2_id, 1)).await?;
    
    // Sync back to peer 1
    let delta2 = orset2.delta(0).unwrap();
    orset1.merge(&delta2).await?;
    
    // Both should have both elements
    assert!(orset1.contains("item1"));
    assert!(orset1.contains("item2"));
    assert!(orset2.contains("item1"));
    assert!(orset2.contains("item2"));
    
    Ok(())
}

/// Test message signing and verification across components
#[tokio::test]
async fn test_message_signing_verification() -> Result<()> {
    use saorsa_gossip_identity::Signer;
    
    // Create identity
    let identity = Identity::generate()?;
    let peer_id = PeerId::from_pubkey(&identity.public_key());
    
    // Create message header
    let topic = TopicId::new([42u8; 32]);
    let mut header = MessageHeader::new(topic, MessageKind::Eager, 10);
    header.msg_id = MessageHeader::calculate_msg_id(
        &topic,
        12345,
        &peer_id,
        &[1u8; 32],
    );
    
    // Sign the header
    let header_bytes = bincode::serialize(&header)?;
    let signature = identity.sign(&header_bytes)?;
    
    // Verify signature (in real scenario, other peers would do this)
    let is_valid = identity.verify(&header_bytes, &signature)?;
    assert!(is_valid);
    
    Ok(())
}

/// Test presence beacon broadcasting
#[tokio::test]
async fn test_presence_beacon() -> Result<()> {
    use saorsa_gossip_presence::{PresenceManager, PresenceConfig};
    use saorsa_gossip_groups::GroupManager;
    
    // Create identity and group
    let identity = Identity::generate()?;
    let group_manager = GroupManager::new();
    let group_id = group_manager.create_group("test-group").await?;
    
    // Initialize presence manager
    let presence_config = PresenceConfig {
        beacon_interval: Duration::from_secs(10),
        ttl: Duration::from_secs(900), // 15 minutes
    };
    let presence_manager = PresenceManager::new(identity.clone(), presence_config);
    
    // Start broadcasting presence
    presence_manager.start_beaconing(group_id).await?;
    
    // Give time for beacon to be created
    sleep(Duration::from_millis(100)).await;
    
    // Check that presence record exists
    let presence_records = presence_manager.get_local_presence().await?;
    assert!(!presence_records.is_empty());
    
    // Verify presence record structure
    let record = &presence_records[0];
    assert_eq!(record.addr_hints.len(), 0); // No address hints initially
    assert!(!record.is_expired());
    
    Ok(())
}

/// Test rendezvous shard discovery
#[tokio::test]
async fn test_rendezvous_discovery() -> Result<()> {
    use saorsa_gossip_rendezvous::{RendezvousClient, RendezvousConfig};
    
    // Create rendezvous client
    let config = RendezvousConfig {
        shard_count: 16,
        replication_factor: 3,
    };
    let client = RendezvousClient::new(config);
    
    // Create identity
    let identity = Identity::generate()?;
    let peer_id = PeerId::from_pubkey(&identity.public_key());
    
    // Publish capability to rendezvous
    let capability = "chat:general".to_string();
    let addr_hints = vec!["127.0.0.1:8080".to_string()];
    
    client.publish_capability(peer_id, capability.clone(), addr_hints.clone()).await?;
    
    // Query for the capability
    let results = client.query_capability(&capability).await?;
    
    // Should find the published peer
    assert!(!results.is_empty());
    assert_eq!(results[0].peer_id, peer_id);
    assert_eq!(results[0].addr_hints, addr_hints);
    
    Ok(())
}

/// Test multi-hop message propagation
#[tokio::test]
async fn test_multi_hop_propagation() -> Result<()> {
    // Create 5 peers in a line topology
    let mut peers = Vec::new();
    let mut memberships = Vec::new();
    let mut pubsubs = Vec::new();
    
    for i in 0..5 {
        let identity = Identity::generate()?;
        let peer_id = PeerId::from_pubkey(&identity.public_key());
        peers.push((identity, peer_id));
        
        let membership = HyParViewMembership::new(peer_id);
        memberships.push(membership);
        
        let pubsub = PlumtreePubSub::new();
        pubsubs.push(pubsub);
    }
    
    // Connect peers in a line: 0-1-2-3-4
    for i in 0..4 {
        let peer_i = peers[i].1;
        let peer_j = peers[i + 1].1;
        let addr_i = format!("127.0.0.1:{}", 8080 + i);
        let addr_j = format!("127.0.0.1:{}", 8080 + i + 1);
        
        memberships[i].add_peer(peer_j, vec![addr_j]).await?;
        memberships[i + 1].add_peer(peer_i, vec![addr_i]).await?;
    }
    
    // Subscribe all peers to topic
    let topic = TopicId::new([99u8; 32]);
    let mut receivers = Vec::new();
    for pubsub in &pubsubs {
        receivers.push(pubsub.subscribe(topic));
    }
    
    // Publish from peer 0
    let message = b"Multi-hop test message";
    pubsubs[0].publish(topic, bytes::Bytes::copy_from_slice(message)).await?;
    
    // Give time for propagation
    sleep(Duration::from_millis(500)).await;
    
    // All peers should receive the message
    for (i, receiver) in receivers.into_iter().enumerate() {
        let received = receiver.try_recv()?;
        assert_eq!(received.1, message, "Peer {} did not receive message", i);
    }
    
    Ok(())
}