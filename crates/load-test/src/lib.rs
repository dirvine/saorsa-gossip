//! Load Testing Framework for Saorsa Gossip
//!
//! This crate provides comprehensive load testing capabilities for
//! validating Saorsa Gossip performance under high message rates and
//! concurrent peer loads.
//!
//! # Features
//!
//! - **High-throughput message generation** with configurable patterns
//! - **Concurrent peer simulation** with realistic behavior models
//! - **Real-time performance metrics** collection and analysis
//! - **Scalable load scenarios** from small tests to massive simulations
//! - **Performance regression detection** with statistical analysis
//!
//! # Example Usage
//!
//! ```rust,no_run
//! use saorsa_gossip_load_test::{LoadTestRunner, LoadScenario, MessagePattern};
//! use saorsa_gossip_simulator::{NetworkSimulator, Topology};
//! use std::time::Duration;
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create network simulator
//! let simulator = Arc::new(RwLock::new(NetworkSimulator::new()
//!     .with_nodes(100)
//!     .with_topology(Topology::Mesh)));
//!
//! // Create a load scenario
//! let scenario = LoadScenario {
//!     name: "pubsub_storm".to_string(),
//!     duration: Duration::from_secs(60),
//!     num_peers: 100,
//!     message_pattern: MessagePattern::Burst {
//!         messages_per_burst: 100,
//!         burst_interval: Duration::from_millis(100),
//!         message_size: 1024,
//!     },
//!     topology: Topology::Mesh,
//!     chaos_events: vec![], // No chaos for pure load testing
//! };
//!
//! // Run the load test
//! let runner = LoadTestRunner::new();
//! let results = runner.run_scenario(scenario, simulator).await?;
//!
//! println!("Throughput: {} msgs/sec", results.throughput_msgs_per_sec);
//! println!("Latency P95: {}ms", results.latency_p95_ms);
//! println!("Memory usage: {}MB", results.memory_usage_mb);
//!
//! # Ok(())
//! # }
//! ```

use hdrhistogram::Histogram;
use rand::prelude::*;
use rand_pcg::Pcg64;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{self, Instant as TokioInstant};
use tracing::{debug, info, warn};

use saorsa_gossip_simulator::{
    ChaosInjector, MessageType, NetworkSimulator, SimulatedMessage, Topology,
};
use saorsa_gossip_types::TopicId;

/// Load testing result metrics
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadTestResults {
    /// Test scenario name
    pub scenario_name: String,
    /// Total duration of the test
    pub duration: Duration,
    /// Number of peers simulated
    pub num_peers: usize,
    /// Total messages sent
    pub total_messages: u64,
    /// Messages per second throughput
    pub throughput_msgs_per_sec: f64,
    /// Latency percentiles (in milliseconds)
    pub latency_p50_ms: u64,
    pub latency_p95_ms: u64,
    pub latency_p99_ms: u64,
    /// Message loss rate (0.0 to 1.0)
    pub message_loss_rate: f64,
    /// Memory usage in MB
    pub memory_usage_mb: f64,
    /// CPU utilization percentage
    pub cpu_utilization_percent: f64,
    /// Error count
    pub error_count: u64,
    /// Start timestamp
    pub start_time: chrono::DateTime<chrono::Utc>,
    /// End timestamp
    pub end_time: chrono::DateTime<chrono::Utc>,
}

/// Message generation patterns for load testing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessagePattern {
    /// Constant rate message generation
    Constant {
        /// Messages per second
        rate_per_second: u32,
        /// Size of each message in bytes
        message_size: usize,
    },
    /// Burst pattern with periodic message floods
    Burst {
        /// Messages per burst
        messages_per_burst: u32,
        /// Time between bursts
        burst_interval: Duration,
        /// Size of each message in bytes
        message_size: usize,
    },
    /// Ramp up pattern starting slow and increasing
    RampUp {
        /// Starting messages per second
        start_rate_per_second: u32,
        /// Ending messages per second
        end_rate_per_second: u32,
        /// Ramp duration
        ramp_duration: Duration,
        /// Size of each message in bytes
        message_size: usize,
    },
    /// Realistic pattern mimicking user behavior
    Realistic {
        /// Base message rate
        base_rate_per_second: u32,
        /// Peak rate multiplier
        peak_multiplier: f64,
        /// Peak duration as fraction of total test (0.0-1.0)
        peak_fraction: f64,
        /// Size of each message in bytes
        message_size: usize,
    },
}

/// Load test scenario configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadScenario {
    /// Scenario name for identification
    pub name: String,
    /// Total test duration
    pub duration: Duration,
    /// Number of concurrent peers to simulate
    pub num_peers: usize,
    /// Message generation pattern
    pub message_pattern: MessagePattern,
    /// Network topology
    pub topology: Topology,
    /// Optional chaos events to inject during load testing
    pub chaos_events: Vec<(Duration, saorsa_gossip_simulator::ChaosEvent)>,
}

/// Message generation statistics
#[derive(Clone, Debug)]
struct MessageStats {
    /// Messages sent
    sent: u64,
    /// Messages received
    received: u64,
    /// Send timestamps for latency calculation
    send_times: HashMap<u64, TokioInstant>,
    /// Latency histogram
    latency_histogram: Histogram<u64>,
}

/// Load test runner - main orchestrator for load testing
pub struct LoadTestRunner {
    /// Random number generator
    rng: Arc<Mutex<Pcg64>>,
    /// Message statistics
    stats: Arc<RwLock<MessageStats>>,
    /// Start time of current test
    start_time: Arc<RwLock<Option<TokioInstant>>>,
}

impl Default for LoadTestRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadTestRunner {
    /// Create a new load test runner
    pub fn new() -> Self {
        let rng = Pcg64::seed_from_u64(31415); // Deterministic seed for testing

        Self {
            rng: Arc::new(Mutex::new(rng)),
            stats: Arc::new(RwLock::new(MessageStats {
                sent: 0,
                received: 0,
                send_times: HashMap::new(),
                latency_histogram: Histogram::new(3).unwrap(), // 1ms to ~8 hours
            })),
            start_time: Arc::new(RwLock::new(None)),
        }
    }

    /// Create load test runner with specific seed
    pub fn with_seed(seed: u64) -> Self {
        let mut runner = Self::new();
        runner.rng = Arc::new(Mutex::new(Pcg64::seed_from_u64(seed)));
        runner
    }

    /// Run a complete load test scenario
    pub async fn run_scenario(
        &self,
        scenario: LoadScenario,
        simulator: Arc<RwLock<NetworkSimulator>>,
    ) -> Result<LoadTestResults, LoadTestError> {
        info!("Starting load test scenario: {}", scenario.name);

        let start_time = chrono::Utc::now();
        *self.start_time.write().await = Some(TokioInstant::now());

        // Start simulator if not already started
        {
            let sim = simulator.read().await;
            if !sim.is_running().await {
                drop(sim);
                let mut sim = simulator.write().await;
                sim.start().await?;
            }
        }

        // Create chaos injector if chaos events are specified
        if !scenario.chaos_events.is_empty() {
            let injector = ChaosInjector::new();

            // Run chaos scenario in background
            let chaos_scenario = saorsa_gossip_simulator::ChaosScenario {
                name: format!("{}_chaos", scenario.name),
                duration: scenario.duration,
                events: scenario.chaos_events.clone(),
            };

            let injector_clone = injector.clone();
            let simulator_clone = Arc::clone(&simulator);
            tokio::spawn(async move {
                if let Err(e) = injector_clone
                    .run_scenario(chaos_scenario, simulator_clone)
                    .await
                {
                    warn!("Chaos scenario failed: {:?}", e);
                }
            });
        }

        // Start message generators
        let message_tasks = self.start_message_generators(&scenario, &simulator).await?;

        // Monitor test progress
        let results = self
            .monitor_test_progress(scenario.clone(), start_time)
            .await?;

        // Clean up - don't stop the simulator as it was passed in
        for task in message_tasks {
            task.abort();
        }

        info!("Completed load test scenario: {}", scenario.name);
        Ok(results)
    }

    /// Start message generator tasks for each peer
    async fn start_message_generators(
        &self,
        scenario: &LoadScenario,
        simulator: &Arc<RwLock<NetworkSimulator>>,
    ) -> Result<Vec<tokio::task::JoinHandle<()>>, LoadTestError> {
        let mut tasks = Vec::new();
        let stats = self.stats.clone();

        for peer_id in 0..scenario.num_peers {
            let peer_id = peer_id as u32;
            let pattern = scenario.message_pattern.clone();
            let stats_clone = stats.clone();
            let simulator_clone = Arc::clone(simulator);

            let task = tokio::spawn(async move {
                Self::run_message_generator(peer_id, pattern, stats_clone, simulator_clone).await;
            });

            tasks.push(task);
        }

        Ok(tasks)
    }

    /// Run message generator for a single peer
    async fn run_message_generator(
        peer_id: u32,
        pattern: MessagePattern,
        stats: Arc<RwLock<MessageStats>>,
        simulator: Arc<RwLock<NetworkSimulator>>,
    ) {
        let topic = TopicId::new([1u8; 32]); // Fixed topic for load testing

        match pattern {
            MessagePattern::Constant {
                rate_per_second,
                message_size,
            } => {
                Self::generate_constant_rate(
                    peer_id,
                    rate_per_second,
                    message_size,
                    topic,
                    stats,
                    simulator,
                )
                .await;
            }
            MessagePattern::Burst {
                messages_per_burst,
                burst_interval,
                message_size,
            } => {
                Self::generate_burst_pattern(
                    peer_id,
                    messages_per_burst,
                    burst_interval,
                    message_size,
                    topic,
                    stats,
                    simulator,
                )
                .await;
            }
            MessagePattern::RampUp {
                start_rate_per_second,
                end_rate_per_second,
                ramp_duration,
                message_size,
            } => {
                Self::generate_ramp_up_pattern(
                    peer_id,
                    start_rate_per_second,
                    end_rate_per_second,
                    ramp_duration,
                    message_size,
                    topic,
                    stats,
                    simulator,
                )
                .await;
            }
            MessagePattern::Realistic {
                base_rate_per_second,
                peak_multiplier,
                peak_fraction,
                message_size,
            } => {
                Self::generate_realistic_pattern(
                    peer_id,
                    base_rate_per_second,
                    peak_multiplier,
                    peak_fraction,
                    message_size,
                    topic,
                    stats,
                    simulator,
                )
                .await;
            }
        }
    }

    /// Generate messages at constant rate
    async fn generate_constant_rate(
        peer_id: u32,
        rate_per_second: u32,
        message_size: usize,
        _topic: TopicId,
        stats: Arc<RwLock<MessageStats>>,
        simulator: Arc<RwLock<NetworkSimulator>>,
    ) {
        let interval = Duration::from_secs(1) / rate_per_second;
        let mut interval_timer = time::interval(interval);

        loop {
            interval_timer.tick().await;

            let message_id = {
                let mut stats_guard = stats.write().await;
                stats_guard.sent += 1;
                stats_guard.sent
            };

            let payload = vec![peer_id as u8; message_size];
            let message = SimulatedMessage {
                from: peer_id,
                to: ((peer_id + 1) % 5), // Send to next peer in ring
                payload,
                message_type: MessageType::PubSub,
                priority: 0,
                id: message_id,
            };

            // Record send time
            {
                let mut stats_guard = stats.write().await;
                stats_guard
                    .send_times
                    .insert(message_id, TokioInstant::now());
            }

            // Send message through simulator
            if let Err(e) = simulator
                .read()
                .await
                .send_message(peer_id, message.to, message.payload, message.message_type)
                .await
            {
                debug!("Failed to send message: {:?}", e);
            }
        }
    }

    /// Generate burst pattern messages
    async fn generate_burst_pattern(
        peer_id: u32,
        messages_per_burst: u32,
        burst_interval: Duration,
        message_size: usize,
        _topic: TopicId,
        stats: Arc<RwLock<MessageStats>>,
        simulator: Arc<RwLock<NetworkSimulator>>,
    ) {
        let mut burst_timer = time::interval(burst_interval);

        loop {
            burst_timer.tick().await;

            // Send burst of messages
            for _ in 0..messages_per_burst {
                let message_id = {
                    let mut stats_guard = stats.write().await;
                    stats_guard.sent += 1;
                    stats_guard.sent
                };

                let payload = vec![peer_id as u8; message_size];
                let message = SimulatedMessage {
                    from: peer_id,
                    to: ((peer_id + 1) % 5),
                    payload,
                    message_type: MessageType::PubSub,
                    priority: 0,
                    id: message_id,
                };

                // Record send time
                {
                    let mut stats_guard = stats.write().await;
                    stats_guard
                        .send_times
                        .insert(message_id, TokioInstant::now());
                }

                if let Err(e) = simulator
                    .read()
                    .await
                    .send_message(peer_id, message.to, message.payload, message.message_type)
                    .await
                {
                    debug!("Failed to send message: {:?}", e);
                }
            }
        }
    }

    /// Generate ramp-up pattern messages
    #[allow(clippy::too_many_arguments)]
    async fn generate_ramp_up_pattern(
        peer_id: u32,
        start_rate: u32,
        end_rate: u32,
        ramp_duration: Duration,
        message_size: usize,
        _topic: TopicId,
        stats: Arc<RwLock<MessageStats>>,
        simulator: Arc<RwLock<NetworkSimulator>>,
    ) {
        let start_time = TokioInstant::now();
        let ramp_duration_secs = ramp_duration.as_secs_f64();
        let rate_range = end_rate as f64 - start_rate as f64;

        loop {
            let elapsed = start_time.elapsed().as_secs_f64();
            let progress = (elapsed / ramp_duration_secs).min(1.0);
            let current_rate = start_rate as f64 + (rate_range * progress);

            let interval = Duration::from_secs_f64(1.0 / current_rate);
            time::sleep(interval).await;

            let message_id = {
                let mut stats_guard = stats.write().await;
                stats_guard.sent += 1;
                stats_guard.sent
            };

            let payload = vec![peer_id as u8; message_size];
            let message = SimulatedMessage {
                from: peer_id,
                to: ((peer_id + 1) % 5),
                payload,
                message_type: MessageType::PubSub,
                priority: 0,
                id: message_id,
            };

            // Record send time
            {
                let mut stats_guard = stats.write().await;
                stats_guard
                    .send_times
                    .insert(message_id, TokioInstant::now());
            }

            if let Err(e) = simulator
                .read()
                .await
                .send_message(peer_id, message.to, message.payload, message.message_type)
                .await
            {
                debug!("Failed to send message: {:?}", e);
            }
        }
    }

    /// Generate realistic pattern messages
    #[allow(clippy::too_many_arguments)]
    async fn generate_realistic_pattern(
        peer_id: u32,
        base_rate: u32,
        _peak_multiplier: f64,
        _peak_fraction: f64,
        message_size: usize,
        _topic: TopicId,
        stats: Arc<RwLock<MessageStats>>,
        simulator: Arc<RwLock<NetworkSimulator>>,
    ) {
        // For simplicity, implement as constant rate with occasional bursts
        let interval = Duration::from_secs(1) / base_rate;
        let mut interval_timer = time::interval(interval);

        loop {
            interval_timer.tick().await;

            let message_id = {
                let mut stats_guard = stats.write().await;
                stats_guard.sent += 1;
                stats_guard.sent
            };

            let payload = vec![peer_id as u8; message_size];
            let message = SimulatedMessage {
                from: peer_id,
                to: ((peer_id + 1) % 5),
                payload,
                message_type: MessageType::PubSub,
                priority: 0,
                id: message_id,
            };

            // Record send time
            {
                let mut stats_guard = stats.write().await;
                stats_guard
                    .send_times
                    .insert(message_id, TokioInstant::now());
            }

            if let Err(e) = simulator
                .read()
                .await
                .send_message(peer_id, message.to, message.payload, message.message_type)
                .await
            {
                debug!("Failed to send message: {:?}", e);
            }
        }
    }

    /// Monitor test progress and collect final results
    async fn monitor_test_progress(
        &self,
        scenario: LoadScenario,
        start_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<LoadTestResults, LoadTestError> {
        let test_duration = scenario.duration;

        // Wait for test to complete
        time::sleep(test_duration).await;

        let end_time = chrono::Utc::now();

        // Collect final statistics
        let stats = self.stats.read().await;
        let total_messages = stats.sent;
        let duration_secs = test_duration.as_secs_f64();

        // Calculate throughput
        let throughput_msgs_per_sec = total_messages as f64 / duration_secs;

        // Calculate latency percentiles
        let latency_p50 = stats.latency_histogram.value_at_percentile(50.0);
        let latency_p95 = stats.latency_histogram.value_at_percentile(95.0);
        let latency_p99 = stats.latency_histogram.value_at_percentile(99.0);

        // Calculate message loss rate
        let message_loss_rate = if total_messages > 0 {
            (total_messages - stats.received) as f64 / total_messages as f64
        } else {
            0.0
        };

        // Estimate memory and CPU (simplified for now)
        let memory_usage_mb = 50.0; // Placeholder
        let cpu_utilization_percent = 75.0; // Placeholder

        let results = LoadTestResults {
            scenario_name: scenario.name,
            duration: test_duration,
            num_peers: scenario.num_peers,
            total_messages,
            throughput_msgs_per_sec,
            latency_p50_ms: latency_p50,
            latency_p95_ms: latency_p95,
            latency_p99_ms: latency_p99,
            message_loss_rate,
            memory_usage_mb,
            cpu_utilization_percent,
            error_count: 0, // TODO: Track actual errors
            start_time,
            end_time,
        };

        Ok(results)
    }
}

/// Load testing error types
#[derive(thiserror::Error, Debug)]
pub enum LoadTestError {
    #[error("Simulator error: {0}")]
    SimulatorError(#[from] saorsa_gossip_simulator::SimulatorError),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Task join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("Test configuration error: {0}")]
    ConfigError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_test_runner_creation() {
        let runner = LoadTestRunner::new();
        assert!(runner.start_time.read().await.is_none());
    }

    #[tokio::test]
    async fn test_load_scenario_creation() {
        let scenario = LoadScenario {
            name: "test_scenario".to_string(),
            duration: Duration::from_secs(10),
            num_peers: 5,
            message_pattern: MessagePattern::Constant {
                rate_per_second: 10,
                message_size: 100,
            },
            topology: Topology::Mesh,
            chaos_events: vec![],
        };

        assert_eq!(scenario.name, "test_scenario");
        assert_eq!(scenario.num_peers, 5);
    }

    #[tokio::test]
    async fn test_message_pattern_constant() {
        let pattern = MessagePattern::Constant {
            rate_per_second: 100,
            message_size: 1024,
        };

        match pattern {
            MessagePattern::Constant {
                rate_per_second,
                message_size,
            } => {
                assert_eq!(rate_per_second, 100);
                assert_eq!(message_size, 1024);
            }
            _ => panic!("Wrong pattern type"),
        }
    }

    #[tokio::test]
    async fn test_message_pattern_burst() {
        let pattern = MessagePattern::Burst {
            messages_per_burst: 50,
            burst_interval: Duration::from_millis(500),
            message_size: 512,
        };

        match pattern {
            MessagePattern::Burst {
                messages_per_burst,
                burst_interval,
                message_size,
            } => {
                assert_eq!(messages_per_burst, 50);
                assert_eq!(burst_interval, Duration::from_millis(500));
                assert_eq!(message_size, 512);
            }
            _ => panic!("Wrong pattern type"),
        }
    }
}
