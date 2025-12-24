# ADR-003: Delta-CRDT Synchronization

## Status

Accepted (2025-12-24)

## Context

Peer-to-peer systems need to replicate state across nodes that may:

1. **Operate offline**: Users disconnect, move between networks
2. **Experience partitions**: Network splits create divergent state
3. **Reconnect after gaps**: Days or weeks between sync opportunities
4. **Apply concurrent edits**: Multiple users modify same data simultaneously

Traditional approaches have significant limitations:

| Approach | Problem |
|----------|---------|
| Last-writer-wins | Data loss on concurrent edits |
| Locking | Unavailable during partitions |
| Consensus (Raft/Paxos) | Requires majority quorum, blocks on partitions |
| Operational Transform | Complex, limited data type support |

We needed a synchronization strategy that:
- Works offline with no coordination required
- Automatically merges concurrent edits without conflict
- Transfers only changes, not full state
- Supports rich data types (sets, registers, sequences)

## Decision

Adopt **Delta-CRDTs** (Conflict-free Replicated Data Types with delta-state transfer) with **IBLT-based anti-entropy**:

### Supported CRDT Types

#### 1. OR-Set (Observed-Remove Set)

For collections where items can be added and removed:

```rust
pub struct OrSet<T> {
    /// Elements mapped to their unique tags
    elements: HashMap<T, HashSet<UniqueTag>>,
    /// Tags that have been removed
    tombstones: HashSet<UniqueTag>,
}

impl<T: Clone + Eq + Hash> OrSet<T> {
    pub fn add(&mut self, item: T, replica_id: (PeerId, u64)) -> Delta {
        let tag = UniqueTag::new(replica_id);
        self.elements.entry(item).or_default().insert(tag);
        Delta::Add { item, tag }
    }

    pub fn remove(&mut self, item: &T) -> Delta {
        if let Some(tags) = self.elements.remove(item) {
            self.tombstones.extend(tags.clone());
            Delta::Remove { tags }
        }
    }

    pub fn merge(&mut self, other: &Self) {
        // Union of all elements with their tags
        for (item, tags) in &other.elements {
            self.elements.entry(item.clone())
                .or_default()
                .extend(tags.clone());
        }
        // Union of all tombstones
        self.tombstones.extend(&other.tombstones);
        // Remove tombstoned tags from elements
        for (_, tags) in self.elements.iter_mut() {
            tags.retain(|t| !self.tombstones.contains(t));
        }
    }

    pub fn contains(&self, item: &T) -> bool {
        self.elements.get(item)
            .map(|tags| !tags.is_empty())
            .unwrap_or(false)
    }
}
```

**Use cases**: Contact lists, group membership, tag sets

**Add-wins semantics**: If Alice adds "Charlie" while Bob removes "Charlie", after merge "Charlie" is present (the add had a new unique tag not covered by Bob's remove).

#### 2. LWW-Register (Last-Writer-Wins Register)

For scalar values where latest write should win:

```rust
pub struct LwwRegister<T> {
    value: T,
    timestamp: u64,
    writer: PeerId,
}

impl<T: Clone> LwwRegister<T> {
    pub fn write(&mut self, value: T, timestamp: u64, writer: PeerId) {
        if timestamp > self.timestamp ||
           (timestamp == self.timestamp && writer > self.writer) {
            self.value = value;
            self.timestamp = timestamp;
            self.writer = writer;
        }
    }

    pub fn merge(&mut self, other: &Self) {
        self.write(other.value.clone(), other.timestamp, other.writer);
    }

    pub fn read(&self) -> &T {
        &self.value
    }
}
```

**Use cases**: Profile fields (name, bio, avatar), status messages

**Tie-breaking**: On equal timestamps, higher PeerId wins (deterministic).

#### 3. DeltaCrdt Trait

All CRDTs implement a common trait for delta extraction:

```rust
pub trait DeltaCrdt {
    type Delta: Serialize + DeserializeOwned;

    /// Extract changes since given version
    fn delta(&self, since_version: u64) -> Option<Self::Delta>;

    /// Apply delta from another replica
    fn merge(&mut self, delta: &Self::Delta) -> Result<()>;

    /// Current version number
    fn version(&self) -> u64;
}
```

### Delta-State Transfer

Instead of sending full state, send only changes:

```
Full state sync:
  Alice's contacts (1000 entries) = 50KB
  Bob's contacts (1000 entries) = 50KB
  Total transfer: 100KB

Delta sync (Alice added 10 contacts):
  Delta: 10 new entries = 500 bytes
  Total transfer: 500 bytes

Savings: 200x reduction
```

### Anti-Entropy with IBLT

**Problem**: After partition heals, Alice has 500 updates Bob doesn't have, and vice versa. How to efficiently identify the difference?

**Solution**: Invertible Bloom Lookup Table (IBLT)

```rust
struct Iblt {
    /// Fixed-size buckets (512 bytes typical)
    buckets: Vec<IbltBucket>,
    /// Hash functions
    hash_count: usize,
}

impl Iblt {
    /// Encode set of message IDs into IBLT
    fn encode(message_ids: &[MessageId]) -> Self;

    /// Subtract another IBLT to find symmetric difference
    fn subtract(&self, other: &Iblt) -> Self;

    /// Decode the difference (what each side is missing)
    fn decode(&self) -> Result<(Vec<MessageId>, Vec<MessageId>)>;
}
```

**Protocol**:
1. Alice computes IBLT of her message IDs (fixed 512 bytes)
2. Alice sends IBLT to Bob
3. Bob subtracts his IBLT from Alice's
4. Result decodes to: (IDs Alice has that Bob doesn't, IDs Bob has that Alice doesn't)
5. Both request missing deltas from each other
6. CRDTs merge, state converges

**Efficiency**: O(d) bytes where d = symmetric difference size, not O(n) total messages.

### Sync Protocol

```rust
enum SyncMessage {
    /// Exchange IBLT digests
    AntiEntropy { iblt: Iblt, version: u64 },
    /// Request specific deltas
    DeltaRequest { message_ids: Vec<MessageId> },
    /// Send delta payload
    DeltaResponse { deltas: Vec<Delta> },
}
```

**Schedule**: Anti-entropy runs every 30-60 seconds with random active peer.

## Consequences

### Benefits

1. **Offline operation**: Edit locally, sync when reconnected
2. **Partition tolerance**: No coordination needed, merge deterministically
3. **Bandwidth efficiency**: 100x+ reduction via delta transfer
4. **Automatic conflict resolution**: CRDTs guarantee convergence
5. **Rich data types**: Sets, registers, counters, sequences

### Trade-offs

1. **Tombstone accumulation**: OR-Set tombstones grow over time
2. **Timestamp dependency**: LWW-Register needs synchronized clocks
3. **Semantic limits**: Some conflicts need application-level resolution
4. **Space overhead**: Each element carries unique tags

### Tombstone Garbage Collection

Tombstones can be garbage collected after sufficient time:

```rust
fn garbage_collect(&mut self, cutoff: Duration) {
    let now = SystemTime::now();
    self.tombstones.retain(|tag| {
        now.duration_since(tag.created).unwrap() < cutoff
    });
}
```

**Trade-off**: Early GC can resurrect deleted items if old add appears.
**Default**: 7 days before tombstone GC.

## Alternatives Considered

### 1. Full State Sync

Always send complete state, merge on receive.

**Rejected because**:
- O(n) transfer for every sync
- Doesn't scale with state size
- Wastes bandwidth on unchanged data

### 2. Operational Transformation (OT)

Transform operations based on concurrent edits.

**Rejected because**:
- Complex transformation functions
- Requires operation history
- Limited to text-like data types

### 3. Consensus-Based (Raft)

Use distributed consensus for all state changes.

**Rejected because**:
- Blocks during partitions
- Requires majority quorum
- High latency for writes

### 4. Event Sourcing

Store all events, replay to compute state.

**Rejected because**:
- Unbounded storage growth
- Replay time increases
- Complex compaction

### 5. Merkle-DAG Sync (like Git)

Track all versions in DAG, sync missing nodes.

**Rejected because**:
- Higher storage overhead
- Complex merge logic
- CRDTs simpler for our use cases

## References

- **Delta-CRDTs**: Almeida, Shoker, Baquero. "Delta State Replicated Data Types" (2018)
- **IBLT**: Goodrich, Mitzenmacher. "Invertible Bloom Lookup Tables" (2011)
- **OR-Set**: Shapiro et al. "A Comprehensive Study of Convergent and Commutative Replicated Data Types" (2011)
- **Implementation**: `crates/crdt-sync/src/`
- **Tests**: `tests/property_tests.rs` (CRDT properties)
