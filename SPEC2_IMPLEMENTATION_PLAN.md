# SPEC2.md Implementation and Testing Plan

**Version**: 1.0
**Date**: 2025-10-05
**Status**: Planning Phase
**Target**: Full implementation of SPEC2.md with comprehensive testing

---

## Executive Summary

This document outlines the complete implementation and testing plan for **SPEC2.md** - the Saorsa Gossip Overlay with PQC-only, no DNS, pure P2P architecture. With `saorsa-mls 0.3.0` now available on crates.io, we have all the necessary cryptographic primitives to implement the full specification.

### Key Dependencies (Ready)
- ✅ `saorsa-pqc 0.3.14` - ML-KEM-768, ML-DSA-65, ChaCha20-Poly1305
- ✅ `saorsa-mls 0.3.0` - MLS group management with PQC
- ✅ `ant-quic 0.10.1` - QUIC transport with NAT traversal

---

## Current Status Assessment

### Completed Components (Phase 1-4: ~80%)

| Component | Status | Completeness | Notes |
|-----------|--------|--------------|-------|
| **Types** | ✅ Complete | 100% | TopicId, PeerId, MessageHeader |
| **Transport** | ✅ Complete | 100% | ant-quic integration, stream multiplexing |
| **Membership Traits** | ✅ Complete | 90% | HyParView + SWIM trait definitions |
| **PubSub Traits** | ✅ Complete | 90% | Plumtree trait definitions |
| **CRDTs** | ✅ Complete | 100% | OR-Set, LWW-Register |
| **Groups** | ✅ Complete | 85% | MLS wrapper with BLAKE3 KDF integration |
| **Identity** | ✅ Complete | 100% | ML-DSA key management |
| **Coordinator Adverts** | ✅ Complete | 100% | Bootstrap with binary deployment |
| **Rendezvous Shards** | ✅ Complete | 100% | k=16 sharding implemented |

### Missing Components (Phase 5-6: ~20%)

| Component | Status | Priority | Blocker |
|-----------|--------|----------|---------|
| **Saorsa Sites** | ❌ Not Started | P0 - Critical | Content distribution |
| **Presence Beacons** | ⚠️ Basic | 60% | Basic implementation, needs MLS exporter |
| **FOAF Queries** | ⚠️ Basic | 60% | Framework exists, needs full implementation |
| **IBLT Reconciliation** | ❌ Not Started | P1 - High | Anti-entropy |
| **Bluetooth Fallback** | ❌ Not Started | P2 - Medium | Offline resilience |
| **Peer Scoring** | ❌ Not Started | P1 - High | Mesh quality |

---

## Phase 4: Core Discovery & Bootstrap (Weeks 1-3)

### 4.1 Coordinator Adverts (Week 1)

**Goal**: Implement seedless bootstrap via self-elected coordinators

#### Tasks
1. **Define Coordinator Advert Structure** (SPEC2 §8)
   - [ ] Create `CoordinatorAdvert` type with CBOR serialization
   - [ ] Implement roles: coordinator, reflector, rendezvous, relay
   - [ ] Add NAT class detection (EIM, EDM, symmetric, unknown)
   - [ ] Implement ML-DSA signature over advert
   - [ ] Add expiry and scoring fields

2. **Coordinator Topic**
   - [ ] Create well-known topic for coordinator gossip
   - [ ] Implement advert publishing every 5-10 minutes
   - [ ] Add LRU cache with TTL for received adverts
   - [ ] Implement advert scoring based on uptime/reliability

3. **FOAF `FIND_COORDINATOR`**
   - [ ] Implement bounded FOAF query (TTL=3, fanout=3)
   - [ ] Add reply carrying Coordinator Adverts
   - [ ] Implement query rate limiting

4. **Bootstrap Flow**
   - [ ] Implement cold-start coordinator discovery
   - [ ] Add peer cache persistence (peer_id, addr_hints, last_success)
   - [ ] Implement traversal preference: direct → reflexive → relay

**Testing**
- [ ] Unit tests: advert serialization, signature verification
- [ ] Integration test: 5-node network with 2 coordinators
- [ ] Test: coordinator failover when primary goes offline
- [ ] Test: FOAF query finds coordinators within 3 hops

**Deliverables**
- `crates/coordinator/` - New crate
- Integration with transport layer
- Documentation and examples

---

### 4.2 Rendezvous Shards (Week 2)

**Goal**: Enable global findability without DNS/DHT

#### Tasks
1. **Shard Infrastructure** (SPEC2 §9)
   - [ ] Implement shard ID calculation: `BLAKE3("saorsa-rendezvous" || target_id) & 0xFFFF`
   - [ ] Create 65,536 shard topic space (k=16)
   - [ ] Implement shard subscription management

2. **Provider Summary**
   - [ ] Define `ProviderSummary` CBOR structure
   - [ ] Add capabilities: SITE, IDENTITY
   - [ ] Implement Bloom filter for content summary
   - [ ] Implement IBLT for large set summaries
   - [ ] Add ML-DSA signature over summary

3. **Publisher Flow**
   - [ ] Implement periodic summary gossip to target shard
   - [ ] Add manifest version tracking
   - [ ] Implement summary refresh on content changes

4. **Seeker Flow**
   - [ ] Implement shard subscription for target
   - [ ] Add provider scoring by reliability
   - [ ] Implement direct QUIC fetch from providers

**Testing**
- [ ] Unit tests: shard ID calculation, summary serialization
- [ ] Test: 100 providers distribute across shards evenly
- [ ] Test: seeker finds provider in <5 seconds
- [ ] Benchmark: shard lookup latency P50/P95
- [ ] Load test: 10k providers, 1k seekers

**Deliverables**
- `crates/rendezvous/` - New crate
- Shard manager in pubsub layer
- Performance benchmarks

---

### 4.3 Presence Beacons (Week 3)

**Goal**: Implement MLS-based presence system

#### Tasks
1. **Beacon Generation** (SPEC2 §10)
   - [ ] Integrate with `saorsa-mls 0.3.0` exporter API
   - [ ] Implement `presence_tag = KDF(exporter, user_id || time_slice)`
   - [ ] Add ML-DSA signature over beacon
   - [ ] Encrypt to group with ChaCha20-Poly1305
   - [ ] Set TTL to 10-15 minutes

2. **Beacon Broadcasting**
   - [ ] Gossip beacons on group topics
   - [ ] Implement beacon refresh every 5 minutes
   - [ ] Add beacon cache with expiry

3. **Find User**
   - [ ] Query shared groups first
   - [ ] Fall back to rendezvous shard subscription
   - [ ] Implement abuse controls: capability tokens, rate limits

4. **MLS Integration**
   - [ ] Use `saorsa-mls::MlsGroup::export_secret()` for KDF
   - [ ] Handle epoch changes gracefully
   - [ ] Implement beacon rotation on epoch advance

**Testing**
- [ ] Unit tests: beacon generation, encryption, signature
- [ ] Test: beacon expires after TTL
- [ ] Test: find user in shared group <1 second
- [ ] Test: find user via rendezvous <5 seconds
- [ ] Test: rate limiting blocks spam queries

**Deliverables**
- Update `crates/presence/` with full implementation
- MLS exporter integration
- User discovery examples

---

## Phase 5: Saorsa Sites (Weeks 4-5)

### 5.1 Site Infrastructure (Week 4)

**Goal**: Implement content-addressed websites inside overlay

#### Tasks
1. **Site Identity** (SPEC2 §11)
   - [ ] Define `SiteId = BLAKE3(site_signing_pubkey)[0..32]`
   - [ ] Generate ML-DSA site signing keys
   - [ ] Implement site key management

2. **Manifest Structure**
   - [ ] Create CBOR manifest schema (version, routes, assets)
   - [ ] Add Merkle root of all CIDs
   - [ ] Implement ML-DSA signature over manifest
   - [ ] Support private site metadata (MLS group ID)

3. **Block Store**
   - [ ] Implement content-addressed storage by BLAKE3
   - [ ] Add chunk size configuration (default 1 MiB)
   - [ ] Create block index for fast lookups
   - [ ] Implement block pruning for disk management

4. **`SITE_SYNC` Protocol**
   - [ ] Define `GET_MANIFEST` request/response
   - [ ] Define `GET_BLOCKS` batch request
   - [ ] Implement over QUIC `bulk` stream
   - [ ] Add block transfer progress tracking

**Testing**
- [ ] Unit tests: manifest creation, signature, serialization
- [ ] Test: CID calculation matches content
- [ ] Test: block chunking and reassembly
- [ ] Test: manifest version increments correctly

**Deliverables**
- `crates/sites/` - New crate
- Site builder CLI tool
- Example: publish HTML site

---

### 5.2 Site Publishing & Fetching (Week 5)

#### Tasks
1. **Publisher**
   - [ ] Implement Provider Summary for sites
   - [ ] Gossip to `SITE_ADVERT:<shard(SID)>` topic
   - [ ] Serve manifest and blocks over `SITE_SYNC`
   - [ ] Update summary on manifest changes

2. **Fetcher**
   - [ ] Subscribe to site's advert shard
   - [ ] Pick top providers by score
   - [ ] Fetch and verify manifest
   - [ ] Reconcile blocks via Bloom/IBLT
   - [ ] Batch-request missing blocks
   - [ ] Verify blocks against CIDs

3. **Private Sites**
   - [ ] Encrypt blocks with MLS exporter key
   - [ ] Use ChaCha20-Poly1305 from `saorsa-pqc`
   - [ ] Implement block decryption on fetch
   - [ ] Handle MLS epoch changes

4. **Caching & Replication**
   - [ ] Implement local block cache
   - [ ] Add cache eviction policy (LRU)
   - [ ] Support partial site replication

**Testing**
- [ ] Integration test: publish site, fetch from peer
- [ ] Test: private site blocks are encrypted
- [ ] Test: block reconciliation finds missing CIDs
- [ ] Benchmark: site fetch latency vs size
- [ ] Load test: 100 sites, 1000 blocks each

**Deliverables**
- Site publisher/fetcher implementation
- Browser proxy for viewing sites
- Example: multi-page website with assets

---

## Phase 6: Anti-Entropy & Reliability (Week 6)

### 6.1 IBLT Reconciliation

#### Tasks
1. **IBLT Implementation**
   - [ ] Create IBLT data structure for set reconciliation
   - [ ] Implement encoding/decoding of element IDs
   - [ ] Add IBLT diffing algorithm
   - [ ] Optimize cell count and hash functions

2. **Integration with Plumtree**
   - [ ] Exchange IBLTs in anti-entropy rounds (every 30s)
   - [ ] Identify missing messages via IBLT diff
   - [ ] Request missing payloads via IWANT

3. **Integration with CRDTs**
   - [ ] Use IBLT for OR-Set reconciliation
   - [ ] Implement delta-state synchronization
   - [ ] Add conflict resolution

**Testing**
- [ ] Unit tests: IBLT insert, encode, diff
- [ ] Test: IBLT finds missing elements in large sets
- [ ] Benchmark: reconciliation time vs set size
- [ ] Property test: IBLT correctness with random data

**Deliverables**
- `crates/iblt/` or integrate into `crdt-sync`
- Anti-entropy integration
- Performance benchmarks

---

### 6.2 Peer Scoring

#### Tasks
1. **Scoring Metrics**
   - [ ] Track: message delivery latency
   - [ ] Track: IWANT responsiveness
   - [ ] Track: invalid message rate
   - [ ] Track: duplicate flood rate

2. **Score Calculation**
   - [ ] Implement sliding window averages
   - [ ] Add decay for old behavior
   - [ ] Set thresholds for mesh admission

3. **Mesh Gating**
   - [ ] Promote high-scoring peers to active view
   - [ ] Demote low-scoring peers to passive view
   - [ ] Implement graft/prune based on score

**Testing**
- [ ] Test: malicious peer gets low score
- [ ] Test: good peer maintains high score
- [ ] Simulation: score-based resilience to eclipse

**Deliverables**
- Peer scoring in membership layer
- Mesh quality metrics

---

## Phase 7: Advanced Features (Weeks 7-8)

### 7.1 Bluetooth Fallback (Week 7)

#### Tasks
1. **BLE Gateway**
   - [ ] Implement Bluetooth Mesh managed flooding
   - [ ] Bridge presence beacons to BLE
   - [ ] Bridge `SITE_ADVERT` digests to BLE

2. **Payload Classes**
   - [ ] Class A: presence + IBLT summaries
   - [ ] Class B: short text ≤120 bytes with FEC
   - [ ] Implement strict TTL and hop limits

**Testing**
- [ ] Test: presence propagates over BLE
- [ ] Test: TTL prevents infinite loops
- [ ] Test: FEC recovers from errors

**Deliverables**
- `crates/bluetooth/` - BLE bridge
- Mobile platform support

---

### 7.2 Full System Integration (Week 8)

#### Tasks
1. **End-to-End Workflows**
   - [ ] User joins network → finds coordinator → joins groups
   - [ ] User publishes site → appears in rendezvous → fetched by peers
   - [ ] User sends message → encrypted with MLS → delivered via Plumtree

2. **Configuration Management**
   - [ ] Define default parameters (see SPEC2 §16)
   - [ ] Allow runtime tuning
   - [ ] Add configuration validation

3. **Monitoring & Telemetry**
   - [ ] Add metrics: infection latency, bytes/msg, shard load
   - [ ] Implement health checks
   - [ ] Create dashboard

**Testing**
- [ ] Full system test: 50-node network
- [ ] Chaos engineering: random failures, partitions
- [ ] Performance benchmarks vs targets

**Deliverables**
- Unified CLI tool
- Configuration examples
- Monitoring setup

---

## Testing Strategy

### Unit Tests (Per Component)
- **Coverage Target**: ≥85% per crate
- **Approach**: TDD - write tests first
- **Tools**: `cargo test`, `proptest` for property-based tests

### Integration Tests
1. **Two-Node Tests**: Basic send/receive for each component
2. **Small Network (5-10 nodes)**: Protocol correctness
3. **Medium Network (50 nodes)**: Performance targets
4. **Large Network (100+ nodes)**: Scalability limits

### Property-Based Tests
- **Rendezvous**: shard distribution is uniform
- **IBLT**: reconciliation is complete and correct
- **Plumtree**: all nodes eventually receive all messages
- **CRDTs**: convergence to same state

### Simulation Tests
- **Churn**: nodes join/leave randomly
- **Partitions**: network splits and heals
- **NAT Classes**: mix of EIM, EDM, symmetric NATs
- **Byzantine**: malicious nodes send invalid data

### Performance Benchmarks
- **Infection Latency**: P50 < 500ms, P95 < 2s
- **Shard Lookup**: P95 < 5s
- **Site Fetch**: 1 MiB in < 10s
- **Memory**: < 50 MiB per node
- **Throughput**: > 100 msg/sec/node

### Security Testing
- **Fuzzing**: `cargo-fuzz` on message parsing
- **Cryptographic**: signature verification, encryption correctness
- **Abuse Resistance**: spam, Sybil, eclipse attacks
- **Audit**: External security review before v1.0

---

## Dependencies & Blockers

### Critical Dependencies (Ready)
- ✅ `saorsa-mls 0.3.0` - published to crates.io
- ✅ `saorsa-pqc 0.3.14` - ChaCha20-Poly1305, ML-KEM, ML-DSA
- ✅ `ant-quic 0.10.1` - NAT traversal

### Potential Blockers
1. **IBLT Library**: May need custom implementation
   - **Mitigation**: Research existing Rust crates or port from reference
2. **Bluetooth Mesh**: Platform-specific APIs
   - **Mitigation**: Start with Linux/Android, defer iOS
3. **Performance**: Anti-entropy overhead may be high
   - **Mitigation**: Profile early, optimize sketch sizes

---

## Success Criteria

### Functional Requirements
- [ ] All SPEC2.md invariants implemented
- [ ] All 18 immediate steps from SPEC2 §18 complete
- [ ] Public API from SPEC2 §15 matches implementation

### Quality Requirements
- [ ] Zero compilation errors/warnings
- [ ] ≥85% test coverage per crate
- [ ] All integration tests pass
- [ ] Performance targets met

### Documentation Requirements
- [ ] API documentation complete
- [ ] User guide with examples
- [ ] Protocol specification compliance document
- [ ] Security audit report

---

## Timeline Summary

| Week | Phase | Deliverables |
|------|-------|--------------|
| 1 | Coordinator Adverts | Bootstrap mechanism working |
| 2 | Rendezvous Shards | Global findability implemented |
| 3 | Presence Beacons | User discovery functional |
| 4 | Saorsa Sites Core | Manifest, blocks, CIDs working |
| 5 | Sites Pub/Fetch | Full site distribution end-to-end |
| 6 | IBLT & Scoring | Anti-entropy and mesh quality |
| 7 | Bluetooth | BLE fallback operational |
| 8 | Integration | Full system testing |

**Total Duration**: 8 weeks (2 months)
**Resources**: 1 full-time developer (with AI assistance)

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| IBLT complexity | Medium | High | Early prototype, seek library |
| Performance targets | Medium | High | Profile continuously, optimize |
| BLE platform support | High | Medium | Defer to Phase 2 if needed |
| Rendezvous shard load | Low | Medium | Implement quotas, monitoring |
| Security vulnerabilities | Low | Critical | External audit, fuzzing |

---

## Next Actions

### Immediate (This Week)
1. [ ] Verify `saorsa-mls 0.3.0` compiles with `saorsa-gossip`
2. [ ] Create `crates/coordinator/` skeleton
3. [ ] Define `CoordinatorAdvert` type and wire format
4. [ ] Write first test: advert serialization round-trip

### Short-term (Next 2 Weeks)
1. [ ] Complete Coordinator Adverts implementation
2. [ ] Begin Rendezvous Shards implementation
3. [ ] Update groups crate to use MLS exporter API

### Medium-term (Next Month)
1. [ ] Complete discovery layer (coordinators, rendezvous, presence)
2. [ ] Implement Saorsa Sites core
3. [ ] First end-to-end demo: publish and fetch a site

---

## Appendix: Compliance Checklist

### SPEC2.md Sections
- [x] §1: Invariants - documented, ready to implement
- [x] §2: Architecture - dependencies ready
- [x] §3: Identities - implemented in identity crate
- [x] §4: Transport - ant-quic integrated
- [x] §5: Membership - traits defined
- [x] §6: Dissemination - traits defined
- [ ] §7: Bootstrap - needs Coordinator Adverts
- [ ] §8: Coordinator Advert - not started
- [ ] §9: Rendezvous Shards - not started
- [ ] §10: Presence - partial, needs MLS exporter
- [ ] §11: Saorsa Sites - not started
- [ ] §12: CRDTs - basic types done, IBLT needed
- [ ] §13: Bluetooth - not started
- [x] §14: Wire format - header defined
- [x] §15: Public API - types match spec
- [x] §16: Defaults - documented
- [ ] §17: Threat model - needs testing
- [ ] §18: Immediate steps - 2/9 done

**Overall Progress**: ~80% complete (v0.1.3 production-ready)

---

**Prepared by**: AI Planning Agent
**Approved by**: [Pending human review]
**Last Updated**: 2025-10-05
