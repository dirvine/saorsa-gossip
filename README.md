# Saorsa Gossip Overlay

[![Crates.io](https://img.shields.io/crates/v/saorsa-gossip)](https://crates.io/crates/saorsa-gossip)
[![Documentation](https://docs.rs/saorsa-gossip/badge.svg)](https://docs.rs/saorsa-gossip)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

A **post-quantum secure gossip overlay network** for decentralized peer-to-peer communication. Designed to replace DHT-based discovery with a contact-graph-aware gossip protocol, providing low-latency broadcast, partition tolerance, and quantum-resistant cryptography.

## 🎯 Overview

Saorsa Gossip implements a complete gossip overlay with:

- **Post-Quantum Cryptography**: ML-KEM-768 + ML-DSA-65 + ChaCha20-Poly1305 (FIPS 203/204)
- **QUIC Transport**: Low-latency, NAT-traversal with connection migration
- **MLS Group Security**: RFC 9420 compliant end-to-end encryption with ChaCha20-Poly1305
- **Gossip Protocols**: HyParView, SWIM, Plumtree for robust dissemination
- **Local-First CRDTs**: Delta-CRDTs with anti-entropy synchronization
- **No DHT**: Contact-graph-based discovery, no global directory

**Status**: 🚧 **Active Development** - Core architecture complete, transport layer integrated, implementation ~85% complete (see [SPEC.md](SPEC.md))

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Saorsa Gossip                         │
│                                                         │
│  ┌─────────┐  ┌──────────┐  ┌─────────┐  ┌─────────┐ │
│  │Presence │  │  PubSub  │  │  CRDT   │  │ Groups  │ │
│  │(Beacons)│  │(Plumtree)│  │  Sync   │  │  (MLS)  │ │
│  └────┬────┘  └─────┬────┘  └────┬────┘  └────┬────┘ │
│       │             │            │            │       │
│  ┌────┴─────────────┴────────────┴────────────┴────┐ │
│  │            Membership Layer                      │ │
│  │         (HyParView + SWIM)                       │ │
│  └──────────────────┬───────────────────────────────┘ │
│                     │                                  │
│  ┌──────────────────┴───────────────────────────────┐ │
│  │          Transport Layer (ant-quic)               │ │
│  │   QUIC + PQC TLS 1.3 + NAT Traversal            │ │
│  └──────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Core Components

| Crate | Purpose | Status |
|-------|---------|--------|
| **types** | Core types (TopicId, PeerId, MessageHeader) | ✅ Complete |
| **transport** | QUIC transport with ant-quic 0.10.1 | ✅ Complete |
| **membership** | HyParView + SWIM protocols | ✅ Wired |
| **pubsub** | Plumtree broadcast dissemination | ✅ Wired |
| **presence** | Beacon broadcasting and FOAF queries | ⚠️ Partial |
| **crdt-sync** | Delta-CRDTs (OR-Set, LWW-Register) | ✅ Complete |
| **groups** | MLS group management | ✅ Complete |
| **identity** | ML-DSA key management | ✅ Complete |

## 🚀 Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
saorsa-gossip-types = "0.1"
saorsa-gossip-transport = "0.1"
saorsa-gossip-membership = "0.1"
saorsa-gossip-pubsub = "0.1"
```

### Basic Usage

```rust
use saorsa_gossip_types::{TopicId, PeerId};
use saorsa_gossip_membership::{Membership, HyParViewMembership};
use saorsa_gossip_pubsub::{PubSub, PlumtreePubSub};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a topic for your group
    let topic = TopicId::new([1u8; 32]);

    // Initialize membership layer
    let membership = HyParViewMembership::default();
    membership.join(vec!["127.0.0.1:8080".to_string()]).await?;

    // Initialize pub/sub
    let pubsub = PlumtreePubSub::new();
    let mut rx = pubsub.subscribe(topic);

    // Publish a message
    pubsub.publish(topic, bytes::Bytes::from("Hello, gossip!")).await?;

    // Receive messages
    while let Some((peer_id, data)) = rx.recv().await {
        println!("Received from {}: {:?}", peer_id, data);
    }

    Ok(())
}
```

## 📚 Protocol Specifications

### Membership (HyParView + SWIM)

- **HyParView**: Partial views for connectivity
  - Active view: 8-12 peers (routing)
  - Passive view: 64-128 peers (healing)
  - Periodic shuffle: every 30s

- **SWIM**: Failure detection
  - Probe interval: 1s
  - Suspect timeout: 3s
  - Piggyback membership deltas

### Dissemination (Plumtree)

- **EAGER** push along spanning tree
- **IHAVE** digests to non-tree links (batch ≤ 1024)
- **IWANT** pull on demand
- **Anti-entropy**: every 30s with message-ID sketches
- **Peer scoring**: mesh gating for quality

### Presence & Discovery

- **Beacons**: MLS exporter-derived tags, ML-DSA signed
  - TTL: 10-15 minutes
  - Encrypted to group with ChaCha20-Poly1305

- **FOAF Queries**: Friends-of-friends discovery
  - Fanout: 3
  - TTL: 3-4 hops
  - No DHT, no global directory

### CRDTs

- **OR-Set**: For membership tracking
- **LWW-Register**: For scalar values
- **Delta-CRDTs**: Bandwidth-efficient synchronization
- **IBLT**: Reconciliation for large sets

## 🔐 Security

### Post-Quantum Cryptography

- **ML-KEM-768**: Key encapsulation (FIPS 203)
- **ML-DSA-65**: Digital signatures (FIPS 204) - default
- **SLH-DSA**: Hash-based signatures (FIPS 205 / SPHINCS+) - available for long-term security
  - 12 parameter sets: SHA2/SHAKE variants at 128/192/256-bit security
  - Trade-offs: fast (larger sigs) vs small (smaller sigs)
- **ChaCha20-Poly1305**: AEAD symmetric encryption (quantum-resistant)
- **MLS**: Group messaging (RFC 9420)

Provided by:
- [`saorsa-pqc`](https://crates.io/crates/saorsa-pqc) v0.3.14+ - PQC primitives including ML-KEM, ML-DSA, SLH-DSA, and ChaCha20-Poly1305
- [`saorsa-mls`](https://crates.io/crates/saorsa-mls) - MLS protocol

### Threat Model

| Attack | Mitigation |
|--------|-----------|
| Spam/Sybil | Invited joins, capability checks, scoring |
| Eclipse | HyParView shuffles, passive diversity |
| Replay | Per-topic nonces, signature checks, expiry |
| Partition | Plumtree lazy links, anti-entropy |

## 🛠️ Development

### Building

```bash
# Build all crates
cargo build --release

# Run tests
cargo test --all

# Run with all features
cargo build --all-features
```

### Testing

```bash
# Unit tests
cargo test --all

# Integration tests (when implemented)
cargo test --test integration

# Benchmarks (when implemented)
cargo bench
```

### Code Quality

```bash
# Format code
cargo fmt --all

# Lint with Clippy (zero warnings enforced)
cargo clippy --all-features --all-targets -- -D warnings

# Generate documentation
cargo doc --all-features --no-deps --open
```

## 📖 Documentation

- [**SPEC.md**](SPEC.md) - Complete protocol specification
- [**API Docs**](https://docs.rs/saorsa-gossip) - Rust API documentation
- [**Examples**](examples/) - Usage examples (coming soon)

### Crate Documentation

- [saorsa-gossip-types](https://docs.rs/saorsa-gossip-types) - Core types and wire format
- [saorsa-gossip-transport](https://docs.rs/saorsa-gossip-transport) - QUIC transport
- [saorsa-gossip-membership](https://docs.rs/saorsa-gossip-membership) - HyParView + SWIM
- [saorsa-gossip-pubsub](https://docs.rs/saorsa-gossip-pubsub) - Plumtree broadcast
- [saorsa-gossip-presence](https://docs.rs/saorsa-gossip-presence) - Presence beacons
- [saorsa-gossip-crdt-sync](https://docs.rs/saorsa-gossip-crdt-sync) - CRDT synchronization
- [saorsa-gossip-groups](https://docs.rs/saorsa-gossip-groups) - MLS groups
- [saorsa-gossip-identity](https://docs.rs/saorsa-gossip-identity) - Identity management

## 🗺️ Roadmap

### ✅ Phase 1: Foundation (Complete)
- [x] Core types and traits
- [x] CRDT implementations (OR-Set, LWW)
- [x] MLS group wrapper
- [x] PQC identity management

### ✅ Phase 2: Protocols (Complete - 100%)
- [x] HyParView trait definitions
- [x] SWIM trait definitions
- [x] Plumtree trait definitions
- [x] Membership wired to transport
- [x] Broadcast dissemination wired to transport
- [ ] Anti-entropy mechanisms

### ✅ Phase 3: Transport (Complete - 100%)
- [x] ant-quic 0.10.1 QUIC integration
- [x] NAT traversal with hole punching
- [x] Ed25519 keypair generation
- [x] Stream multiplexing (mship, pubsub, bulk)
- [x] Message send/receive with routing

### 📋 Phase 4: Advanced Features (Planned)
- [ ] Presence beacon system
- [ ] FOAF discovery
- [ ] IBLT reconciliation
- [ ] Peer scoring and mesh gating
- [ ] Bluetooth mesh fallback

### 📋 Phase 5: Production (Future)
- [ ] 100-node test harness
- [ ] Performance benchmarks
- [ ] Security audit
- [ ] Production deployment guide

## 📊 Performance Targets

| Metric | Target | Status |
|--------|--------|--------|
| Broadcast P50 latency | < 500ms | 🔄 Testing |
| Broadcast P95 latency | < 2s | 🔄 Testing |
| Failure detection | < 5s | 🔄 Testing |
| Memory per node | < 50MB | 🔄 Testing |
| Messages/sec/node | > 100 | 🔄 Testing |

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Priorities

**High Priority** (blocking):
1. Complete QUIC transport implementation
2. Implement Plumtree EAGER/IHAVE/IWANT
3. Implement proper message ID derivation

**Medium Priority** (important):
4. Complete HyParView join/shuffle
5. Complete SWIM probe/suspect
6. Add anti-entropy protocols

**Low Priority** (enhancement):
7. Performance optimization
8. Comprehensive testing
9. Example applications

## 📜 License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## 🙏 Acknowledgments

Built on top of:
- [`ant-quic`](https://crates.io/crates/ant-quic) - QUIC transport with NAT traversal
- [`saorsa-pqc`](https://crates.io/crates/saorsa-pqc) - Post-quantum cryptography
- [`saorsa-mls`](https://crates.io/crates/saorsa-mls) - MLS group messaging

Inspired by:
- **Plumtree** - Efficient epidemic broadcast
- **HyParView** - Partial view membership protocol
- **SWIM** - Scalable failure detection
- **GossipSub** - Libp2p's gossip protocol

## 📞 Contact

- **Project**: [github.com/dirvine/saorsa-gossip](https://github.com/dirvine/saorsa-gossip)
- **Issues**: [github.com/dirvine/saorsa-gossip/issues](https://github.com/dirvine/saorsa-gossip/issues)
- **Author**: David Irvine ([@dirvine](https://github.com/dirvine))

---

**⚠️ Status**: This project is under active development. The core architecture is complete, transport layer integrated with ant-quic 0.10.1, and approximately 85% of the SPEC.md features are complete. Not recommended for production use until security audit.

See [SPEC.md](SPEC.md) for the complete technical specification and [Compliance Audit](docs/audit.md) for detailed implementation status.
