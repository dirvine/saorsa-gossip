# SPEC2.md Implementation Quick Start Guide

**Goal**: Get started implementing SPEC2.md immediately with clear, actionable steps.

---

## Prerequisites ✅

- [x] Rust 1.82+ installed
- [x] `saorsa-pqc 0.3.14` available (crates.io)
- [x] `saorsa-mls 0.3.0` available (local path dependency)
- [x] `ant-quic 0.10.1` available (crates.io)
- [x] Workspace compiles with zero errors

---

## Phase 1: Coordinator Adverts (Start Here)

### Step 1: Create Coordinator Crate

```bash
cd crates/
cargo new coordinator --lib
```

**Update `Cargo.toml`**:
```toml
[dependencies]
saorsa-gossip-types = { version = "0.1.2", path = "../types" }
saorsa-pqc = { workspace = true }
saorsa-mls = { workspace = true }
serde = { workspace = true }
bincode = { workspace = true }
bytes = { workspace = true }
anyhow = { workspace = true }
blake3 = { workspace = true }

[dev-dependencies]
proptest = { workspace = true }
```

Add to workspace `Cargo.toml`:
```toml
members = [
    # ... existing members ...
    "crates/coordinator",
]
```

---

### Step 2: Define Coordinator Advert Type

**File**: `crates/coordinator/src/lib.rs`

```rust
use saorsa_gossip_types::PeerId;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Coordinator roles per SPEC2 §8
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorRoles {
    pub coordinator: bool,
    pub reflector: bool,
    pub rendezvous: bool,
    pub relay: bool,
}

/// NAT class detection per SPEC2 §8
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NatClass {
    Eim,       // Endpoint-Independent Mapping
    Edm,       // Endpoint-Dependent Mapping
    Symmetric, // Symmetric NAT
    Unknown,
}

/// Address hint for NAT traversal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddrHint {
    pub addr: SocketAddr,
    pub observed_at: u64, // unix timestamp ms
}

/// Coordinator Advertisement per SPEC2 §8
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorAdvert {
    /// Protocol version
    pub v: u8,
    /// Peer identifier
    pub peer: PeerId,
    /// Coordinator roles
    pub roles: CoordinatorRoles,
    /// Address hints for connection
    pub addr_hints: Vec<AddrHint>,
    /// NAT classification
    pub nat_class: NatClass,
    /// Valid not before (unix ms)
    pub not_before: u64,
    /// Valid not after (unix ms)
    pub not_after: u64,
    /// Local-only advisory score
    pub score: i32,
    /// ML-DSA signature over all fields except sig
    pub sig: Vec<u8>,
}

impl CoordinatorAdvert {
    /// Create a new coordinator advert (unsigned)
    pub fn new(
        peer: PeerId,
        roles: CoordinatorRoles,
        addr_hints: Vec<AddrHint>,
        nat_class: NatClass,
        validity_duration_ms: u64,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            v: 1,
            peer,
            roles,
            addr_hints,
            nat_class,
            not_before: now,
            not_after: now + validity_duration_ms,
            score: 0,
            sig: Vec::new(), // Will be filled by sign()
        }
    }

    /// Sign the advert with ML-DSA
    pub fn sign(&mut self, signing_key: &saorsa_pqc::MlDsaSecretKey) -> anyhow::Result<()> {
        // TODO: Implement signing logic
        // 1. Serialize all fields except sig
        // 2. Sign with ML-DSA-65
        // 3. Store signature in sig field
        unimplemented!("ML-DSA signing not yet implemented")
    }

    /// Verify the advert signature
    pub fn verify(&self, public_key: &saorsa_pqc::MlDsaPublicKey) -> anyhow::Result<bool> {
        // TODO: Implement verification logic
        unimplemented!("ML-DSA verification not yet implemented")
    }

    /// Check if advert is currently valid
    pub fn is_valid(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        now >= self.not_before && now <= self.not_after
    }

    /// Serialize to CBOR wire format
    pub fn to_cbor(&self) -> anyhow::Result<Vec<u8>> {
        // TODO: Use CBOR serialization (consider serde_cbor or ciborium)
        // For now, use bincode as placeholder
        Ok(bincode::serialize(self)?)
    }

    /// Deserialize from CBOR wire format
    pub fn from_cbor(data: &[u8]) -> anyhow::Result<Self> {
        // TODO: Use CBOR deserialization
        Ok(bincode::deserialize(data)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advert_creation() {
        let peer = PeerId::new([1u8; 32]);
        let roles = CoordinatorRoles {
            coordinator: true,
            reflector: true,
            rendezvous: false,
            relay: false,
        };
        let addr_hints = vec![];
        let nat_class = NatClass::Eim;

        let advert = CoordinatorAdvert::new(
            peer,
            roles,
            addr_hints,
            nat_class,
            3600_000, // 1 hour
        );

        assert_eq!(advert.v, 1);
        assert_eq!(advert.peer, peer);
        assert!(advert.roles.coordinator);
        assert!(advert.is_valid());
    }

    #[test]
    fn test_advert_serialization() {
        let peer = PeerId::new([1u8; 32]);
        let roles = CoordinatorRoles {
            coordinator: true,
            reflector: false,
            rendezvous: false,
            relay: false,
        };
        let advert = CoordinatorAdvert::new(
            peer,
            roles,
            vec![],
            NatClass::Unknown,
            1000,
        );

        let bytes = advert.to_cbor().expect("serialization should succeed");
        let decoded = CoordinatorAdvert::from_cbor(&bytes)
            .expect("deserialization should succeed");

        assert_eq!(advert.peer, decoded.peer);
        assert_eq!(advert.v, decoded.v);
    }
}
```

---

### Step 3: Run Tests

```bash
cargo test -p saorsa-gossip-coordinator
```

**Expected**: Tests compile and pass (sign/verify will be unimplemented panics for now).

---

### Step 4: Implement ML-DSA Signing

**Add to `CoordinatorAdvert` impl**:

```rust
use saorsa_pqc::{MlDsa65, MlDsaOperations};

pub fn sign(&mut self, signing_key: &saorsa_pqc::MlDsaSecretKey) -> anyhow::Result<()> {
    // Serialize all fields except signature
    let mut to_sign = bincode::serialize(&(
        self.v,
        &self.peer,
        &self.roles,
        &self.addr_hints,
        &self.nat_class,
        self.not_before,
        self.not_after,
        self.score,
    ))?;

    // Sign with ML-DSA-65
    let signer = MlDsa65::new();
    let signature = signer.sign(signing_key, &to_sign)?;

    self.sig = signature.to_bytes().to_vec();
    Ok(())
}

pub fn verify(&self, public_key: &saorsa_pqc::MlDsaPublicKey) -> anyhow::Result<bool> {
    // Reconstruct signed data
    let to_verify = bincode::serialize(&(
        self.v,
        &self.peer,
        &self.roles,
        &self.addr_hints,
        &self.nat_class,
        self.not_before,
        self.not_after,
        self.score,
    ))?;

    // Verify signature
    let verifier = MlDsa65::new();
    let sig = saorsa_pqc::MlDsaSignature::from_bytes(&self.sig)?;

    Ok(verifier.verify(public_key, &to_verify, &sig)?)
}
```

---

### Step 5: Add Coordinator Topic

**File**: `crates/coordinator/src/topic.rs`

```rust
use saorsa_gossip_types::TopicId;

/// Well-known topic for coordinator advertisements
pub fn coordinator_topic() -> TopicId {
    // BLAKE3("saorsa-coordinator-topic")
    let hash = blake3::hash(b"saorsa-coordinator-topic");
    TopicId::new(*hash.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_topic_deterministic() {
        let topic1 = coordinator_topic();
        let topic2 = coordinator_topic();
        assert_eq!(topic1, topic2, "Topic should be deterministic");
    }
}
```

---

### Step 6: Add Advert Cache

**File**: `crates/coordinator/src/cache.rs`

```rust
use crate::CoordinatorAdvert;
use lru::LruCache;
use saorsa_gossip_types::PeerId;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// LRU cache for coordinator advertisements
pub struct AdvertCache {
    cache: Arc<Mutex<LruCache<PeerId, CoordinatorAdvert>>>,
}

impl AdvertCache {
    /// Create a new advert cache
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(cap))),
        }
    }

    /// Insert an advert (if valid and not expired)
    pub fn insert(&self, advert: CoordinatorAdvert) -> bool {
        if !advert.is_valid() {
            return false;
        }

        let mut cache = self.cache.lock().unwrap();
        cache.put(advert.peer, advert);
        true
    }

    /// Get an advert by peer ID
    pub fn get(&self, peer: &PeerId) -> Option<CoordinatorAdvert> {
        let mut cache = self.cache.lock().unwrap();
        cache.get(peer).cloned()
    }

    /// Get all cached adverts, sorted by score
    pub fn get_all_sorted(&self) -> Vec<CoordinatorAdvert> {
        let cache = self.cache.lock().unwrap();
        let mut adverts: Vec<_> = cache.iter()
            .filter(|(_, advert)| advert.is_valid())
            .map(|(_, advert)| advert.clone())
            .collect();

        adverts.sort_by(|a, b| b.score.cmp(&a.score));
        adverts
    }

    /// Prune expired adverts
    pub fn prune_expired(&self) {
        let mut cache = self.cache.lock().unwrap();
        let to_remove: Vec<_> = cache.iter()
            .filter(|(_, advert)| !advert.is_valid())
            .map(|(peer, _)| *peer)
            .collect();

        for peer in to_remove {
            cache.pop(&peer);
        }
    }
}
```

---

## Next Steps

### Week 1 Checklist

- [ ] Create coordinator crate
- [ ] Implement `CoordinatorAdvert` type
- [ ] Add ML-DSA signing/verification
- [ ] Create coordinator topic
- [ ] Implement advert cache
- [ ] Add integration with pubsub for gossip
- [ ] Write integration test: node publishes advert, peer receives

### Week 2: Rendezvous Shards

- [ ] Create rendezvous crate
- [ ] Implement shard ID calculation
- [ ] Define `ProviderSummary` type
- [ ] Implement Bloom filter for summaries

### Week 3: Presence Beacons

- [ ] Update presence crate
- [ ] Integrate with MLS exporter API
- [ ] Implement beacon generation and encryption
- [ ] Add find-user logic

---

## Useful Commands

```bash
# Build everything
cargo build --all-features

# Run all tests
cargo test --all

# Check specific crate
cargo check -p saorsa-gossip-coordinator

# Format code
cargo fmt --all

# Lint
cargo clippy --all-features --all-targets -- -D warnings

# Generate docs
cargo doc --all-features --no-deps --open
```

---

## Common Patterns

### Creating a New Crate

1. `cargo new crates/my-crate --lib`
2. Add to workspace `Cargo.toml` members
3. Add dependencies to crate's `Cargo.toml`
4. Create `lib.rs` with public API
5. Write tests in `#[cfg(test)]` modules

### TDD Workflow

1. Write failing test
2. Implement minimal code to pass
3. Refactor
4. Repeat

### Integration Testing

Create `tests/integration_test.rs`:
```rust
use saorsa_gossip_coordinator::*;

#[tokio::test]
async fn test_coordinator_advert_flow() {
    // Setup
    // Execute
    // Assert
}
```

---

## Resources

- **SPEC2.md**: Full protocol specification
- **SPEC2_IMPLEMENTATION_PLAN.md**: Detailed 8-week roadmap
- **saorsa-pqc docs**: https://docs.rs/saorsa-pqc
- **saorsa-mls source**: `../saorsa-mls/src/`
- **ant-quic docs**: https://docs.rs/ant-quic

---

**Ready to code? Start with Step 1!** 🚀
