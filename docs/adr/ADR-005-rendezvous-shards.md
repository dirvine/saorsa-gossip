# ADR-005: Rendezvous Shards (DHT Replacement)

## Status

Accepted (2025-12-24)

## Context

Decentralized networks need mechanisms for peers to find each other and content. Traditional DHT (Distributed Hash Table) approaches have significant security and performance issues:

| DHT Problem | Description |
|-------------|-------------|
| **Sybil attacks** | Attacker creates many identities near target's key range |
| **Eclipse attacks** | Attacker surrounds victim with malicious nodes |
| **Routing attacks** | Malicious nodes intercept/drop lookup messages |
| **Churn sensitivity** | Rapid join/leave destabilizes routing |
| **Latency** | O(log N) hops for lookups add RTT |

We needed a discovery mechanism that:
- Resists Sybil and routing attacks
- Works with gossip-based topology (no separate DHT overlay)
- Has O(1) lookup latency after subscription
- Supports both peer and content discovery

## Decision

Replace DHT with **Rendezvous Shards**: deterministic content-addressed topic partitioning where providers announce to shards and seekers subscribe.

### Shard Calculation

```rust
const SHARD_COUNT: u16 = 65536; // 2^16 = 64k shards

pub fn calculate_shard(target: &[u8]) -> u16 {
    let hash = blake3::hash(b"saorsa-rendezvous")
        .chain_update(target)
        .finalize();

    // Take last 2 bytes as shard ID
    let bytes = hash.as_bytes();
    u16::from_le_bytes([bytes[30], bytes[31]])
}

// Examples:
// calculate_shard(b"site:example.site") -> 0x3A5F
// calculate_shard(b"user:alice@domain") -> 0x7C21
// calculate_shard(b"topic:general-chat") -> 0xB4E8
```

### Provider Announcement

Providers (who have content/services) publish to their shard:

```rust
#[derive(Serialize, Deserialize)]
pub struct ProviderSummary {
    /// What the provider offers
    pub target: Vec<u8>,
    /// Provider's peer ID
    pub provider_id: PeerId,
    /// Connection hints
    pub addr_hints: Vec<SocketAddr>,
    /// Capabilities/metadata
    pub capabilities: Capabilities,
    /// TTL (typically 1 hour)
    pub not_after: u64,
    /// ML-DSA signature
    pub signature: Vec<u8>,
}

impl ProviderSummary {
    pub fn publish(&self, pubsub: &PubSub) -> Result<()> {
        let shard = calculate_shard(&self.target);
        let topic = TopicId::from_string(format!("saorsa:rendezvous:{:04x}", shard));
        pubsub.publish(topic, self.serialize()).await
    }
}
```

### Seeker Discovery

Seekers subscribe to the shard for their target:

```rust
impl Seeker {
    pub async fn find_providers(&self, target: &[u8]) -> Vec<ProviderSummary> {
        let shard = calculate_shard(target);
        let topic = TopicId::from_string(format!("saorsa:rendezvous:{:04x}", shard));

        // Subscribe to shard topic
        let mut stream = self.pubsub.subscribe(topic).await?;

        // Collect provider summaries for timeout period
        let mut providers = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);

        while Instant::now() < deadline {
            tokio::select! {
                Some(msg) = stream.next() => {
                    if let Ok(summary) = ProviderSummary::deserialize(&msg.payload) {
                        if summary.verify() && summary.target == target {
                            providers.push(summary);
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline.into()) => break,
            }
        }

        providers
    }
}
```

### Use Cases

#### 1. User Discovery (FIND_USER)

```rust
// Alice wants to find Bob
let target = format!("user:{}", bob_peer_id);
let shard = calculate_shard(target.as_bytes());
let providers = seeker.find_providers(target.as_bytes()).await;

// Bob would have announced himself:
let summary = ProviderSummary::new("user:{bob_id}", bob_addrs);
summary.publish(&pubsub).await;
```

#### 2. Site Discovery

```rust
// Finding a Saorsa Site
let target = format!("site:{}", site_fingerprint);
let providers = seeker.find_providers(target.as_bytes()).await;

// Site hosts announce:
let summary = ProviderSummary::new("site:{fingerprint}", host_addrs);
summary.publish(&pubsub).await;
```

#### 3. Service Discovery

```rust
// Finding file-sharing peers
let target = b"service:file-sharing:v1";
let providers = seeker.find_providers(target).await;
```

### Shard Distribution

With k=16 bits (65,536 shards), at steady state:

| Network Size | Providers/Shard | Seekers/Shard |
|--------------|-----------------|---------------|
| 10,000 peers | ~0.15 avg | Many share shards |
| 100,000 peers | ~1.5 avg | Good distribution |
| 1,000,000 peers | ~15 avg | Natural load balance |

Shards with more providers see more traffic, but gossip scales logarithmically.

### Security Properties

#### Sybil Resistance

Unlike DHT where attackers target key ranges:
- Shards are 65,536 random bins
- Attacker needs to compromise gossip topology, not key space
- HyParView's random topology resists targeted attacks

#### No Routing Attacks

Unlike DHT recursive lookups:
- Direct subscription to shard topic
- No intermediate nodes can drop/modify lookups
- Provider summaries are signed (can't be forged)

#### Partition Tolerance

Unlike DHT stabilization protocols:
- Works with any connected subset
- Providers re-announce after partitions heal
- No "correct" routing table to maintain

## Consequences

### Benefits

1. **Sybil resistant**: No key-space proximity attacks
2. **O(1) discovery**: Direct subscription, no routing hops
3. **Partition tolerant**: Works with gossip overlay
4. **Simple**: No complex routing tables or stabilization
5. **Verifiable**: Signed summaries prevent forgery

### Trade-offs

1. **Bandwidth**: Must subscribe to shard (receives all traffic)
2. **Cold start**: Takes time for provider summaries to gossip
3. **65k limit**: Finite shards (but sufficient for most use cases)
4. **Announcement required**: Providers must actively publish

### Shard Topic Subscription Strategy

To avoid subscribing to many shards:

```rust
impl ShardManager {
    /// Only subscribe to shards we're actively seeking
    fn subscribe_strategy(&self) -> Vec<TopicId> {
        let mut shards = HashSet::new();

        // Shards for our own announcements
        for target in &self.our_targets {
            shards.insert(calculate_shard(target));
        }

        // Shards for active searches
        for target in &self.active_searches {
            shards.insert(calculate_shard(target));
        }

        // Convert to topic IDs
        shards.into_iter()
            .map(|s| TopicId::from_string(format!("saorsa:rendezvous:{:04x}", s)))
            .collect()
    }
}
```

## Alternatives Considered

### 1. Kademlia DHT

Use XOR-based routing for peer and content discovery.

**Rejected because**:
- Sybil attacks on key ranges
- Eclipse attacks on victims
- Routing attacks drop/modify lookups
- Additional overlay maintenance
- O(log N) lookup latency

### 2. Chord DHT

Use consistent hashing ring for routing.

**Rejected because**:
- Similar attack surface to Kademlia
- Finger table maintenance overhead
- Churn sensitivity

### 3. Centralized Registry

Use servers to track providers.

**Rejected because**:
- Single point of failure
- Censorship vulnerability
- Requires infrastructure

### 4. Gossip-Based Search

Flood search queries through network.

**Rejected because**:
- O(N) messages per query
- No bounded response time
- Bandwidth explosion

### 5. More Shards (k=20)

Use 1M shards instead of 65k.

**Rejected because**:
- Many empty shards in small networks
- More topics to manage
- 65k sufficient for 10M+ network

## References

- **Rendezvous Protocol**: Inspired by libp2p rendezvous
- **Implementation**: `crates/rendezvous/src/`
- **Shard calculation**: `crates/rendezvous/src/lib.rs:calculate_shard()`
- **Related**: ADR-004 (Seedless Bootstrap uses similar topic-based discovery)
