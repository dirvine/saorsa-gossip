# Plumtree Implementation - Dependency Analysis

**Purpose**: Identify all required dependencies before implementation
**Status**: Pre-implementation analysis

## Required Crates

### 1. Existing Dependencies (Already Available)

#### From Workspace
```toml
# Already in workspace.dependencies
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
bytes = "1.0"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
```

#### From Project
```toml
saorsa-gossip-types = { path = "../types" }
saorsa-gossip-transport = { path = "../transport" }
saorsa-gossip-membership = { path = "../membership" }
```

### 2. New Dependencies Required

#### 2.1 LRU Cache
**Crate**: `lru = "0.12"`
**Purpose**: Message cache with automatic eviction
**Usage**:
```rust
use lru::LruCache;
let mut cache: LruCache<MessageId, CachedMessage> = LruCache::new(10_000);
```

#### 2.2 Bincode Serialization
**Crate**: `bincode = "1.3"`
**Purpose**: Efficient binary serialization for wire format
**Usage**:
```rust
let bytes = bincode::serialize(&message)?;
let msg: GossipMessage = bincode::deserialize(&bytes)?;
```

#### 2.3 Tracing (Optional but Recommended)
**Crate**: `tracing = "0.1"`
**Purpose**: Structured logging for debugging
**Usage**:
```rust
tracing::debug!(peer_id = %peer, msg_id = ?msg_id, "PRUNE: duplicate EAGER");
```

#### 2.4 Dashmap (Thread-safe HashMap)
**Crate**: `dashmap = "6.0"`
**Purpose**: Concurrent access to topic state without RwLock contention
**Alternative**: Stick with `tokio::sync::RwLock<HashMap<>>` (current approach)
**Decision**: Use RwLock initially, optimize later if needed

### 3. Dependency Summary for Cargo.toml

#### Add to `crates/pubsub/Cargo.toml`:
```toml
[dependencies]
saorsa-gossip-types = { path = "../types" }
saorsa-gossip-transport = { path = "../transport" }
saorsa-gossip-membership = { path = "../membership" }

# Existing
tokio = { workspace = true }
tokio-util = { workspace = true }
async-trait = { workspace = true }
bytes = { workspace = true }
anyhow = { workspace = true }
serde = { workspace = true }

# NEW - Add these
lru = "0.12"              # Message cache
bincode = "1.3"           # Wire serialization
tracing = "0.1"           # Structured logging
blake3 = { workspace = true }  # For msg_id calculation (already in types)

[dev-dependencies]
proptest = { workspace = true }
tokio-test = "0.4"        # For async test utilities
```

#### Add to `Cargo.toml` workspace.dependencies:
```toml
[workspace.dependencies]
# ... existing ...
lru = "0.12"
bincode = "1.3"
tracing = "0.1"
tokio-test = "0.4"
```

## Feature Analysis

### Required tokio Features (Already Enabled)
- ✅ `tokio::sync::mpsc` - For subscriber channels
- ✅ `tokio::sync::RwLock` - For shared state
- ✅ `tokio::time` - For batching timers
- ✅ `tokio::spawn` - For background tasks

### Required bytes Features
- ✅ Default features sufficient

### LRU Cache Configuration
- `LruCache::new(NonZeroUsize)` - Capacity limit
- `LruCache::put()` - Insert with eviction
- `LruCache::get()` - Retrieve without moving
- `LruCache::pop_lru()` - Manual eviction if needed

## Type Compatibility Check

### MessageId Type
```rust
// In types crate:
pub type MessageId = [u8; 32];

// Must be:
- Hash (for HashMap keys) ✅ - arrays implement Hash
- Eq (for HashMap keys) ✅ - arrays implement Eq
- Clone (for caching) ✅ - arrays implement Copy
- Serialize (for bincode) ✅ - arrays implement Serialize
```

### PeerId Type
```rust
// Already defined in types crate
pub struct PeerId([u8; 32]);

// Implements:
- Hash ✅
- Eq ✅
- Clone ✅
- Serialize ✅
```

### Bytes Type
```rust
// From bytes crate
pub struct Bytes { ... }

// Implements:
- Clone (cheap, Arc-based) ✅
- AsRef<[u8]> ✅
- Serialize ✅
```

## Potential Conflicts

### 1. blake3 Dependency
- **Already in types crate**: ✅ No conflict
- **Already in workspace**: ✅ Version aligned

### 2. serde Dependency
- **Already everywhere**: ✅ No conflict
- **Version consistency**: ✅ Using workspace version

### 3. tokio Dependency
- **Already in transport**: ✅ No conflict
- **Feature alignment**: ✅ Using "full" features

## Architecture Compatibility

### With Transport Layer
```rust
// QuicTransport provides:
async fn open_stream(&self, peer: PeerId, stream_type: StreamType)
    -> Result<(SendStream, RecvStream)>

// We need:
async fn send_message(&self, peer: PeerId, msg: GossipMessage) -> Result<()> {
    let (mut send, _recv) = transport.open_stream(peer, StreamType::Gossip).await?;
    let bytes = bincode::serialize(&msg)?;
    send.write_all(&bytes).await?;
    Ok(())
}
```
✅ **Compatible** - Can wrap QuicTransport

### With Membership Layer
```rust
// Membership provides:
fn active_view(&self) -> Vec<PeerId>

// We need:
async fn initialize_peers(&mut self, topic: TopicId) {
    let peers = membership.active_view();
    // Start all as eager, optimize later
    topic_state.eager_peers.extend(peers);
}
```
✅ **Compatible** - Simple query interface

### With Types Layer
```rust
// Types provides:
pub struct MessageHeader { ... }
pub enum MessageKind { Eager, IHave, IWant, ... }
impl MessageHeader {
    pub fn calculate_msg_id(...) -> [u8; 32]
}
```
✅ **Compatible** - Exact types we need

## Size Estimates

### Memory per Topic
```rust
// Worst case (10,000 cached messages @ 1KB each):
- message_cache: ~10MB (10,000 * 1KB)
- eager_peers: ~320 bytes (10 peers * 32 bytes)
- lazy_peers: ~3.2KB (100 peers * 32 bytes)
- pending_ihave: ~32KB (1024 * 32 bytes)
- outstanding_iwants: ~64 bytes (2 * 32 bytes avg)

Total per topic: ~10.4MB
```

### Scalability
- 10 topics: ~104MB
- 100 topics: ~1GB
- **Recommendation**: Make cache size configurable, default 1,000 entries per topic

## Wire Format Size

### EAGER Message
```rust
struct GossipMessage {
    header: MessageHeader,     // 72 bytes (ver:1 + topic:32 + msg_id:32 + kind:1 + hop:1 + ttl:1 + padding:4)
    payload: Option<Bytes>,    // 0-1MB
    signature: Vec<u8>,        // ~2420 bytes (ML-DSA-65)
}

// Total: ~2.5KB (header + sig) + payload
// Max: ~1.002MB (1MB payload + 2.5KB overhead)
```

### IHAVE Message
```rust
struct IHaveBatch {
    msg_ids: Vec<[u8; 32]>,   // ≤1024 * 32 = 32KB
}

// Total: ~32KB max per IHAVE
```

### IWANT Message
```rust
struct IWantRequest {
    msg_ids: Vec<[u8; 32]>,   // Usually 1-10 * 32 = 32-320 bytes
}

// Total: <1KB typically
```

## Performance Considerations

### Serialization Overhead
- **bincode**: ~1μs for small structs (<1KB)
- **blake3**: ~1μs for 1KB hash
- **Total overhead**: <10μs per message (negligible)

### Lock Contention
- **RwLock** on topic_state: Potential bottleneck
- **Mitigation**: Use `tokio::sync::RwLock` (async-aware)
- **Alternative**: Shard by topic_id hash if needed

### Memory Allocation
- **Bytes** is Arc-based (cheap clone)
- **LruCache** pre-allocates capacity
- **HashMap** grows dynamically (use `with_capacity()`)

## Testing Dependencies

### Unit Tests
```toml
[dev-dependencies]
tokio-test = "0.4"        # Async test utilities
proptest = { workspace = true }  # Property-based testing
```

### Mock Transport
```rust
// Create in-memory mock:
struct MockTransport {
    sent_messages: Arc<Mutex<Vec<(PeerId, GossipMessage)>>>,
}

// Use for testing without real QUIC
```

## Dependency Decision Matrix

| Dependency | Required? | Alternative | Decision |
|------------|-----------|-------------|----------|
| `lru` | ✅ Yes | Write own LRU | ✅ Use crate (battle-tested) |
| `bincode` | ✅ Yes | serde_json | ✅ Use bincode (efficient) |
| `tracing` | ⚠️ Optional | println! | ✅ Use tracing (production-ready) |
| `dashmap` | ❌ No | RwLock<HashMap> | ❌ Use RwLock (simpler) |
| `tokio-test` | ✅ Yes (dev) | Manual async setup | ✅ Use crate (convenience) |

## Final Dependency List

### Add to `crates/pubsub/Cargo.toml`:
```toml
[dependencies]
# ... existing ...
lru = "0.12"
bincode = "1.3"
tracing = "0.1"

[dev-dependencies]
proptest = { workspace = true }
tokio-test = "0.4"
```

### Add to `Cargo.toml`:
```toml
[workspace.dependencies]
# ... existing ...
lru = "0.12"
bincode = "1.3"
tracing = "0.1"
tokio-test = "0.4"
```

## Validation Checklist

- [x] All dependencies compatible with existing crates
- [x] No version conflicts
- [x] Size estimates acceptable (<1GB for 100 topics)
- [x] Wire format efficient (<3KB overhead)
- [x] Testing dependencies identified
- [x] Performance overhead negligible (<10μs)

## Next Steps

1. Update `Cargo.toml` with new dependencies
2. Verify `cargo check` passes
3. Begin Step 1 of implementation plan (Data Structures)

---

**Status**: ✅ Dependency analysis complete, ready to implement
