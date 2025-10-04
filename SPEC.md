# Saorsa Gossip Overlay — SPEC

Version: 0.1  
Status: Draft for implementation  
Scope: Replace DHT discovery with a PQC‑secure gossip overlay for Communitas. Provide transport, membership, dissemination, presence, CRDT sync, and Bluetooth fallback.

---

## 1. Goals

- Remove global DHT. Use the contact graph and existing groups as the overlay.  
- Low overhead broadcast. Partition‑tolerant. Works behind NAT.  
- Local‑first storage with CRDT repair on reconnection.  
- Transport over QUIC with PQC. Group security via MLS.

Non‑goals: public peer directory, global search, servers.

---

## 2. Architecture

**Transport.** `ant-quic` only. QUIC with TLS 1.3, connection migration, NAT rebinding; enable hole‑punching where available. Hybrid or pure‑PQC handshake per policy.

**Crypto.** `saorsa-pqc` for ML‑KEM and ML‑DSA. `saorsa-mls` for group keys. Default suite: ML‑KEM‑768 + ML‑DSA‑65.

**Membership.** HyParView partial views for connectivity plus SWIM for failure detection. Active view routes, passive view heals.

**Dissemination.** Plumtree broadcast: eager‑push on a tree, lazy digests on side links, periodic anti‑entropy. Add peer scoring controls similar to GossipSub v1.1.

**Sync.** Delta‑CRDTs with anti‑entropy. For large sets, reconcile via IBLT.

**Discovery/Presence.** No DHT. Presence beacons scoped to groups, FOAF random‑walk queries over the contact graph with short TTL.

**Fallback.** Bluetooth Mesh bridge for collapse‑mode presence and short messages. Managed flooding only.

---

## 3. Identities, Topics, IDs

- Identity: long‑term ML‑DSA public key, bound to human‑readable alias in app.  
- TopicId: 32‑byte per MLS group.  
- PeerId: `BLAKE3(pubkey)[:32]`. BLAKE3 doubles as XOF/KDF when needed.

---

## 4. Transport Profile

- QUIC streams:  
  `mship` for HyParView+SWIM, `pubsub` for Plumtree control, `bulk` for payloads and CRDT deltas.  
- Enable 0‑RTT resumption where safe, path migration by default.  
- Implemented via `ant-quic`.

---

## 5. Membership

**HyParView.** Maintain two views.  
- Active degree 8–12. Passive degree 64–128.  
- Periodic shuffle every 30 s. Promote from passive on failure.

**SWIM.** Probe each second, indirect probes on timeout, suspect then dead. Piggyback membership deltas and lazy message IDs on probes.

---

## 6. Dissemination

**Plumtree.**  
- EAGER along the spanning tree.  
- IHAVE digests to non‑tree links.  
- On IWANT, send payload.  
- Anti‑entropy every 30 s exchanging message‑ID sketches.

**Peer scoring and mesh gating.** Keep well‑behaved peers in mesh, drop poor performers, opportunistic grafting to improve the median.

---

## 7. Presence and “Find user”

**Beacons.** For each MLS group epoch, derive `presence_tag = KDF(exporter_secret, user_id || time_slice)`. Sign with ML‑DSA. Encrypt to the group. TTL 10–15 minutes.

**Query.** If no shared group, run FOAF random‑walk on the contact graph with fanout 3 and TTL 3–4. Replies encrypted to requester.

**Summaries.** Exchange IBLTs of recent presence tags to test likely membership before asking for full entries.

**Abuse controls.** Capability‑gated FIND, ≤2‑hop proximity checks, per‑source rate limits, scoring penalties.

---

## 8. Data, Storage, CRDTs

- Local‑first. Favourites store encrypted replicas of contact lists and minimal account state.  
- CRDT choices: OR‑Set for membership, LWW‑Register for small scalars, RGA for ordered threads. Delta‑CRDTs for bandwidth efficiency.  
- Large sets: IBLT reconciliation.

---

## 9. Bluetooth Fallback

- Bearer: Bluetooth Mesh managed flooding. Gateway nodes bridge Mesh subnets to QUIC topics.  
- Payload classes:  
  A) presence beacons and IBLT summaries,  
  B) short text ≤120 bytes with FEC, reassemble at IP edge.  
- Strict TTL and hop limits. No large file transfer.  
- Optional directed forwarding if supported by stack.

---

## 10. Wire Format

**Header** for control frames, ML‑DSA signed:

```
ver:u8, topic:[u8;32], msg_id:[u8;32], kind:u8, hop:u8, ttl:u8
// kind ∈ {EAGER, IHAVE, IWANT, PING, ACK, FIND, PRESENCE, ANTIENTROPY, SHUFFLE}
```

`msg_id = BLAKE3(topic || epoch || signer || payload_hash)[:32]`.

IHAVE digest: Bloom or IBLT of recent IDs. Prefer IBLT for reconciliation.

Presence record (MLS‑encrypted): `{presence_tag, addr_hints, since, expires, seq}`.

---

## 11. Public API (Rust)

```rust
pub struct TopicId([u8; 32]);
pub struct PeerId([u8; 32]);

pub trait GossipTransport {
    async fn dial(&self, peer: PeerId) -> anyhow::Result<()>;
    async fn listen(&self, bind: std::net::SocketAddr) -> anyhow::Result<()>;
}

pub trait Membership {
    async fn join(&self, seeds: Vec<String>) -> anyhow::Result<()>;
    fn active_view(&self) -> Vec<PeerId>;
    fn passive_view(&self) -> Vec<PeerId>;
}

pub trait PubSub {
    async fn publish(&self, topic: TopicId, data: bytes::Bytes) -> anyhow::Result<()>;
    fn subscribe(&self, topic: TopicId) -> tokio::sync::mpsc::Receiver<(PeerId, bytes::Bytes)>;
}

pub trait Presence {
    async fn beacon(&self, topic: TopicId) -> anyhow::Result<()>;
    async fn find(&self, user: PeerId) -> anyhow::Result<Vec<String>>; // Addr hints
}
```

---

## 12. Defaults

`active_deg=8`, `passive_deg=64`, `fanout=3`, `IHAVE_batch≤1024`, `anti_entropy=30s`, `SWIM_period=1s`, `suspect_timeout=3s`, `presence_ttl=10m`.

---

## 13. Threat Model

- Spam/Sybil: invited joins only, FOAF and capability checks, scoring, token buckets.  
- Eclipse: HyParView shuffles; passive diversity; multipath dial.  
- Replay: per‑topic nonces; signature checks; expiry on presence.  
- Partition: Plumtree lazy links and anti‑entropy ensure convergence on heal.

---

## 14. Immediate Steps

1. Crate skeleton: `transport`, `membership`, `pubsub`, `presence`, `crdt_sync`, `groups`, `identity`, `types.rs`.  
2. Transport adapter for `ant-quic`. Define three control streams and path‑migration hooks.  
3. Membership: HyParView + SWIM with piggybacked deltas.  
4. PubSub: Plumtree EAGER/IHAVE/IWANT and anti‑entropy using IBLT.  
5. Presence: MLS exporter‑derived tags and FOAF `find` with capability tokens.  
6. CRDT sync: delta‑CRDT plumbing and IBLT reconciliation.  
7. Bluetooth bridge: Mesh flooding gateway for presence and short messages.  
8. Peer scoring controls and mesh gating.  
9. Simulator: churn, partitions, NAT. KPIs: infection latency P50/P95, bytes per delivered msg, reconvergence time, false suspicion.

---

## 15. Compliance

- QUIC 9000/9001.  
- MLS 9420.  
- PQC FIPS 203/204/205.

---

## 16. Repos

`ant-quic`, `saorsa-pqc`, `saorsa-mls`, `communitas`.

---

## 17. Contrast with DHT

Hyperswarm and HyperDHT use a Kademlia‑style DHT for discovery. Saorsa‑gossip omits this global surface.

```
References: QUIC RFC 9000/9001; MLS RFC 9420; FIPS 203/204/205; SWIM; HyParView; Plumtree; GossipSub v1.1; CRDT/delta‑CRDT papers; IBLT; Bluetooth Mesh specs.
```
