//! Persistent peer cache for fast reconnection and parallel bootstrap
//!
//! This module provides a persistent cache of discovered peers that enables:
//! - Fast reconnection to known peers
//! - Parallel bootstrap from multiple peers
//! - Automatic stale peer removal
//! - Configurable storage locations
//!
//! # Platform-Specific Default Locations
//!
//! - **Linux**: `~/.local/share/saorsa-gossip/peer_cache.bin`
//! - **macOS**: `~/Library/Application Support/com.saorsa.gossip/peer_cache.bin`
//! - **Windows**: `%LOCALAPPDATA%\SaorsaGossip\peer_cache.bin`
//! - **Testing**: `/tmp/saorsa-gossip-test-{uuid}/peer_cache.bin`
//!
//! # Examples
//!
//! ## Default Production Cache
//! ```rust,no_run
//! use saorsa_gossip_transport::PeerCache;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let cache = PeerCache::default_production()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom Cache Location
//! ```rust,no_run
//! use saorsa_gossip_transport::{PeerCache, PeerCacheConfig};
//! use std::path::PathBuf;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = PeerCacheConfig::default()
//!     .cache_directory(PathBuf::from("/opt/my-app"))
//!     .cache_filename("peers.db")
//!     .max_capacity(10000);
//!
//! let cache = PeerCache::new(config)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Testing Mode
//! ```rust,no_run
//! use saorsa_gossip_transport::PeerCache;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let cache = PeerCache::default_testing()?;
//! # Ok(())
//! # }
//! ```

use anyhow::{Context, Result};
use saorsa_gossip_types::PeerId as GossipPeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Default maximum number of peers to cache
pub const DEFAULT_CACHE_CAPACITY: usize = 5000;

/// Default batch size for parallel bootstrap
pub const DEFAULT_BATCH_SIZE: usize = 50;

/// Default maximum concurrent connections during bootstrap
pub const DEFAULT_MAX_CONCURRENT: usize = 100;

/// Default maximum consecutive failures before removing peer
pub const DEFAULT_MAX_FAILURES: u32 = 3;

/// Default stale timeout in days
pub const DEFAULT_STALE_TIMEOUT_DAYS: u64 = 30;

/// Default number of successful connections required
pub const DEFAULT_REQUIRED_CONNECTIONS: usize = 10;

/// Configuration for peer cache behavior and storage
#[derive(Debug, Clone)]
pub struct PeerCacheConfig {
    /// Cache file path (None = auto-detect based on mode)
    pub cache_path: Option<PathBuf>,

    /// Cache file name (default: "peer_cache.bin")
    pub cache_filename: String,

    /// Use testing mode (temp directory)
    pub testing_mode: bool,

    /// Maximum number of peers to cache
    pub max_capacity: usize,

    /// Maximum consecutive connection failures before removing peer
    pub max_consecutive_failures: u32,

    /// Duration after which a peer is considered stale
    pub stale_timeout: Duration,

    /// Save interval for periodic cache persistence
    pub save_interval: Duration,

    /// Cleanup interval for removing stale peers
    pub cleanup_interval: Duration,
}

impl Default for PeerCacheConfig {
    fn default() -> Self {
        Self {
            cache_path: None, // Auto-detect
            cache_filename: "peer_cache.bin".to_string(),
            testing_mode: false,
            max_capacity: DEFAULT_CACHE_CAPACITY,
            max_consecutive_failures: DEFAULT_MAX_FAILURES,
            stale_timeout: Duration::from_secs(60 * 60 * 24 * DEFAULT_STALE_TIMEOUT_DAYS),
            save_interval: Duration::from_secs(60), // 1 minute
            cleanup_interval: Duration::from_secs(300), // 5 minutes
        }
    }
}

impl PeerCacheConfig {
    /// Create config for testing with temp directory
    pub fn testing() -> Self {
        Self {
            testing_mode: true,
            ..Default::default()
        }
    }

    /// Create config for production with custom path
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            cache_path: Some(path),
            ..Default::default()
        }
    }

    /// Builder: Set custom cache filename
    pub fn cache_filename(mut self, name: impl Into<String>) -> Self {
        self.cache_filename = name.into();
        self
    }

    /// Builder: Set custom cache directory (filename will be appended)
    pub fn cache_directory(mut self, dir: PathBuf) -> Self {
        self.cache_path = Some(dir);
        self
    }

    /// Builder: Set max capacity
    pub fn max_capacity(mut self, capacity: usize) -> Self {
        self.max_capacity = capacity;
        self
    }

    /// Builder: Set stale timeout in days
    pub fn stale_timeout_days(mut self, days: u64) -> Self {
        self.stale_timeout = Duration::from_secs(60 * 60 * 24 * days);
        self
    }

    /// Resolve the final cache file path based on configuration
    pub fn resolve_cache_path(&self) -> Result<PathBuf> {
        // If explicit path provided, use it
        if let Some(path) = &self.cache_path {
            return Ok(path.join(&self.cache_filename));
        }

        // Testing mode: use temp directory with unique name
        if self.testing_mode {
            let unique_id = uuid::Uuid::new_v4();
            return Ok(std::env::temp_dir()
                .join(format!("saorsa-gossip-test-{}", unique_id))
                .join(&self.cache_filename));
        }

        // Production mode: platform-specific app data directory
        #[cfg(target_os = "linux")]
        let base_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("saorsa-gossip");

        #[cfg(target_os = "macos")]
        let base_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("com.saorsa.gossip");

        #[cfg(target_os = "windows")]
        let base_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("SaorsaGossip");

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        let base_dir = PathBuf::from(".");

        Ok(base_dir.join(&self.cache_filename))
    }
}

/// A cached peer entry with connection metadata
#[derive(Serialize, Deserialize, Clone, Debug)]
struct CachedPeer {
    /// Peer's gossip ID
    peer_id: GossipPeerId,
    /// Peer's socket address
    socket_addr: SocketAddr,
    /// Last successful connection time
    last_seen: SystemTime,
    /// Total connection attempts
    connection_attempts: u32,
    /// Consecutive failures since last success
    consecutive_failures: u32,
    /// Total successful connections
    successful_connections: u32,
}

impl CachedPeer {
    /// Check if peer is stale based on timeout and failure count
    fn is_stale(&self, max_failures: u32, stale_timeout: Duration) -> bool {
        // Stale if too many consecutive failures
        if self.consecutive_failures >= max_failures {
            return true;
        }

        // Stale if not seen within timeout
        let time_since_last_seen = SystemTime::now()
            .duration_since(self.last_seen)
            .unwrap_or(Duration::MAX);

        time_since_last_seen > stale_timeout
    }
}

/// Persistent cache of discovered peers for fast reconnection
pub struct PeerCache {
    /// Cache configuration
    config: PeerCacheConfig,
    /// Resolved cache file path
    cache_file: PathBuf,
    /// In-memory cache of peers
    peers: Arc<RwLock<HashMap<GossipPeerId, CachedPeer>>>,
}

impl PeerCache {
    /// Create a new peer cache with custom configuration
    pub fn new(config: PeerCacheConfig) -> Result<Self> {
        let cache_file = config
            .resolve_cache_path()
            .context("Failed to resolve cache file path")?;

        info!("Creating peer cache at: {}", cache_file.display());

        // Create parent directory if it doesn't exist
        if let Some(parent) = cache_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create cache directory: {}", parent.display()))?;
        }

        // Load existing cache if available
        let peers = if cache_file.exists() {
            info!("Loading existing peer cache from {}", cache_file.display());
            Self::load_from_file(&cache_file)?
        } else {
            info!("No existing cache found, starting fresh");
            Arc::new(RwLock::new(HashMap::new()))
        };

        let cache = Self {
            config: config.clone(),
            cache_file,
            peers,
        };

        // Spawn background tasks
        cache.spawn_periodic_save();
        cache.spawn_cleanup_task();

        Ok(cache)
    }

    /// Create a peer cache with default settings (production mode)
    pub fn default_production() -> Result<Self> {
        Self::new(PeerCacheConfig::default())
    }

    /// Create a peer cache for testing (temp directory)
    pub fn default_testing() -> Result<Self> {
        Self::new(PeerCacheConfig::testing())
    }

    /// Load peers from cache file
    fn load_from_file(path: &PathBuf) -> Result<Arc<RwLock<HashMap<GossipPeerId, CachedPeer>>>> {
        let data = std::fs::read(path)
            .with_context(|| format!("Failed to read cache file: {}", path.display()))?;

        let peers: HashMap<GossipPeerId, CachedPeer> = bincode::deserialize(&data)
            .context("Failed to deserialize peer cache")?;

        info!("Loaded {} peers from cache", peers.len());
        Ok(Arc::new(RwLock::new(peers)))
    }

    /// Spawn background task for periodic cache saves
    fn spawn_periodic_save(&self) {
        let cache_file = self.cache_file.clone();
        let peers = Arc::clone(&self.peers);
        let save_interval = self.config.save_interval;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(save_interval);

            loop {
                interval.tick().await;

                let peers_guard = peers.read().await;
                if peers_guard.is_empty() {
                    continue;
                }

                let data = match bincode::serialize(&*peers_guard) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!("Failed to serialize peer cache: {}", e);
                        continue;
                    }
                };
                drop(peers_guard);

                let temp_file = cache_file.with_extension("tmp");
                if let Err(e) = tokio::fs::write(&temp_file, data).await {
                    warn!("Failed to write temp cache file: {}", e);
                    continue;
                }

                if let Err(e) = tokio::fs::rename(&temp_file, &cache_file).await {
                    warn!("Failed to rename cache file: {}", e);
                }
            }
        });
    }

    /// Spawn background task for periodic stale peer cleanup
    fn spawn_cleanup_task(&self) {
        let peers = Arc::clone(&self.peers);
        let max_failures = self.config.max_consecutive_failures;
        let stale_timeout = self.config.stale_timeout;
        let cleanup_interval = self.config.cleanup_interval;
        let max_capacity = self.config.max_capacity;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);

            loop {
                interval.tick().await;

                let mut peers_guard = peers.write().await;
                let initial_count = peers_guard.len();

                // Remove stale peers
                peers_guard.retain(|_, peer| !peer.is_stale(max_failures, stale_timeout));

                // Enforce capacity limit (remove least successful if over capacity)
                if peers_guard.len() > max_capacity {
                    let mut peer_vec: Vec<_> = peers_guard.drain().collect();
                    peer_vec.sort_by_key(|(_, p)| p.successful_connections);
                    peer_vec.truncate(max_capacity);
                    *peers_guard = peer_vec.into_iter().collect();
                }

                let final_count = peers_guard.len();
                if initial_count != final_count {
                    info!(
                        "Cleaned up peer cache: {} -> {} peers",
                        initial_count, final_count
                    );
                }
            }
        });
    }

    /// Mark a peer connection as successful
    pub async fn mark_success(&self, peer_id: GossipPeerId, addr: SocketAddr) {
        let mut peers = self.peers.write().await;

        peers
            .entry(peer_id)
            .and_modify(|p| {
                p.last_seen = SystemTime::now();
                p.consecutive_failures = 0;
                p.successful_connections = p.successful_connections.saturating_add(1);
                p.connection_attempts = p.connection_attempts.saturating_add(1);
            })
            .or_insert(CachedPeer {
                peer_id,
                socket_addr: addr,
                last_seen: SystemTime::now(),
                connection_attempts: 1,
                consecutive_failures: 0,
                successful_connections: 1,
            });
    }

    /// Mark a peer connection as failed
    pub async fn mark_failure(&self, peer_id: GossipPeerId, addr: SocketAddr) {
        let mut peers = self.peers.write().await;

        peers
            .entry(peer_id)
            .and_modify(|p| {
                p.consecutive_failures = p.consecutive_failures.saturating_add(1);
                p.connection_attempts = p.connection_attempts.saturating_add(1);
            })
            .or_insert(CachedPeer {
                peer_id,
                socket_addr: addr,
                last_seen: SystemTime::now(),
                connection_attempts: 1,
                consecutive_failures: 1,
                successful_connections: 0,
            });
    }

    /// Get all viable (non-stale) peers for bootstrap
    pub async fn get_viable_peers(&self) -> Vec<(GossipPeerId, SocketAddr)> {
        let peers = self.peers.read().await;

        let mut viable: Vec<_> = peers
            .values()
            .filter(|p| !p.is_stale(self.config.max_consecutive_failures, self.config.stale_timeout))
            .map(|p| (p.clone(), p.successful_connections, p.last_seen))
            .collect();

        // Sort by success count (descending) then by last_seen (descending)
        viable.sort_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2))
        });

        viable
            .into_iter()
            .map(|(p, _, _)| (p.peer_id, p.socket_addr))
            .collect()
    }

    /// Get cache statistics
    pub async fn stats(&self) -> PeerCacheStats {
        let peers = self.peers.read().await;

        let total_peers = peers.len();
        let viable_peers = peers
            .values()
            .filter(|p| !p.is_stale(self.config.max_consecutive_failures, self.config.stale_timeout))
            .count();

        PeerCacheStats {
            total_peers,
            viable_peers,
            cache_file: self.cache_file.clone(),
        }
    }

    /// Bootstrap connections in parallel batches
    ///
    /// Connects to peers from the cache in parallel batches, stopping early once
    /// `required_connections` successful connections are established.
    ///
    /// # Arguments
    /// * `connect_fn` - Async function to connect to a peer, returns Result<bool> (true = success)
    /// * `batch_size` - Number of peers per batch (default: 50)
    /// * `max_concurrent` - Maximum concurrent connections (default: 100)
    /// * `required_connections` - Stop after this many successes (default: 10)
    ///
    /// # Returns
    /// Vec of successfully connected (PeerId, SocketAddr) tuples
    pub async fn bootstrap_parallel<F, Fut>(
        &self,
        connect_fn: F,
        batch_size: Option<usize>,
        max_concurrent: Option<usize>,
        required_connections: Option<usize>,
    ) -> Result<Vec<(GossipPeerId, SocketAddr)>>
    where
        F: Fn(GossipPeerId, SocketAddr) -> Fut + Clone + Send + 'static,
        Fut: std::future::Future<Output = Result<bool>> + Send,
    {
        use futures::stream::{FuturesUnordered, StreamExt};

        let batch_size = batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
        let max_concurrent = max_concurrent.unwrap_or(DEFAULT_MAX_CONCURRENT);
        let required_connections = required_connections.unwrap_or(DEFAULT_REQUIRED_CONNECTIONS);

        info!(
            "Starting parallel bootstrap: batch_size={}, max_concurrent={}, required={}",
            batch_size, max_concurrent, required_connections
        );

        // Get all viable peers sorted by success rate
        let viable_peers = self.get_viable_peers().await;

        if viable_peers.is_empty() {
            warn!("No viable peers in cache for bootstrap");
            return Ok(Vec::new());
        }

        info!("Found {} viable peers for bootstrap", viable_peers.len());

        let mut successful_connections = Vec::new();
        let mut processed = 0;

        // Process peers in batches
        for batch in viable_peers.chunks(batch_size) {
            if successful_connections.len() >= required_connections {
                info!(
                    "Reached required connections ({}), stopping bootstrap",
                    required_connections
                );
                break;
            }

            debug!(
                "Processing batch of {} peers (total processed: {})",
                batch.len(),
                processed
            );

            // Create futures for this batch
            let mut futures = FuturesUnordered::new();

            for (peer_id, addr) in batch.iter().take(max_concurrent) {
                let peer_id = *peer_id;
                let addr = *addr;
                let connect_fn = connect_fn.clone();

                futures.push(async move {
                    let timeout_duration = Duration::from_secs(5);
                    match tokio::time::timeout(timeout_duration, connect_fn(peer_id, addr)).await {
                        Ok(Ok(true)) => (peer_id, addr, true),
                        Ok(Ok(false)) => (peer_id, addr, false),
                        Ok(Err(e)) => {
                            debug!("Connection to {} ({}) failed: {}", peer_id, addr, e);
                            (peer_id, addr, false)
                        }
                        Err(_) => {
                            debug!("Connection to {} ({}) timed out", peer_id, addr);
                            (peer_id, addr, false)
                        }
                    }
                });
            }

            // Collect results from this batch
            while let Some((peer_id, addr, success)) = futures.next().await {
                if success {
                    debug!("✓ Successfully connected to {} ({})", peer_id, addr);
                    self.mark_success(peer_id, addr).await;
                    successful_connections.push((peer_id, addr));

                    // Check if we've reached the required count
                    if successful_connections.len() >= required_connections {
                        info!(
                            "Reached required connections ({}), stopping batch",
                            required_connections
                        );
                        break;
                    }
                } else {
                    debug!("✗ Failed to connect to {} ({})", peer_id, addr);
                    self.mark_failure(peer_id, addr).await;
                }

                processed += 1;
            }
        }

        info!(
            "Bootstrap complete: {}/{} successful connections (processed {} peers)",
            successful_connections.len(),
            required_connections,
            processed
        );

        Ok(successful_connections)
    }
}

/// Peer cache statistics
#[derive(Debug, Clone)]
pub struct PeerCacheStats {
    /// Total number of peers in cache
    pub total_peers: usize,
    /// Number of viable (non-stale) peers
    pub viable_peers: usize,
    /// Cache file location
    pub cache_file: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_peer_cache_creation() {
        let cache = PeerCache::default_testing().expect("Failed to create cache");
        let stats = cache.stats().await;
        assert_eq!(stats.total_peers, 0);
    }

    #[tokio::test]
    async fn test_mark_success() {
        let cache = PeerCache::default_testing().expect("Failed to create cache");
        let peer_id = GossipPeerId::new([1u8; 32]);
        let addr: SocketAddr = "127.0.0.1:8080".parse().expect("Invalid address");

        cache.mark_success(peer_id, addr).await;

        let stats = cache.stats().await;
        assert_eq!(stats.total_peers, 1);
        assert_eq!(stats.viable_peers, 1);
    }

    #[tokio::test]
    async fn test_mark_failure() {
        let cache = PeerCache::default_testing().expect("Failed to create cache");
        let peer_id = GossipPeerId::new([2u8; 32]);
        let addr: SocketAddr = "127.0.0.1:8081".parse().expect("Invalid address");

        // Mark failures up to threshold
        for _ in 0..DEFAULT_MAX_FAILURES {
            cache.mark_failure(peer_id, addr).await;
        }

        let stats = cache.stats().await;
        assert_eq!(stats.total_peers, 1);
        // Should now be stale due to consecutive failures
        assert_eq!(stats.viable_peers, 0);
    }

    #[tokio::test]
    async fn test_custom_config() {
        let config = PeerCacheConfig::testing()
            .cache_filename("test_custom.bin")
            .max_capacity(100);

        let cache = PeerCache::new(config).expect("Failed to create cache");
        let stats = cache.stats().await;

        assert!(stats.cache_file.to_string_lossy().contains("test_custom.bin"));
    }
}
