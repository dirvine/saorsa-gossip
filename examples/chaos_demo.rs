//! Chaos Engineering Demo for Saorsa Gossip
//!
//! This example demonstrates the chaos engineering capabilities
//! of the network simulator, showing how to inject various failure
//! scenarios to test system resilience.

use saorsa_gossip_simulator::{NetworkSimulator, ChaosInjector, ChaosEvent, ChaosScenario};
use std::time::Duration;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌀 Saorsa Gossip Chaos Engineering Demo");
    println!("========================================");

    // Create a network simulator with challenging conditions
    println!("\n🌐 Setting up network simulator...");
    let simulator = Arc::new(RwLock::new(NetworkSimulator::new()
        .with_topology(saorsa_gossip_simulator::Topology::Mesh)
        .with_nodes(5)
        .with_time_dilation(5.0) // 5x speedup for demo
        .with_seed(42))); // Deterministic for reproducible results

    // Configure baseline network conditions (already challenging)
    simulator.write().await.set_link_config_all(saorsa_gossip_simulator::LinkConfig {
        latency_ms: 100,
        bandwidth_bps: 500_000, // 500 Kbps
        packet_loss_rate: 0.02, // 2% baseline loss
        jitter_ms: 20,
    });

    println!("✓ Created simulator with 5 nodes in mesh topology");
    println!("  - 100ms latency, 2% packet loss baseline");
    println!("  - 5x time acceleration");

    // Start the simulation
    simulator.write().await.start().await?;
    println!("✓ Simulation started");

    // Create chaos injector
    let injector = ChaosInjector::new();
    println!("\n🌀 Chaos Engineering Scenarios");
    println!("==============================");

    // Scenario 1: Network Degradation
    println!("\n📉 Scenario 1: Network Degradation");
    println!("-----------------------------------");
    let degradation_scenario = ChaosScenario {
        name: "network_degradation".to_string(),
        duration: Duration::from_secs(8),
        events: vec![
            (Duration::from_secs(2), ChaosEvent::LatencySpike {
                latency_ms: 300,
                duration: Duration::from_secs(3),
            }),
            (Duration::from_secs(4), ChaosEvent::MessageLoss {
                loss_rate: 0.15, // Additional 15% loss
                duration: Duration::from_secs(2),
            }),
        ],
    };

    println!("Injecting: 300ms latency spike + 15% message loss");
    let injector_clone = injector.clone();
    let simulator_clone = Arc::clone(&simulator);
    let scenario1 = tokio::spawn(async move {
        injector_clone.run_scenario(degradation_scenario, simulator_clone).await
    });

    // Monitor during scenario
    for i in 0..8 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let chaos_stats = injector.get_stats().await;
        if chaos_stats.enabled {
            println!("  [{}] Chaos active: {} events", i + 1, chaos_stats.active_events);
        } else {
            println!("  [{}] Normal operation", i + 1);
        }
    }

    scenario1.await??;
    println!("✓ Network degradation scenario completed");

    // Scenario 2: Extreme Chaos
    println!("\n💥 Scenario 2: Extreme Chaos");
    println!("----------------------------");
    let extreme_scenario = ChaosScenario {
        name: "extreme_chaos".to_string(),
        duration: Duration::from_secs(6),
        events: vec![
            (Duration::from_secs(1), ChaosEvent::MessageLoss {
                loss_rate: 0.3, // 30% loss
                duration: Duration::from_secs(4),
            }),
            (Duration::from_secs(2), ChaosEvent::LatencySpike {
                latency_ms: 1000, // 1 second!
                duration: Duration::from_secs(3),
            }),
            (Duration::from_secs(3), ChaosEvent::BandwidthThrottling {
                bandwidth_bps: 10_000, // 10 Kbps - very slow
                duration: Duration::from_secs(2),
            }),
        ],
    };

    println!("Injecting: 30% loss + 1000ms latency + 10Kbps bandwidth");
    let injector_clone = injector.clone();
    let simulator_clone = Arc::clone(&simulator);
    let scenario2 = tokio::spawn(async move {
        injector_clone.run_scenario(extreme_scenario, simulator_clone).await
    });

    // Monitor during extreme chaos
    for i in 0..6 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let chaos_stats = injector.get_stats().await;
        let sim_stats = simulator.read().await.get_stats().await;
        println!("  [{}] Chaos: {} events, Messages: {}",
                i + 1, chaos_stats.active_events, sim_stats.queued_messages);
    }

    scenario2.await??;
    println!("✓ Extreme chaos scenario completed");

    // Scenario 3: Individual Events
    println!("\n🎯 Scenario 3: Individual Chaos Events");
    println!("-------------------------------------");

    let events = vec![
        ("Node Failure", ChaosEvent::NodeFailure {
            node_id: 2,
            duration: Duration::from_secs(3),
        }),
        ("Clock Skew", ChaosEvent::ClockSkew {
            node_id: 1,
            offset_ms: 500,
            duration: Duration::from_secs(2),
        }),
        ("Message Corruption", ChaosEvent::MessageCorruption {
            corruption_rate: 0.1,
            duration: Duration::from_secs(2),
        }),
    ];

    for (name, event) in events {
        println!("Injecting: {}", name);
        injector.inject_event(event).await?;

        // Show chaos is active
        let stats = injector.get_stats().await;
        println!("  Active chaos events: {}", stats.active_events);

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    // Stop the simulation
    simulator.write().await.stop().await?;
    println!("\n⏹️  Simulation stopped");

    println!("\n🎯 Chaos Engineering Demo Complete!");
    println!("\n📊 Demonstrated Capabilities:");
    println!("  ✅ Network degradation testing");
    println!("  ✅ Extreme failure scenario simulation");
    println!("  ✅ Individual chaos event injection");
    println!("  ✅ Real-time monitoring and statistics");
    println!("  ✅ Deterministic chaos with seeded RNG");
    println!("  ✅ Time-dilated simulation for fast testing");

    println!("\n🚀 Next Steps:");
    println!("  - Integrate with actual gossip protocols");
    println!("  - Add property-based testing with chaos");
    println!("  - Implement automated resilience verification");
    println!("  - Create chaos testing dashboards");

    Ok(())
}
