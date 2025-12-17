//! Demonstration of the Network Simulator for testing gossip protocols
//!
//! This example shows how to use the network simulator to test gossip
//! protocols under various network conditions including latency, packet
//! loss, and topology changes.

use saorsa_gossip_simulator::{LinkConfig, NetworkSimulator, Topology};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Saorsa Gossip Network Simulator Demo");
    println!("=========================================");

    // Create a simulator with 5 nodes in a mesh topology
    println!("\n📡 Setting up network simulator...");
    let mut simulator = NetworkSimulator::new()
        .with_topology(Topology::Mesh)
        .with_nodes(5)
        .with_time_dilation(5.0) // 5x speedup for demo
        .with_seed(42); // Deterministic for reproducible results

    // Configure realistic network conditions
    let network_config = LinkConfig {
        latency_ms: 50,           // 50ms average latency
        bandwidth_bps: 1_000_000, // 1 Mbps
        packet_loss_rate: 0.02,   // 2% packet loss
        jitter_ms: 10,            // 10ms jitter
    };

    simulator.set_link_config_all(network_config.clone());

    println!("✓ Created simulator with:");
    println!("  - 5 nodes in mesh topology");
    println!(
        "  - {}ms latency, {}% packet loss",
        network_config.latency_ms,
        network_config.packet_loss_rate * 100.0
    );
    println!("  - 5x time acceleration");

    // Start the simulation
    println!("\n▶️  Starting simulation...");
    simulator.start().await?;

    // Show initial stats
    let stats = simulator.get_stats().await;
    println!("✓ Simulation running with {} nodes", stats.nodes);

    // Simulate some network activity
    println!("\n📨 Simulating network activity...");

    // Simulate message passing between nodes
    for i in 0..5 {
        for j in 0..5 {
            if i != j {
                // This would normally be handled by the actual gossip protocol
                // For demo purposes, we just show the concept
                println!("  Node {} → Node {}: Message queued", i, j);
            }
        }
    }

    // Simulate network degradation (high latency scenario)
    println!("\n🌊 Simulating network congestion...");
    let congested_config = LinkConfig {
        latency_ms: 200,        // High latency
        bandwidth_bps: 100_000, // Low bandwidth
        packet_loss_rate: 0.1,  // High loss
        jitter_ms: 50,
    };

    // Apply congestion to specific links
    simulator.set_link_config(0, 1, congested_config.clone());
    simulator.set_link_config(1, 2, congested_config.clone());

    println!("✓ Applied congestion to links 0→1 and 1→2:");
    println!(
        "  - {}ms latency, {}% packet loss",
        congested_config.latency_ms,
        congested_config.packet_loss_rate * 100.0
    );

    // Wait a bit to simulate time passing
    sleep(Duration::from_millis(100)).await;

    // Show updated stats
    let updated_stats = simulator.get_stats().await;
    println!("\n📊 Simulation stats:");
    println!("  - Nodes: {}", updated_stats.nodes);
    println!("  - Queued messages: {}", updated_stats.queued_messages);
    println!("  - Time dilation: {}x", updated_stats.time_dilation);

    // Stop the simulation
    println!("\n⏹️  Stopping simulation...");
    simulator.stop().await?;

    println!("✓ Simulation completed successfully!");
    println!("\n🎯 Key Features Demonstrated:");
    println!("  - Deterministic network simulation");
    println!("  - Configurable latency, bandwidth, and packet loss");
    println!("  - Dynamic network condition changes");
    println!("  - Time dilation for accelerated testing");
    println!("  - Real-time statistics monitoring");

    println!("\n📚 Next Steps:");
    println!("  - Integrate with actual gossip protocols");
    println!("  - Add chaos engineering (node failures, partitions)");
    println!("  - Implement property-based testing");
    println!("  - Add performance benchmarking");

    Ok(())
}
