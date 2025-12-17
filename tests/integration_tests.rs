//! Integration tests for Saorsa Gossip overlay network
//!
//! Tests the interaction between multiple components to ensure
//! they work together correctly in a production-like scenario.

/// Test CRDT synchronization between peers
#[tokio::test]
async fn test_crdt_synchronization() -> Result<(), Box<dyn std::error::Error>> {
    use saorsa_gossip_crdt_sync::{DeltaCrdt, OrSet};

    // Create two OR-Set instances
    let mut orset1 = OrSet::<String>::new();
    let mut orset2 = OrSet::<String>::new();

    // Peer 1 adds an element
    let peer1_id = saorsa_gossip_types::PeerId::new([1u8; 32]);
    let item1 = "item1".to_string();
    orset1.add(item1.clone(), (peer1_id, 1))?;

    // Generate delta from peer 1
    let delta1 = orset1.delta(0).ok_or("No delta available")?;

    // Peer 2 merges the delta
    orset2.merge(&delta1)?;

    // Both should have the element
    assert!(orset1.contains(&item1));
    assert!(orset2.contains(&item1));

    // Peer 2 adds another element
    let peer2_id = saorsa_gossip_types::PeerId::new([2u8; 32]);
    let item2 = "item2".to_string();
    orset2.add(item2.clone(), (peer2_id, 1))?;

    // Sync back to peer 1
    let delta2 = orset2.delta(0).ok_or("No delta available")?;
    orset1.merge(&delta2)?;

    // Both should have both elements
    assert!(orset1.contains(&item1));
    assert!(orset1.contains(&item2));
    assert!(orset2.contains(&item1));
    assert!(orset2.contains(&item2));

    Ok(())
}

/// Test message signing and verification across components
#[tokio::test]
async fn test_message_signing_verification() -> Result<(), Box<dyn std::error::Error>> {
    // Create identity
    let identity = saorsa_gossip_identity::MlDsaKeyPair::generate()?;
    let peer_id = saorsa_gossip_types::PeerId::from_pubkey(identity.public_key());

    // Create message header
    let topic = saorsa_gossip_types::TopicId::new([42u8; 32]);
    let mut header =
        saorsa_gossip_types::MessageHeader::new(topic, saorsa_gossip_types::MessageKind::Eager, 10);
    header.msg_id =
        saorsa_gossip_types::MessageHeader::calculate_msg_id(&topic, 12345, &peer_id, &[1u8; 32]);

    // Sign the header
    let header_bytes = bincode::serialize(&header)?;
    let signature = identity.sign(&header_bytes)?;

    // Verify signature (in real scenario, other peers would do this)
    let is_valid = saorsa_gossip_identity::MlDsaKeyPair::verify(
        identity.public_key(),
        &header_bytes,
        &signature,
    )?;
    assert!(is_valid);

    Ok(())
}

/// Test network simulator basic functionality
#[tokio::test]
async fn test_network_simulator_basic() -> Result<(), Box<dyn std::error::Error>> {
    use saorsa_gossip_simulator::{LinkConfig, NetworkSimulator, Topology};

    // Create simulator with 3 nodes in mesh topology
    let mut simulator = NetworkSimulator::new()
        .with_topology(Topology::Mesh)
        .with_nodes(3)
        .with_time_dilation(5.0)
        .with_seed(12345); // Deterministic for testing

    // Configure high-latency, lossy network
    let config = LinkConfig {
        latency_ms: 100,
        bandwidth_bps: 100_000, // 100 Kbps
        packet_loss_rate: 0.1,  // 10% loss
        jitter_ms: 20,
    };
    simulator.set_link_config_all(config);

    // Start simulation
    simulator.start().await?;

    // Get initial stats
    let initial_stats = simulator.get_stats().await;
    assert_eq!(initial_stats.nodes, 3);
    assert_eq!(initial_stats.queued_messages, 0);

    // Stop simulation
    simulator.stop().await?;

    Ok(())
}

/// Test chaos engineering with network simulator
#[tokio::test]
async fn test_chaos_engineering_integration() -> Result<(), Box<dyn std::error::Error>> {
    use saorsa_gossip_simulator::{ChaosEvent, ChaosInjector, ChaosScenario, NetworkSimulator};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;

    // Create simulator with 4 nodes
    let simulator = Arc::new(RwLock::new(
        NetworkSimulator::new()
            .with_nodes(4)
            .with_time_dilation(10.0) // Fast testing
            .with_seed(999),
    ));

    // Create chaos injector
    let injector = ChaosInjector::new();

    // Test individual chaos events
    injector.enable().await;

    // Test latency spike
    let latency_event = ChaosEvent::LatencySpike {
        latency_ms: 300,
        duration: Duration::from_secs(2),
    };
    injector.inject_event(latency_event).await?;

    // Verify chaos is active
    let chaos_stats = injector.get_stats().await;
    assert!(chaos_stats.enabled);
    assert_eq!(chaos_stats.active_events, 1);

    // Test chaos scenario
    let scenario = ChaosScenario {
        name: "test_scenario".to_string(),
        duration: Duration::from_secs(5),
        events: vec![(
            Duration::from_secs(1),
            ChaosEvent::MessageLoss {
                loss_rate: 0.05,
                duration: Duration::from_secs(2),
            },
        )],
    };

    // Run the scenario
    let injector_clone = injector.clone();
    let simulator_clone = Arc::clone(&simulator);
    let scenario_handle =
        tokio::spawn(async move { injector_clone.run_scenario(scenario, simulator_clone).await });

    // Wait a bit for scenario to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check that scenario is running
    let chaos_stats = injector.get_stats().await;
    assert!(chaos_stats.enabled);

    // Wait for scenario to complete
    scenario_handle.await??;

    // Verify chaos is disabled after scenario
    let chaos_stats = injector.get_stats().await;
    assert!(!chaos_stats.enabled);
    assert_eq!(chaos_stats.active_events, 0);

    Ok(())
}

/// Test predefined chaos scenarios
#[tokio::test]
async fn test_predefined_chaos_scenarios() -> Result<(), Box<dyn std::error::Error>> {
    use saorsa_gossip_simulator::{ChaosEvent, NetworkSimulator};
    use std::time::Duration;

    let scenarios = NetworkSimulator::create_chaos_scenarios();

    // Verify we have the expected scenarios
    assert_eq!(scenarios.len(), 4);

    // Check each scenario has expected properties
    for scenario in scenarios {
        assert!(!scenario.name.is_empty());
        assert!(!scenario.events.is_empty());
        assert!(scenario.duration > Duration::from_secs(0));

        // Verify all events have valid durations
        for (_, event) in &scenario.events {
            let event_duration = match event {
                ChaosEvent::NodeFailure { duration, .. } => *duration,
                ChaosEvent::NetworkPartition { duration, .. } => *duration,
                ChaosEvent::MessageLoss { duration, .. } => *duration,
                ChaosEvent::MessageCorruption { duration, .. } => *duration,
                ChaosEvent::LatencySpike { duration, .. } => *duration,
                ChaosEvent::BandwidthThrottling { duration, .. } => *duration,
                ChaosEvent::ClockSkew { duration, .. } => *duration,
                ChaosEvent::Custom { duration, .. } => *duration,
            };
            assert!(event_duration > Duration::from_secs(0));
        }
    }

    Ok(())
}
