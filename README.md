# Saorsa Gossip Overlay

A post-quantum secure gossip overlay network implementation for the Communitas project.

## Overview

Saorsa Gossip replaces traditional DHT discovery with a PQC-secure gossip overlay based on contact graphs and existing groups. It provides:

- **Transport**: QUIC with PQC (ML-KEM-768 + ML-DSA-65)
- **Membership**: HyParView + SWIM for connectivity and failure detection
- **Dissemination**: Plumtree broadcast with peer scoring
- **Presence**: MLS-based beacons and FOAF queries
- **CRDT Sync**: Delta-CRDTs with anti-entropy
- **Bluetooth Fallback**: Mesh bridge for collapse scenarios

## Architecture

The implementation is organized as a Rust workspace with the following crates:

- `types` - Core types and identities (PeerId, TopicId, wire formats)
- `transport` - QUIC transport adapter
- `membership` - HyParView + SWIM membership protocols
- `pubsub` - Plumtree broadcast and dissemination
- `presence` - Presence beacons and user discovery
- `crdt-sync` - Delta-CRDT synchronization
- `groups` - MLS group management
- `identity` - ML-DSA identity and key management

## Building

```bash
cargo build --release
```

## Testing

```bash
cargo test --all
```

## License

MIT OR Apache-2.0

## References

- QUIC: RFC 9000/9001
- MLS: RFC 9420
- PQC: FIPS 203/204/205
- HyParView, SWIM, Plumtree, GossipSub v1.1
