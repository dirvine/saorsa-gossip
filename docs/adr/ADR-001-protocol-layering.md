# ADR-001: Protocol Layering (HyParView + SWIM + Plumtree)

## Status

Accepted (2025-12-24)

## Context

Building a scalable gossip overlay network requires solving three distinct problems:

1. **Topology Management**: How do peers discover and maintain connections?
2. **Failure Detection**: How do we detect when peers become unreachable?
3. **Message Dissemination**: How do we broadcast messages efficiently?

Traditional approaches suffer from fundamental limitations:

| Approach | Problem |
|----------|---------|
| Full mesh | O(N^2) connections, doesn't scale past ~50 nodes |
| Flooding | O(N^2) messages per broadcast, bandwidth explosion |
| Leader-based | Single point of failure, centralization |
| DHT routing | Sybil attacks, routing attacks, high latency |

We needed a protocol stack that:
- Scales to thousands of peers with O(log N) connections
- Detects failures in sub-5-second timeframes
- Broadcasts with O(N) message overhead, not O(N^2)
- Operates without any central infrastructure

## Decision

Adopt a **three-protocol stack** combining proven academic research:

### 1. HyParView for Topology Management

HyParView (Hybrid Partial View) maintains two peer views:

```rust
struct MembershipState {
    /// Connected peers used for routing (8-12 peers)
    active_view: HashSet<PeerId>,
    /// Candidates for healing partitions (64-128 peers)
    passive_view: HashSet<PeerId>,
}
```

**Key mechanisms**:
- **Shuffle**: Every 30s, exchange random peer subsets with neighbors
- **Promotion**: When active peer fails, promote from passive view
- **Join**: New peers contact seed, receive active view subset

**Why HyParView**:
- Self-healing: Passive view provides repair candidates
- Partition resistant: Random shuffling spreads peer knowledge
- Scalable: O(log N) active connections regardless of network size

### 2. SWIM for Failure Detection

SWIM (Scalable Weakly-consistent Infection-style Membership) provides fast, accurate failure detection:

```rust
enum PeerState {
    Alive,      // Responding to probes
    Suspect,    // Failed direct probe, attempting indirect
    Dead,       // No response after timeout, remove from view
}

struct SwimConfig {
    probe_interval: Duration,      // 1 second
    probe_timeout: Duration,       // 500ms direct, 200ms indirect
    suspect_timeout: Duration,     // 5 seconds
    indirect_probe_count: usize,   // 3 peers
}
```

**Protocol flow**:
1. Every second, probe one random active peer with PING
2. If no ACK within 500ms, request K=3 random peers to probe indirectly
3. If no indirect ACK within 200ms, mark peer as SUSPECT
4. After 5s in SUSPECT state, mark as DEAD and remove from active view

**State dissemination**: Piggyback state changes on PING/ACK messages for O(log N) convergence.

### 3. Plumtree for Message Dissemination

Plumtree (Push-Lazy-Push Multicast Tree) builds efficient spanning trees:

```rust
struct PlumtreeState {
    /// Peers that receive EAGER pushes (spanning tree)
    eager_peers: HashSet<PeerId>,
    /// Peers that receive IHAVE digests (lazy backup)
    lazy_peers: HashSet<PeerId>,
    /// Recently seen messages for deduplication
    message_cache: LruCache<MessageId, CachedMessage>,
}

enum PlumtreeMessage {
    Eager { payload: Bytes },           // Full message push
    IHave { message_ids: Vec<MessageId> }, // Lazy digest
    IWant { message_ids: Vec<MessageId> }, // Pull request
    Prune,                              // Move to lazy
    Graft,                              // Move to eager
}
```

**Tree optimization**:
- **PRUNE**: When duplicate received via eager, send PRUNE to second sender
- **GRAFT**: When IHAVE reveals missing message, send GRAFT to add eager link
- **Anti-entropy**: IBLT exchange every 30s identifies missing messages

**Why Plumtree**:
- O(N) messages for broadcast (tree), not O(N^2) (flood)
- Lazy links provide redundancy without bandwidth cost
- Self-optimizing tree adapts to network conditions

### Protocol Integration

The three protocols compose cleanly:

```
+-------------------+
|    Plumtree       |  Efficient broadcast
|   (Dissemination) |  Uses active view for routing
+--------+----------+
         |
+--------v----------+
|      SWIM         |  Failure detection
| (Failure Detect)  |  Updates active/passive views
+--------+----------+
         |
+--------v----------+
|    HyParView      |  Topology management
|    (Topology)     |  Maintains peer connectivity
+--------+----------+
         |
+--------v----------+
|   QUIC Transport  |  Reliable streams
+-------------------+
```

## Consequences

### Benefits

1. **Scalability**: O(log N) connections, O(N) broadcast overhead
2. **Fast failure detection**: Sub-5-second detection with low false positive rate
3. **Self-healing**: Passive view repairs partitions automatically
4. **No single point of failure**: Fully decentralized operation
5. **Proven protocols**: Based on peer-reviewed academic research

### Trade-offs

1. **Complexity**: Three protocols to implement and debug
2. **Tuning**: Multiple parameters (shuffle interval, probe timeout, etc.)
3. **Message overhead**: SWIM probes add baseline traffic (~1 msg/sec/peer)

### Configuration

Default parameters optimized for typical deployments:

| Parameter | Default | Rationale |
|-----------|---------|-----------|
| Active view size | 8-12 | Balance connectivity vs overhead |
| Passive view size | 64-128 | Sufficient repair candidates |
| Shuffle interval | 30s | Spread peer knowledge gradually |
| SWIM probe interval | 1s | Quick detection without flooding |
| Suspect timeout | 5s | Low false positives on transient failures |
| Eager peer count | 6-12 | Reliable tree coverage |

## Alternatives Considered

### 1. Chord/Kademlia DHT

Using a DHT for both routing and membership.

**Rejected because**:
- Sybil attacks: Attacker can control key ranges
- Routing attacks: Malicious nodes intercept lookups
- Higher latency: Recursive routing adds RTT
- Eclipse attacks: Targeted isolation of victims

### 2. Pure Flooding

Broadcast every message to all connected peers.

**Rejected because**:
- O(N^2) message overhead
- Bandwidth explosion at scale
- Network congestion under load

### 3. Gossip with Random Selection

Probabilistic forwarding to random peer subset.

**Rejected because**:
- No delivery guarantee
- Variable latency
- Difficult to tune reliably

### 4. Central Broker

Route all messages through central servers.

**Rejected because**:
- Single point of failure
- Scalability bottleneck
- Requires infrastructure operators

## References

- **HyParView**: Leitao, Pereira, Rodrigues. "HyParView: A Membership Protocol for Reliable Gossip-Based Broadcast" (DSN 2007)
- **SWIM**: Das, Gupta, Motivala. "SWIM: Scalable Weakly-consistent Infection-style process group Membership protocol" (DSN 2002)
- **Plumtree**: Leitao, Pereira, Rodrigues. "Epidemic Broadcast Trees" (SRDS 2007)
- **Implementation**: `crates/membership/src/`, `crates/pubsub/src/`
- **Tests**: `tests/property_tests.rs` (protocol invariants)
