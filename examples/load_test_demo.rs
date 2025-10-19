//! Load Testing Demo for Saorsa Gossip
//!
//! Demonstrates the load testing framework capabilities including:
//! - Various message generation patterns
//! - Performance metrics collection
//! - Combining load with chaos engineering

use saorsa_gossip_load_test::{LoadTestRunner, LoadScenario, MessagePattern};
use saorsa_gossip_simulator::{NetworkSimulator, Topology, LinkConfig, ChaosEvent};
use std::time::Duration;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Saorsa Gossip Load Testing Demo");
    println!("===================================");

    // Create network simulator
    println!("\n🌐 Setting up test environment...");
    let simulator = Arc::new(RwLock::new(NetworkSimulator::new()
        .with_topology(Topology::Mesh)
        .with_nodes(10)
        .with_time_dilation(10.0) // 10x speedup for demo
        .with_seed(42)));

    // Configure network conditions
    simulator.write().await.set_link_config_all(LinkConfig {
        latency_ms: 50,
        bandwidth_bps: 1_000_000,
        packet_loss_rate: 0.01,
        jitter_ms: 10,
    });

    println!("✓ Created simulator with 10 nodes");

    // Start simulator
    simulator.write().await.start().await?;
    println!("✓ Simulator started");

    // Create load test runner
    let runner = LoadTestRunner::new();

    // Scenario 1: Constant Rate Load
    println!("\n📊 Scenario 1: Constant Rate Load");
    println!("-----------------------------------");
    let constant_scenario = LoadScenario {
        name: "constant_rate".to_string(),
        duration: Duration::from_secs(5),
        num_peers: 10,
        message_pattern: MessagePattern::Constant {
            rate_per_second: 50,
            message_size: 512,
        },
        topology: Topology::Mesh,
        chaos_events: vec![],
    };

    println!("Running: 50 messages/sec for 5 seconds");
    let results1 = runner.run_scenario(constant_scenario, Arc::clone(&simulator)).await?;
    print_results(&results1);

    // Scenario 2: Burst Pattern
    println!("\n💥 Scenario 2: Burst Pattern Load");
    println!("-----------------------------------");
    let burst_scenario = LoadScenario {
        name: "burst_pattern".to_string(),
        duration: Duration::from_secs(5),
        num_peers: 10,
        message_pattern: MessagePattern::Burst {
            messages_per_burst: 100,
            burst_interval: Duration::from_millis(1000),
            message_size: 256,
        },
        topology: Topology::Mesh,
        chaos_events: vec![],
    };

    println!("Running: 100 messages per burst, every 1s");
    let results2 = runner.run_scenario(burst_scenario, Arc::clone(&simulator)).await?;
    print_results(&results2);

    // Scenario 3: Ramp-up Pattern
    println!("\n📈 Scenario 3: Ramp-up Load");
    println!("---------------------------");
    let ramp_scenario = LoadScenario {
        name: "ramp_up".to_string(),
        duration: Duration::from_secs(5),
        num_peers: 10,
        message_pattern: MessagePattern::RampUp {
            start_rate_per_second: 10,
            end_rate_per_second: 100,
            ramp_duration: Duration::from_secs(5),
            message_size: 1024,
        },
        topology: Topology::Mesh,
        chaos_events: vec![],
    };

    println!("Running: Ramping from 10 to 100 msgs/sec");
    let results3 = runner.run_scenario(ramp_scenario, Arc::clone(&simulator)).await?;
    print_results(&results3);

    // Scenario 4: Load + Chaos Engineering
    println!("\n⚡ Scenario 4: Load + Chaos");
    println!("---------------------------");
    let chaos_load_scenario = LoadScenario {
        name: "chaos_load".to_string(),
        duration: Duration::from_secs(5),
        num_peers: 10,
        message_pattern: MessagePattern::Constant {
            rate_per_second: 50,
            message_size: 512,
        },
        topology: Topology::Mesh,
        chaos_events: vec![
            (Duration::from_secs(1), ChaosEvent::MessageLoss {
                loss_rate: 0.1,
                duration: Duration::from_secs(2),
            }),
            (Duration::from_secs(2), ChaosEvent::LatencySpike {
                latency_ms: 200,
                duration: Duration::from_secs(2),
            }),
        ],
    };

    println!("Running: 50 msgs/sec with chaos injection");
    let results4 = runner.run_scenario(chaos_load_scenario, Arc::clone(&simulator)).await?;
    print_results(&results4);

    // Stop simulator
    simulator.write().await.stop().await?;
    println!("\n⏹️  Simulator stopped");

    // Summary
    println!("\n📈 Load Test Summary");
    println!("=====================");
    println!("Scenario               | Throughput (msg/s) | P95 Latency (ms) | Loss Rate");
    println!("------------------------|-------------------|------------------|----------");
    println!("{:<23} | {:>17.2} | {:>16} | {:>7.2}%",
        results1.scenario_name, results1.throughput_msgs_per_sec,
        results1.latency_p95_ms, results1.message_loss_rate * 100.0);
    println!("{:<23} | {:>17.2} | {:>16} | {:>7.2}%",
        results2.scenario_name, results2.throughput_msgs_per_sec,
        results2.latency_p95_ms, results2.message_loss_rate * 100.0);
    println!("{:<23} | {:>17.2} | {:>16} | {:>7.2}%",
        results3.scenario_name, results3.throughput_msgs_per_sec,
        results3.latency_p95_ms, results3.message_loss_rate * 100.0);
    println!("{:<23} | {:>17.2} | {:>16} | {:>7.2}%",
        results4.scenario_name, results4.throughput_msgs_per_sec,
        results4.latency_p95_ms, results4.message_loss_rate * 100.0);

    println!("\n🎯 Demonstrated Capabilities:");
    println!("  ✅ Multiple load generation patterns");
    println!("  ✅ Performance metrics collection");
    println!("  ✅ Chaos engineering integration");
    println!("  ✅ Latency percentile tracking");
    println!("  ✅ Message loss monitoring");
    println!("  ✅ Scalable peer simulation");

    println!("\n🚀 Production Use Cases:");
    println!("  - Performance regression testing");
    println!("  - Capacity planning");
    println!("  - Resilience validation");
    println!("  - SLA verification");

    Ok(())
}

fn print_results(results: &saorsa_gossip_load_test::LoadTestResults) {
    println!("✓ Completed: {}", results.scenario_name);
    println!("  Total messages:     {}", results.total_messages);
    println!("  Throughput:         {:.2} msgs/sec", results.throughput_msgs_per_sec);
    println!("  Latency P50:        {}ms", results.latency_p50_ms);
    println!("  Latency P95:        {}ms", results.latency_p95_ms);
    println!("  Latency P99:        {}ms", results.latency_p99_ms);
    println!("  Message loss rate:  {:.2}%", results.message_loss_rate * 100.0);
    println!("  Memory usage:       {:.2}MB", results.memory_usage_mb);
}
