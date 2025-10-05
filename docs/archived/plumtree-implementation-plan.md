# Plumtree Implementation Plan

**Based on**: plumtree-design.md
**Target**: Production-ready Plumtree dissemination
**Timeline**: Phase 1 (Core Implementation)

## Implementation Checklist

### Step 1: Core Data Structures ✅ or ❌

#### 1.1 Message Types
- [ ] `GossipMessage` struct (header + optional payload + signature)
- [ ] Message serialization/deserialization (bincode)
- [ ] Message ID type alias: `type MessageId = [u8; 32]`

#### 1.2 Topic State
- [ ] `TopicState` struct with:
  - [ ] `eager_peers: HashSet<PeerId>`
  - [ ] `lazy_peers: HashSet<PeerId>`
  - [ ] `message_cache: LruCache<MessageId, CachedMessage>`
  - [ ] `pending_ihave: Vec<MessageId>`
  - [ ] `outstanding_iwants: HashMap<MessageId, (PeerId, Instant)>`
  - [ ] `subscribers: Vec<mpsc::Sender<(PeerId, Bytes)>>`

#### 1.3 Cached Message
- [ ] `CachedMessage` struct (payload + timestamp + metadata)
- [ ] TTL enforcement (5 minute expiry)
- [ ] Size tracking for cache limits

### Step 2: Core Protocol Handlers ✅ or ❌

#### 2.1 Publish Handler (Local Origin)
```rust
async fn handle_publish(&mut self, topic: TopicId, payload: Bytes) -> Result<()>
```
- [ ] Calculate msg_id using BLAKE3
- [ ] Create MessageHeader
- [ ] Sign with ML-DSA (placeholder: use identity crate later)
- [ ] Add to message_cache
- [ ] Send EAGER to eager_peers
- [ ] Batch msg_id to pending_ihave
- [ ] Deliver to local subscribers

#### 2.2 EAGER Handler (Remote Origin)
```rust
async fn handle_eager(&mut self, from: PeerId, msg: GossipMessage) -> Result<()>
```
- [ ] Verify signature
- [ ] Check message_cache for duplicate
- [ ] **If NEW**:
  - [ ] Add to cache
  - [ ] Deliver to subscribers
  - [ ] Forward to eager_peers (except sender)
  - [ ] Batch msg_id to pending_ihave for lazy_peers
- [ ] **If DUPLICATE**:
  - [ ] PRUNE: move sender from eager → lazy
  - [ ] Log optimization event

#### 2.3 IHAVE Handler
```rust
async fn handle_ihave(&mut self, from: PeerId, msg_ids: Vec<MessageId>) -> Result<()>
```
- [ ] For each msg_id:
  - [ ] Check if in cache → skip
  - [ ] Check if in outstanding_iwants → skip
  - [ ] Send IWANT to sender
  - [ ] Track in outstanding_iwants with timestamp

#### 2.4 IWANT Handler
```rust
async fn handle_iwant(&mut self, from: PeerId, msg_ids: Vec<MessageId>) -> Result<()>
```
- [ ] For each msg_id:
  - [ ] Check message_cache
  - [ ] **If FOUND**:
    - [ ] Send EAGER with payload to sender
    - [ ] GRAFT: move sender from lazy → eager
  - [ ] **If NOT FOUND**:
    - [ ] Log warning (shouldn't happen in normal operation)

### Step 3: Tree Optimization ✅ or ❌

#### 3.1 PRUNE (Duplicate Optimization)
- [ ] Detect duplicate EAGER
- [ ] Move peer from eager → lazy
- [ ] Ensure lazy set doesn't exceed capacity
- [ ] Log topology change

#### 3.2 GRAFT (Pull Optimization)
- [ ] Detect IWANT (peer wants faster route)
- [ ] Move peer from lazy → eager
- [ ] Ensure eager set doesn't exceed degree limit (8-12)
- [ ] If over limit: demote random eager peer to lazy

#### 3.3 Degree Maintenance
- [ ] Periodic check: `maintain_degree()` every 30s
- [ ] If eager_peers.len() < 6: promote random lazy peers
- [ ] If eager_peers.len() > 12: demote random eager peers
- [ ] Prefer low-latency peers for eager set

### Step 4: IHAVE Batching ✅ or ❌

#### 4.1 Batch Accumulator
- [ ] Add msg_id to `pending_ihave` on new message
- [ ] Track batch size (max 1024)

#### 4.2 Flush Logic
- [ ] Timer-based flush: every 100ms
- [ ] Size-based flush: when batch reaches 1024
- [ ] Send IHAVE to all lazy_peers
- [ ] Clear pending_ihave after send

#### 4.3 Batch Message Format
```rust
struct IHaveBatch {
    msg_ids: Vec<MessageId>,  // ≤1024
}
```

### Step 5: Message Cache Management ✅ or ❌

#### 5.1 LRU Cache
- [ ] Use `lru` crate for cache implementation
- [ ] Max 10,000 entries (configurable)
- [ ] Evict oldest on insert when full

#### 5.2 TTL Enforcement
- [ ] Store timestamp with each entry
- [ ] Periodic cleanup task: every 60s
- [ ] Remove entries older than 5 minutes

#### 5.3 Size Limits
- [ ] Track total cache size in bytes
- [ ] Max cache size: 100MB
- [ ] Evict LRU entries when size limit reached

### Step 6: Integration ✅ or ❌

#### 6.1 Transport Integration
- [ ] Send messages via `QuicTransport::open_stream(StreamType::Gossip)`
- [ ] Receive messages via `QuicTransport::accept_stream()`
- [ ] Background task: `spawn_receive_loop()`

#### 6.2 Membership Integration
- [ ] Initialize eager/lazy split from `Membership::active_view()`
- [ ] Subscribe to membership changes
- [ ] Add new peers to lazy set initially

#### 6.3 Signature Integration (Placeholder)
- [ ] Create placeholder signer (returns empty vec)
- [ ] Create placeholder verifier (returns Ok(true))
- [ ] TODO comment: integrate saorsa-pqc ML-DSA later

### Step 7: Error Handling & Safety ✅ or ❌

#### 7.1 Network Errors
- [ ] Handle connection failures gracefully
- [ ] Retry failed sends to eager_peers
- [ ] Move failed peers to lazy after N retries

#### 7.2 Validation Errors
- [ ] Drop messages with invalid signatures
- [ ] Track invalid signature rate per peer
- [ ] Demote/ban peers with >10% invalid rate

#### 7.3 Resource Limits
- [ ] Enforce max message size (1MB)
- [ ] Enforce max IHAVE batch size (1024)
- [ ] Enforce cache limits

#### 7.4 ZERO PANIC POLICY
- [ ] No `.unwrap()` in production code
- [ ] No `.expect()` in production code
- [ ] Proper error propagation with `Result<T, E>`
- [ ] Defensive checks on all external input

### Step 8: Testing ✅ or ❌

#### 8.1 Unit Tests
- [ ] test_message_routing_eager_peers
- [ ] test_message_routing_lazy_peers
- [ ] test_duplicate_detection_prune
- [ ] test_iwant_graft
- [ ] test_ihave_batching
- [ ] test_cache_eviction_lru
- [ ] test_cache_eviction_ttl
- [ ] test_degree_maintenance

#### 8.2 Integration Tests
- [ ] test_3_node_triangle
- [ ] test_10_node_mesh_broadcast
- [ ] test_churn_stability
- [ ] test_partition_healing (deferred to anti-entropy)

#### 8.3 Property Tests (with proptest)
- [ ] prop_all_peers_receive_message
- [ ] prop_no_duplicate_forwards
- [ ] prop_tree_stays_connected

### Step 9: Documentation ✅ or ❌

#### 9.1 Code Documentation
- [ ] Module-level docs explaining Plumtree
- [ ] Struct docs for all public types
- [ ] Method docs with examples
- [ ] Algorithm docs for PRUNE/GRAFT

#### 9.2 Usage Examples
- [ ] Example: Simple pub/sub
- [ ] Example: Multi-topic broadcast
- [ ] Example: Observing tree topology

### Step 10: Performance & Quality Gates ✅ or ❌

#### 10.1 Compilation
- [ ] `cargo check --all-features` → 0 errors
- [ ] `cargo clippy --all-features -- -D warnings` → 0 warnings
- [ ] `cargo fmt --check` → properly formatted

#### 10.2 Testing
- [ ] `cargo test --all-features` → 100% pass rate
- [ ] No ignored tests
- [ ] No disabled assertions

#### 10.3 Performance Benchmarks
- [ ] Measure P50/P95 latency in 10-node network
- [ ] Measure redundancy ratio (copies per message)
- [ ] Verify eager degree stays in 6-12 range

## Implementation Order

### Phase 1A: Foundation (Day 1)
1. ✅ Design document (done)
2. ✅ Implementation plan (done)
3. ⏳ Data structures (Step 1)
4. ⏳ Publish handler (Step 2.1)

### Phase 1B: Core Protocol (Day 2)
5. ⏳ EAGER handler (Step 2.2)
6. ⏳ IHAVE handler (Step 2.3)
7. ⏳ IWANT handler (Step 2.4)
8. ⏳ Basic tests

### Phase 1C: Optimization (Day 3)
9. ⏳ PRUNE/GRAFT logic (Step 3)
10. ⏳ IHAVE batching (Step 4)
11. ⏳ Cache management (Step 5)

### Phase 1D: Integration (Day 4)
12. ⏳ Transport integration (Step 6)
13. ⏳ Membership integration (Step 6)
14. ⏳ Integration tests (Step 8.2)

### Phase 1E: Validation (Day 5)
15. ⏳ Comprehensive testing (Step 8)
16. ⏳ Documentation (Step 9)
17. ⏳ Quality gates (Step 10)

## Success Criteria

✅ **Functional**:
- All peers receive all published messages
- No duplicate forwards on same link
- Tree optimizes over time (PRUNE/GRAFT work)

✅ **Quality**:
- Zero compilation warnings
- 100% test pass rate
- All public APIs documented

✅ **Performance**:
- P95 latency < 2s in 10-node network
- Redundancy ratio < 1.5x
- Eager degree converges to 6-8

## Current Status

**Overall Progress**: 10% (Design Complete)

**Next Action**: Implement Step 1 (Core Data Structures)

---

**Review Notes**:
- Design validated against SPEC.md Section 6
- Performance targets from audit.md
- Zero-panic policy enforced per CLAUDE.md
- Integration points identified and feasible
