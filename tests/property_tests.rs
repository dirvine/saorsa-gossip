//! Property-based tests for protocol invariants
//!
//! This module uses proptest to verify critical properties of the
//! Saorsa Gossip protocols that should hold under all circumstances.

use proptest::prelude::*;
use saorsa_gossip_crdt_sync::{DeltaCrdt, OrSet};
use saorsa_gossip_types::{PeerId, TopicId};

/// Strategy for generating PeerIds
fn peer_id_strategy() -> impl Strategy<Value = PeerId> {
    prop::array::uniform32(any::<u8>()).prop_map(PeerId::new)
}

/// Strategy for generating replica IDs (PeerId + u64 timestamp)
fn replica_id_strategy() -> impl Strategy<Value = (PeerId, u64)> {
    (peer_id_strategy(), 1u64..1000u64)
}

/// Strategy for generating OR-Set operations
#[derive(Debug, Clone)]
enum OrSetOp {
    Add(String, (PeerId, u64)),
    Remove(String),
}

fn orset_op_strategy() -> impl Strategy<Value = OrSetOp> {
    prop_oneof![
        ("[a-z]{3,8}", replica_id_strategy()).prop_map(|(s, rid)| OrSetOp::Add(s, rid)),
        "[a-z]{3,8}".prop_map(OrSetOp::Remove),
    ]
}

proptest! {
    /// Property: OR-Set eventual consistency
    /// Multiple replicas applying the same operations (in any order)
    /// should eventually converge to the same state
    #[test]
    fn prop_orset_eventual_consistency(
        operations in prop::collection::vec(orset_op_strategy(), 1..20)
    ) {
        // Create two replicas
        let mut replica1 = OrSet::<String>::new();
        let mut replica2 = OrSet::<String>::new();

        // Apply all operations to replica 1
        for op in &operations {
            match op {
                OrSetOp::Add(item, replica_id) => {
                    let _ = replica1.add(item.clone(), *replica_id);
                }
                OrSetOp::Remove(item) => {
                    let _ = replica1.remove(item);
                }
            }
        }

        // Apply same operations to replica 2 in reverse order
        for op in operations.iter().rev() {
            match op {
                OrSetOp::Add(item, replica_id) => {
                    let _ = replica2.add(item.clone(), *replica_id);
                }
                OrSetOp::Remove(item) => {
                    let _ = replica2.remove(item);
                }
            }
        }

        // Generate deltas and merge
        if let Some(delta1) = replica1.delta(0) {
            let _ = replica2.merge(&delta1);
        }
        if let Some(delta2) = replica2.delta(0) {
            let _ = replica1.merge(&delta2);
        }

        // Both replicas should have the same elements
        // (We can't directly compare OrSet equality, so we check contains for each item)
        let all_items: Vec<String> = operations.iter()
            .filter_map(|op| {
                match op {
                    OrSetOp::Add(item, _) => Some(item.clone()),
                    OrSetOp::Remove(item) => Some(item.clone()),
                }
            })
            .collect();

        for item in all_items {
            prop_assert_eq!(
                replica1.contains(&item),
                replica2.contains(&item),
                "Replicas diverged for item: {}", item
            );
        }
    }

    /// Property: OR-Set idempotence
    /// Adding the same element multiple times should be idempotent
    #[test]
    fn prop_orset_add_idempotence(
        item in "[a-z]{3,8}",
        replica_id in replica_id_strategy(),
        repeat_count in 1usize..10
    ) {
        let mut orset = OrSet::<String>::new();

        // Add the same item multiple times
        for _ in 0..repeat_count {
            let _ = orset.add(item.clone(), replica_id);
        }

        // Element should be present exactly once (logically)
        prop_assert!(orset.contains(&item));
    }

    /// Property: OR-Set commutativity
    /// The order of add operations shouldn't matter
    #[test]
    fn prop_orset_add_commutativity(
        item1 in "[a-z]{3,8}",
        item2 in "[a-z]{3,8}",
        rid1 in replica_id_strategy(),
        rid2 in replica_id_strategy()
    ) {
        let mut orset_a = OrSet::<String>::new();
        let mut orset_b = OrSet::<String>::new();

        // Apply operations in different order
        let _ = orset_a.add(item1.clone(), rid1);
        let _ = orset_a.add(item2.clone(), rid2);

        let _ = orset_b.add(item2.clone(), rid2);
        let _ = orset_b.add(item1.clone(), rid1);

        // Both should contain both items
        prop_assert!(orset_a.contains(&item1));
        prop_assert!(orset_a.contains(&item2));
        prop_assert!(orset_b.contains(&item1));
        prop_assert!(orset_b.contains(&item2));
    }

    /// Property: OR-Set remove after add
    /// Removing an element after adding it should remove it
    #[test]
    fn prop_orset_remove_after_add(
        item in "[a-z]{3,8}",
        replica_id in replica_id_strategy()
    ) {
        let mut orset = OrSet::<String>::new();

        // Add then remove
        let _ = orset.add(item.clone(), replica_id);
        let _ = orset.remove(&item);

        // Element should not be present
        prop_assert!(!orset.contains(&item));
    }

    /// Property: Topic ID determinism
    /// Creating a TopicId from the same bytes should always produce the same ID
    #[test]
    fn prop_topic_id_determinism(bytes in prop::array::uniform32(any::<u8>())) {
        let topic1 = TopicId::new(bytes);
        let topic2 = TopicId::new(bytes);

        prop_assert_eq!(topic1, topic2);
    }

    /// Property: PeerId determinism
    /// Creating a PeerId from the same bytes should always produce the same ID
    #[test]
    fn prop_peer_id_determinism(bytes in prop::array::uniform32(any::<u8>())) {
        let peer1 = PeerId::new(bytes);
        let peer2 = PeerId::new(bytes);

        prop_assert_eq!(peer1, peer2);
    }

    /// Property: Message ID consistency
    /// Same inputs to message ID calculation should produce same output
    #[test]
    fn prop_message_id_consistency(
        topic_bytes in prop::array::uniform32(any::<u8>()),
        sequence in any::<u64>(),
        sender_bytes in prop::array::uniform32(any::<u8>()),
        content_hash in prop::array::uniform32(any::<u8>())
    ) {
        use saorsa_gossip_types::MessageHeader;

        let topic = TopicId::new(topic_bytes);
        let sender = PeerId::new(sender_bytes);

        let msg_id1 = MessageHeader::calculate_msg_id(&topic, sequence, &sender, &content_hash);
        let msg_id2 = MessageHeader::calculate_msg_id(&topic, sequence, &sender, &content_hash);

        prop_assert_eq!(msg_id1, msg_id2);
    }
}

#[cfg(test)]
mod standard_tests {
    use super::*;

    #[test]
    fn test_orset_basic_operations() {
        let mut orset = OrSet::<String>::new();
        let replica_id = (PeerId::new([1u8; 32]), 1);

        // Add an item
        assert!(orset.add("test".to_string(), replica_id).is_ok());
        assert!(orset.contains(&"test".to_string()));

        // Remove the item
        assert!(orset.remove(&"test".to_string()).is_ok());
        assert!(!orset.contains(&"test".to_string()));
    }

    #[test]
    fn test_orset_concurrent_adds() {
        let mut orset1 = OrSet::<String>::new();
        let mut orset2 = OrSet::<String>::new();

        let replica_id1 = (PeerId::new([1u8; 32]), 1);
        let replica_id2 = (PeerId::new([2u8; 32]), 1);

        // Concurrent adds on different replicas
        assert!(orset1.add("item1".to_string(), replica_id1).is_ok());
        assert!(orset2.add("item2".to_string(), replica_id2).is_ok());

        // Merge deltas
        if let Some(delta1) = orset1.delta(0) {
            assert!(orset2.merge(&delta1).is_ok());
        }
        if let Some(delta2) = orset2.delta(0) {
            assert!(orset1.merge(&delta2).is_ok());
        }

        // Both replicas should have both items
        assert!(orset1.contains(&"item1".to_string()));
        assert!(orset1.contains(&"item2".to_string()));
        assert!(orset2.contains(&"item1".to_string()));
        assert!(orset2.contains(&"item2".to_string()));
    }

    #[test]
    fn test_orset_concurrent_add_remove() {
        let mut orset1 = OrSet::<String>::new();
        let mut orset2 = OrSet::<String>::new();

        let replica_id1 = (PeerId::new([1u8; 32]), 1);
        let replica_id2 = (PeerId::new([2u8; 32]), 1);

        // Add on replica1
        assert!(orset1.add("item".to_string(), replica_id1).is_ok());

        // Sync to replica2
        if let Some(delta) = orset1.delta(0) {
            assert!(orset2.merge(&delta).is_ok());
        }

        // Concurrent operations: remove on replica1, add again with new tag on replica2
        assert!(orset1.remove(&"item".to_string()).is_ok());
        assert!(orset2.add("item".to_string(), replica_id2).is_ok());

        // Merge back
        if let Some(delta1) = orset1.delta(1) {
            assert!(orset2.merge(&delta1).is_ok());
        }
        if let Some(delta2) = orset2.delta(1) {
            assert!(orset1.merge(&delta2).is_ok());
        }

        // Item should still be present (add-wins semantics)
        assert!(orset1.contains(&"item".to_string()));
        assert!(orset2.contains(&"item".to_string()));
    }
}
