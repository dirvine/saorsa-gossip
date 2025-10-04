# Plumtree Dissemination Design

**Status**: Design Phase
**Version**: 0.1.0
**Date**: 2025-01-04

## 1. Protocol Overview

Plumtree (Epidemic Broadcast Trees) combines:
- **Eager push** (low latency via spanning tree)
- **Lazy push** (low overhead via gossip)
- **Self-optimization** (automatic tree repair)

### Core Invariant
Every peer maintains TWO sets of neighbors for each topic:
- **Eager peers** (tree links): Forward full messages immediately
- **Lazy peers** (gossip links): Send only message IDs (IHAVE)

## 2. Message Types (from SPEC.md)

```rust
MessageKind::Eager    // Full message with payload (tree)
MessageKind::IHave    // Message ID digest (gossip)
MessageKind::IWant    // Request for payload (pull)
```

## 3. Data Structures Required

### 3.1 Per-Topic State

```rust
struct TopicState {
    // Spanning tree peers (forward EAGER)
    eager_peers: HashSet<PeerId>,

    // Non-tree peers (send IHAVE only)
    lazy_peers: HashSet<PeerId>,

    // Message cache: msg_id -> (payload, timestamp)
    // Needed for: duplicate detection, IWANT responses
    message_cache: LruCache<MessageId, (Bytes, Instant)>,

    // Pending IHAVE batch (≤1024 message IDs per batch)
    pending_ihave: Vec<MessageId>,

    // Outstanding IWANT requests: msg_id -> peer we requested from
    outstanding_iwants: HashMap<MessageId, PeerId>,

    // Local subscribers
    subscribers: Vec<mpsc::Sender<(PeerId, Bytes)>>,
}
```

### 3.2 Message Wrapper

```rust
#[derive(Clone)]
struct GossipMessage {
    header: MessageHeader,
    payload: Option<Bytes>,  // None for IHAVE
    signature: Vec<u8>,      // ML-DSA signature
}
```

## 4. Protocol State Machine

### 4.1 Publishing a Message (Local Origin)

```
1. Create MessageHeader with msg_id = BLAKE3(topic || epoch || signer || payload_hash)
2. Sign header with ML-DSA
3. Add to local message_cache
4. Send EAGER to all eager_peers
5. Batch msg_id into pending_ihave for lazy_peers
6. Deliver to local subscribers
```

### 4.2 Receiving EAGER (from peer P)

```
IF msg_id NOT in message_cache:
    1. Add to message_cache
    2. Deliver to local subscribers
    3. Forward EAGER to (eager_peers - P)
    4. Batch msg_id to pending_ihave for (lazy_peers + possibly P)
ELSE (duplicate):
    1. PRUNE link: move P from eager_peers to lazy_peers
    2. Send PRUNE message to P (optimization, not in minimal spec)
```

### 4.3 Receiving IHAVE (from peer P)

```
FOR EACH msg_id in IHAVE batch:
    IF msg_id NOT in message_cache AND msg_id NOT in outstanding_iwants:
        1. Send IWANT(msg_id) to P
        2. Track in outstanding_iwants[msg_id] = P
        3. Start timeout (if no response in 2s, try another peer)
```

### 4.4 Receiving IWANT (from peer P)

```
FOR EACH msg_id in IWANT:
    IF msg_id in message_cache:
        1. Send EAGER(msg_id, payload) to P
        2. GRAFT link: move P from lazy_peers to eager_peers
    ELSE:
        // We don't have it (rare edge case)
        Send PRUNE to P
```

## 5. Tree Optimization Logic

### 5.1 PRUNE (Duplicate Detection)
- **Trigger**: Receive duplicate EAGER from peer P
- **Action**: Move P from eager → lazy
- **Effect**: Reduces redundant traffic, maintains connectivity via IHAVE

### 5.2 GRAFT (Pull Optimization)
- **Trigger**: Receive IWANT from peer P
- **Action**: Move P from lazy → eager
- **Effect**: Repairs tree when lazy path is faster than eager path

### 5.3 Mesh Quality Maintenance
- Periodically check eager_peers.len()
- If too low (<3): promote random lazy peers
- If too high (>12): demote random eager peers to lazy
- Prioritize peers with low latency and high reliability

## 6. IHAVE Batching (SPEC: ≤1024)

```rust
const IHAVE_BATCH_SIZE: usize = 1024;
const IHAVE_FLUSH_INTERVAL_MS: u64 = 100;  // Flush every 100ms

// Batching strategy:
1. Accumulate msg_ids in pending_ihave
2. When batch reaches 1024 OR 100ms timer fires:
   - Send IHAVE to each lazy_peer
   - Clear pending_ihave
```

## 7. Message Cache Management

### 7.1 Cache Size
- LRU with 10,000 entry limit (configurable)
- Evict oldest when full

### 7.2 Entry TTL
- Keep messages for 5 minutes (300s)
- Periodic cleanup of expired entries
- Needed for delayed IWANT responses

### 7.3 Deduplication
- Check cache on EAGER receipt
- Ignore duplicates (but use for PRUNE optimization)

## 8. Integration Requirements

### 8.1 Transport Layer Integration

```rust
// Need from transport:
async fn send_to_peer(&self, peer: PeerId, message: GossipMessage) -> Result<()>;
async fn receive_from_peer(&self) -> Result<(PeerId, GossipMessage)>;

// Stream type: StreamType::Gossip
```

### 8.2 Membership Layer Integration

```rust
// Need from membership:
fn get_active_peers(&self, topic: TopicId) -> Vec<PeerId>;

// Initialize eager/lazy split:
// - Start with all active peers as eager
// - Optimize over time based on duplicates
```

### 8.3 Signature Verification

```rust
// Need from identity:
async fn verify_signature(
    &self,
    header: &MessageHeader,
    signature: &[u8],
    peer_id: &PeerId
) -> Result<bool>;
```

## 9. Security Considerations

### 9.1 Rate Limiting
- Max messages/sec per peer: 100
- Max IHAVE batch frequency: 10/sec per peer
- Violators get demoted to lazy or dropped

### 9.2 Signature Verification
- All EAGER messages MUST have valid ML-DSA signature
- Drop unsigned or invalid messages
- Track invalid signature attempts for peer scoring

### 9.3 Replay Protection
- msg_id includes epoch (time-based)
- Reject messages with epoch > current_time + 5min
- Reject messages with epoch < current_time - 1hour

### 9.4 Mesh Gating
- Minimum peer reputation score to join eager set
- Kick out peers with >10% invalid signatures
- Prioritize low-latency, high-uptime peers

## 10. Anti-Entropy (Separate Implementation)

**Deferred to separate task** (SPEC: every 30s)

```rust
// High-level strategy:
1. Exchange IBLT summaries of message_cache
2. Reconcile differences
3. Pull missing messages
4. Update tree topology based on latency observations
```

## 11. Implementation Phases

### Phase 1: Core Plumtree (THIS TASK)
- [x] Design document (this file)
- [ ] Implement TopicState and message routing
- [ ] Implement EAGER/IHAVE/IWANT handlers
- [ ] Implement PRUNE/GRAFT optimization
- [ ] Integrate with transport layer
- [ ] Comprehensive unit tests

### Phase 2: Quality & Performance
- [ ] IHAVE batching with timer
- [ ] Message cache with LRU eviction
- [ ] Peer scoring and mesh gating
- [ ] Rate limiting per peer
- [ ] Integration tests with simulated network

### Phase 3: Anti-Entropy (Separate Task)
- [ ] IBLT-based reconciliation
- [ ] Periodic sync (30s interval)
- [ ] Partition healing

## 12. Testing Strategy

### 12.1 Unit Tests
- Message routing logic (EAGER → eager_peers, IHAVE → lazy_peers)
- Duplicate detection (prune on duplicate EAGER)
- IWANT/GRAFT logic (pull → promotion)
- Cache eviction (LRU + TTL)

### 12.2 Integration Tests
- 3-node triangle: verify tree forms
- 10-node mesh: verify all nodes receive messages
- Partition & heal: verify convergence
- Churn: add/remove nodes, verify stability

### 12.3 Property Tests
- All peers receive all messages (reliability)
- Each message forwarded at most once per link (efficiency)
- Tree remains connected (liveness)

## 13. Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Broadcast latency (P50) | <500ms | Time from publish to all peers receive |
| Broadcast latency (P95) | <2s | 95th percentile |
| Redundancy ratio | <1.5x | Avg copies received per peer |
| Eager peer degree | 6-8 | Spanning tree fan-out |
| Message cache size | <100MB | At 10K messages |

## 14. Open Questions

1. **Epoch source**: Where does `epoch` come from in msg_id calculation?
   - **Answer**: System timestamp (SystemTime::now())

2. **Initial tree bootstrap**: How to initialize eager/lazy split?
   - **Answer**: Start with all peers as eager, optimize via PRUNE

3. **Signature format**: What exactly gets signed?
   - **Answer**: Entire MessageHeader (serialize then sign)

4. **Payload size limits**: Max message size?
   - **Answer**: 1MB (enforced by transport MTU)

## 15. References

- **Plumtree Paper**: "Epidemic Broadcast Trees" (Leitão et al., 2007)
- **GossipSub v1.1**: Peer scoring and mesh quality (libp2p spec)
- **SPEC.md Section 6**: Saorsa-gossip dissemination requirements
- **audit.md**: Current compliance gaps (~40% for dissemination)

---

**Next Steps**:
1. Review this design with stakeholders
2. Implement Phase 1 (core Plumtree)
3. Write comprehensive tests
4. Measure performance vs targets
