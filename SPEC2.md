# Saorsa Gossip Overlay — SPEC (PQC‑only, No DNS, Pure P2P)

Version: 0.2  
Status: Draft for implementation  
Scope: Fully PQC gossip overlay with ant‑quic NAT traversal and relays. No DNS. No HTTP. Friend‑of‑a‑friend + rendezvous shards. Includes Saorsa Sites (website publishing inside the overlay), presence, CRDT sync, and Bluetooth fallback.

---

## 1. Invariants

- **PQC‑only.** Use ML‑KEM for key establishment, ML‑DSA (or SLH‑DSA) for signatures, and ChaCha20‑Poly1305 for AEAD symmetric encryption. No classical or hybrid algorithms.
- **No DNS or Web.** No SRV/TXT, no HTTP/3 gateways. Entirely peer‑to‑peer over QUIC.
- **Overlay‑first.** Discovery and global findability happen via gossip and rendezvous shards.
- **Local‑first data.** Each node stores its own state with CRDT repair on reconnection.

---

## 2. Architecture

**Transport.** `ant-quic` only. Enable: address observation, candidate exchange, hole punching, QUIC path migration, optional relay mode.

**Crypto.** `saorsa-pqc` for ML‑KEM, ML‑DSA, and ChaCha20‑Poly1305 AEAD. `saorsa-mls` for group keys. Default suite: ML‑KEM‑768 + ML‑DSA‑65 + ChaCha20‑Poly1305.
*Note:* SLH‑DSA (SPHINCS+) is also available in `saorsa-pqc` v0.3.14+ with 12 parameter sets (SHA2/SHAKE variants at 128/192/256‑bit security, fast/small trade‑offs) for use cases requiring hash‑based signatures or long‑term security guarantees.

**Membership.** HyParView + SWIM. Active view routes. Passive view heals.

**Dissemination.** Plumtree eager push + lazy digests. Periodic anti‑entropy. Local peer‑scoring gates mesh membership.

**Discovery.** FOAF + **Coordinator Adverts** + **Rendezvous Shards**. No DHT. No DNS.

**Websites.** **Saorsa Sites**: signed, content‑addressed sites fetched over `SITE_SYNC` streams. Optional private sites use MLS.

**Sync.** Delta‑CRDTs. IBLT for large set reconciliation.

**Fallback.** Bluetooth Mesh bridging for presence and short messages.

---

## 3. Identities and IDs

- **Peer identity:** ML‑DSA public key bound to a four‑word alias at the app layer.  
- **PeerId:** `BLAKE3(ml_dsa_pubkey)[0..32]`.  
- **TopicId:** 32‑byte ID per group/topic.  
- **SiteId (SID):** `BLAKE3(site_signing_pubkey)[0..32]`.

All signatures use ML‑DSA (default) or optionally SLH‑DSA for long‑term security. All key agreements use ML‑KEM. All symmetric encryption uses ChaCha20‑Poly1305 from `saorsa-pqc`. No Ed25519/X25519. No AES-GCM.

---

## 4. Transport profile (ant‑quic)

- Streams: `mship` (membership/SWIM), `pubsub` (Plumtree control), `bulk` (payload, CRDT, SITE_SYNC).  
- Address discovery: consume `OBSERVED_ADDRESS` events and treat as reflexive candidates.  
- Punching: ant‑quic coordinates simultaneous sends; migrate to the best path on success.  
- Relay: last‑resort; any public node may opt‑in to relay role; rate‑limited.  
- Path migration: enabled by default.

---

## 5. Membership

**HyParView.** Active degree 8–12. Passive 64–128. Shuffle every 30 s. Promote from passive on failure.

**SWIM.** Probe every 1 s. Indirect probes on timeout. States: alive → suspect → dead. Piggyback membership deltas and lazy message IDs.

---

## 6. Dissemination

**Plumtree.** EAGER on the tree. IHAVE digests to non‑tree links. IWANT pulls missing payloads. Anti‑entropy every 30 s exchanging sketches (Bloom/IBLT).

**Peer scoring.** Up: timely delivery, IWANT responsiveness, low invalid‑msg rate. Down: duplicate floods, invalid signatures, poor responsiveness.

---

## 7. Bootstrap and reachability (no DNS)

1) **Local modes**  
   - Loopback allowed by policy.  
   - LAN gossip beacons (UDP multicast or BLE) for zero‑config joins.  

2) **Social bootstrap**  
   - **Peer cache** is primary: `(peer_id, addr_hints, last_success, nat_class, roles)`.  
   - **Coordinator Adverts**: any public node may self‑elect coordinator/reflector/relay and gossip a signed advert; others cache it with TTL and score.

3) **FOAF help**  
   - If the cache is cold, issue a bounded **FIND_COORDINATOR** over FOAF (TTL=3, fanout=3). Replies carry Coordinator Adverts.

4) **Traversal preference**  
   - Try direct (punched) path → reflexive → relay. Persist the winning path and reuse on next start.

---

## 8. Coordinator Advert

**Purpose.** Seedless bootstrap, address reflection, optional rendezvous and relay.

**Wire (CBOR):**
```
{
  "v":1,
  "peer": PeerId,
  "roles": { "coordinator":true, "reflector":true, "rendezvous":bool, "relay":bool },
  "addr_hints":[ AddrHint ],
  "nat_class":"eim|edm|symmetric|unknown",
  "not_before": u64, // unix ms
  "not_after":  u64, // unix ms
  "score": i32,      // scaled, local-only advisory
  "sig": MLDSA_sig( ... )
}
```
Adverts are gossiped on a well‑known **Coordinator Topic** and cached with LRU+expiry.

---

## 9. Rendezvous Shards (global findability without DNS/DHT)

**Goal.** Let publishers be found by anyone without a directory service.

- **Shard space:** `k=16` ⇒ 65,536 shards.  
- **Shard ID:** `shard = BLAKE3("saorsa-rendezvous" || target_id) & ((1<<k)-1)`.  
- **Provider Summary (CBOR):**
```
{
  "v":1, "target": TargetId, "provider": PeerId,
  "cap": ["SITE","IDENTITY"], // what is served
  "have_root": bool, "manifest_ver": u64?,
  "summary": { "bloom": bytes? , "iblt": bytes? },
  "exp": u64, "sig": MLDSA_sig(...)
}
```
- Publishers periodically gossip summaries to the target’s shard.  
- Seekers subscribe only to relevant shards, pick top providers by score, then fetch directly over QUIC.

---

## 10. Presence and “find user”

- **Presence beacons:** per MLS epoch derive `presence_tag = KDF(exporter, user_id || time_slice)`. Sign with ML‑DSA. Encrypt to group with ChaCha20‑Poly1305. TTL 10–15 min.
- **Find user:** FOAF query inside shared groups first. If not found, subscribe to the user's **rendezvous shard** and wait for Provider Summaries.
- **Abuse gates:** capability tokens for FIND, two‑hop proximity checks, per‑source rate limits, scoring penalties.

---

## 11. Saorsa Sites (websites inside the overlay)

**Identity.** `SID = BLAKE3(site_signing_pubkey)[0..32]` (ML‑DSA).  
**Manifest (CBOR, ML‑DSA‑signed).**
```
{
  "v":1, "sid": SID, "pub": site_signing_pubkey,
  "version": u64, "chunk_size": u32,
  "root": hash256, // merkle of all cids
  "routes":[ { "path": "/index.html", "cid": hash256, "mime":"text/html" }, ... ],
  "assets":[ { "cid": hash256, "len": u64 }, ... ],
  "private": { "mls_group": bytes }? // present for private sites
}
```
**Blocks.** Content‑addressed chunks by BLAKE3. No per‑block signatures; integrity via CID and manifest signature.

**Transport topics.**
- `SITE_ADVERT:<shard(SID)>` → Provider Summary for the site.  
- `SITE_SYNC:<SID>` → request/response over QUIC streams.

**Fetch flow.**
1. Subscribe `SITE_ADVERT` shard. Pick providers by score.  
2. `GET_MANIFEST` → verify ML‑DSA signature and root.  
3. Reconcile block set via Bloom/IBLT summary.  
4. `GET_BLOCKS [cid...]` until complete.

**Private sites.** Encrypt blocks with ChaCha20‑Poly1305 using a key derived from the MLS exporter of the site's group. Manifest still signed by the site key; sensitive fields may be encrypted with ChaCha20‑Poly1305.

**Publisher flow.**
- Serve as provider. Gossip Provider Summary to shard. Stream manifest/blocks on demand. Update version by gossipping new manifest.

---

## 12. Data and CRDTs

- Local‑first state.  
- Favourites store encrypted replicas of contacts and minimal account state.  
- CRDT choices: OR‑Set, LWW‑Register, RGA. Delta‑CRDTs preferred.  
- Anti‑entropy: Bloom/IBLT of element IDs then fetch diffs.

---

## 13. Bluetooth fallback

- Bearer: Bluetooth Mesh managed flood.  
- Bridge: nodes with BLE+IP translate `Coordinator Topic`, `presence`, and small `SITE_ADVERT` digests.  
- Payload classes: presence and IBLT/Bloom summaries; optional 120‑byte short text with FEC.  
- Strict TTL and hop limits.

---

## 14. Wire: control header

All control frames have an ML‑DSA signature over:
```
ver:u8, topic:[u8;32], msg_id:[u8;32], kind:u8, hop:u8, ttl:u8
```
`msg_id = BLAKE3(topic || epoch || signer || payload_hash)[0..32]`.

Kinds: `{EAGER, IHAVE, IWANT, PING, ACK, FIND, PRESENCE, ANTIENTROPY, SHUFFLE, COORD_ADVERT, FIND_COORD, SITE_ADVERT, SITE_SYNC}`.

---

## 15. Public API (Rust)

```rust
pub struct TopicId([u8; 32]);
pub struct PeerId([u8; 32]);
pub struct Sid([u8; 32]);

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

pub trait Rendezvous {
    async fn advertise_provider(&self, target: [u8;32], summary: bytes::Bytes) -> anyhow::Result<()>;
    async fn find_providers(&self, target: [u8;32]) -> anyhow::Result<Vec<PeerId>>;
}

pub trait Presence {
    async fn beacon(&self, topic: TopicId) -> anyhow::Result<()>;
    async fn find_user(&self, user: PeerId) -> anyhow::Result<Vec<String>>;
}

pub trait Sites {
    async fn site_advertise(&self, sid: Sid, summary: bytes::Bytes) -> anyhow::Result<()>;
    async fn site_get_manifest(&self, sid: Sid, peer: PeerId) -> anyhow::Result<Vec<u8>>;
    async fn site_get_blocks(&self, sid: Sid, peer: PeerId, cids: Vec<[u8;32]>) -> anyhow::Result<Vec<Vec<u8>>>;
}
```

---

## 16. Defaults

`active_deg=8`, `passive_deg=64`, `fanout=3`, `anti_entropy=30s`, `SWIM_period=1s`, `suspect_timeout=3s`, `presence_ttl=10m`, `k=16 rendezvous shards`, `chunk_size=1 MiB`.

---

## 17. Threat model

- **Sybil adverts:** score peers; capability tokens for high‑traffic shards; per‑peer rate limits.  
- **Eclipse:** HyParView shuffles + passive diversity.  
- **Replay:** nonces + expiries for presence and adverts; strict signature checks.  
- **Relay abuse:** quotas and admission policies; prefer punched paths.  
- **Partition:** Plumtree and anti‑entropy guarantee convergence after heal.

---

## 18. Immediate steps

1. Expose ant‑quic address discovery/punching/relay hooks to the overlay.  
2. Implement **Coordinator Advert** publishing and cache.  
3. Add FOAF `FIND_COORDINATOR` with TTL/fanout caps.  
4. Implement **Rendezvous Shards** and **Provider Summary**.  
5. Ship **Saorsa Sites**: manifest schema, block store, `SITE_SYNC` streams, verification.  
6. Presence beacons using MLS exporter.  
7. Delta‑CRDT plumbing and IBLT reconciliation.  
8. Bluetooth bridge for presence and SITE_ADVERT digests.  
9. Simulator: churn, NAT classes, shard load; KPIs: infection latency P50/P95, bytes/msg, success of punch vs relay, shard lookup latency.

---

## 19. Compliance

- QUIC transport semantics.  
- MLS group semantics for messaging and private sites.  
- PQC algorithms: ML‑KEM, ML‑DSA (SLH‑DSA optional).

