# ADR-007: FOAF Privacy-Preserving Discovery

## Status

Accepted (2025-12-24)

## Context

Finding specific peers in a decentralized network creates privacy concerns:

| Discovery Method | Privacy Problem |
|------------------|-----------------|
| Global directory | Reveals entire social graph to operators |
| DHT lookup | Query observable by routing nodes |
| Flood query | Reveals who you're searching for to everyone |
| Mutual contacts | Works but limited to direct friends |

We needed discovery that:
- Finds peers without global directory
- Limits exposure to social graph distance
- Provides plausible deniability
- Works with partial network connectivity
- Bounds query propagation

## Decision

Implement **FOAF (Friend-of-a-Friend) Discovery**: bounded social graph walks with capability tokens:

### Query Structure

```rust
#[derive(Serialize, Deserialize)]
pub struct FoafQuery {
    /// What we're looking for
    pub target: FoafTarget,
    /// Requester's peer ID
    pub requester: PeerId,
    /// Remaining hops (decremented at each relay)
    pub ttl: u8,
    /// Fanout factor (how many peers to query at each hop)
    pub fanout: u8,
    /// Optional capability token (proves relationship)
    pub capability: Option<CapabilityToken>,
    /// Signature by requester
    pub signature: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub enum FoafTarget {
    /// Find a specific peer
    User { peer_id: PeerId },
    /// Find coordinators (for bootstrap)
    Coordinator,
    /// Find content providers
    Content { target_hash: [u8; 32] },
}
```

### Bounded Propagation

Queries propagate only TTL hops with limited fanout:

```
Query: FIND_USER(Bob)
TTL: 3
Fanout: 3

Hop 0: Alice queries 3 of her contacts
       → Carol, Dave, Eve

Hop 1: Carol, Dave, Eve each query 3 of their contacts (TTL=2)
       → 9 peers total

Hop 2: Each of 9 peers queries 3 (TTL=1)
       → 27 peers total

Hop 3: Each of 27 peers checks local only (TTL=0)
       → No further propagation

Total peers reached: 3 + 9 + 27 = 39 peers max
```

### Query Handling

```rust
impl FoafHandler {
    pub async fn handle_query(&mut self, query: FoafQuery) -> Option<FoafResponse> {
        // Verify signature
        if !query.verify() {
            return None;
        }

        // Check TTL
        if query.ttl == 0 {
            // End of propagation, check local only
            return self.check_local(&query);
        }

        // Check if we know the target
        if let Some(response) = self.check_local(&query) {
            return Some(response);
        }

        // Forward to random subset of contacts
        let next_query = query.with_decremented_ttl();
        let selected_peers = self.select_random_contacts(query.fanout);

        for peer in selected_peers {
            if let Some(response) = self.forward_query(peer, &next_query).await? {
                return Some(response);
            }
        }

        None
    }

    fn check_local(&self, query: &FoafQuery) -> Option<FoafResponse> {
        match &query.target {
            FoafTarget::User { peer_id } => {
                if *peer_id == self.my_peer_id {
                    // We are the target
                    Some(FoafResponse::Found {
                        peer_id: self.my_peer_id,
                        addr_hints: self.my_addresses.clone(),
                    })
                } else if self.contacts.contains(peer_id) {
                    // We know the target
                    let info = self.contacts.get(peer_id)?;
                    Some(FoafResponse::Found {
                        peer_id: *peer_id,
                        addr_hints: info.addr_hints.clone(),
                    })
                } else {
                    None
                }
            }
            // ... other target types
        }
    }

    fn select_random_contacts(&self, count: u8) -> Vec<PeerId> {
        self.contacts
            .keys()
            .choose_multiple(&mut rand::thread_rng(), count as usize)
            .cloned()
            .collect()
    }
}
```

### Capability Tokens

For sensitive queries, capability tokens prove relationship:

```rust
#[derive(Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Who is allowed to query
    pub requester: PeerId,
    /// What they can query for
    pub target: PeerId,
    /// Capability type
    pub capability: Capability,
    /// When issued
    pub issued_at: u64,
    /// When expires
    pub expires_at: u64,
    /// Signature by target peer
    pub signature: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub enum Capability {
    /// Can search for this user
    FindUser,
    /// Can view presence
    ViewPresence,
    /// Can send messages
    SendMessage,
}

impl CapabilityToken {
    /// Issue token to allow someone to find you
    pub fn issue_find_user(
        issuer_keypair: &MlDsaKeyPair,
        requester: PeerId,
        ttl: Duration,
    ) -> Self {
        let now = unix_time_millis();
        let mut token = Self {
            requester,
            target: PeerId::from_pubkey(issuer_keypair.public_key()),
            capability: Capability::FindUser,
            issued_at: now,
            expires_at: now + ttl.as_millis() as u64,
            signature: vec![],
        };
        token.signature = issuer_keypair.sign(&token.serialize_unsigned());
        token
    }
}
```

### Privacy Properties

#### 1. Bounded Exposure

With TTL=3, fanout=3:
- Maximum 39 peers see query
- Not the entire network
- Probability of finding target depends on social graph connectivity

#### 2. Query Unobservability

Non-participants don't learn:
- Who Alice is searching for
- That Alice is searching at all
- Social graph connections

#### 3. Plausible Deniability

Intermediate relayers can claim:
- "I just forwarded a query"
- "I don't know the requester"
- No proof of relationship

### Response Flow

```rust
#[derive(Serialize, Deserialize)]
pub struct FoafResponse {
    /// Original query ID
    pub query_id: MessageId,
    /// Response type
    pub result: FoafResult,
    /// Proof of knowledge (optional)
    pub proof: Option<Vec<u8>>,
    /// Signature by responder
    pub signature: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub enum FoafResult {
    /// Target found
    Found {
        peer_id: PeerId,
        addr_hints: Vec<SocketAddr>,
    },
    /// Target not found in scope
    NotFound,
    /// Query rejected (rate limited, invalid capability)
    Rejected { reason: String },
}
```

### Rate Limiting

Prevent query flooding:

```rust
impl FoafRateLimiter {
    const MAX_QUERIES_PER_HOUR: u32 = 10;
    const MAX_FORWARDS_PER_MINUTE: u32 = 20;

    fn check_query_rate(&self, requester: &PeerId) -> bool {
        let count = self.query_counts.get(requester).unwrap_or(&0);
        *count < Self::MAX_QUERIES_PER_HOUR
    }

    fn check_forward_rate(&self) -> bool {
        self.forwards_this_minute < Self::MAX_FORWARDS_PER_MINUTE
    }

    fn handle_query(&mut self, query: &FoafQuery) -> Result<()> {
        if !self.check_query_rate(&query.requester) {
            return Err(Error::RateLimited);
        }

        if query.ttl > 0 && !self.check_forward_rate() {
            // Don't forward, but still check locally
            return self.check_local_only(query);
        }

        self.record_query(&query.requester);
        self.process_query(query)
    }
}
```

## Consequences

### Benefits

1. **Privacy preserving**: Limited query exposure
2. **No global directory**: Discovery through social graph
3. **Censorship resistant**: No central point to block
4. **Capability-based**: Fine-grained access control
5. **Bounded resources**: TTL and rate limiting prevent abuse

### Trade-offs

1. **Limited reach**: May not find target if socially distant
2. **Latency**: Multi-hop queries take time
3. **No guarantee**: Success depends on graph connectivity
4. **Token management**: Capabilities need distribution

### Success Probability

With TTL=3, fanout=3, finding target depends on:
- Size of requester's contact list
- Overlap between requester's and target's social graphs
- How many hops away target is

Estimated success rates (simulated):
- 1 hop away: >99%
- 2 hops away: ~80%
- 3 hops away: ~50%
- 4+ hops away: <20%

For distant peers, fall back to rendezvous shards (ADR-005).

## Alternatives Considered

### 1. Global Directory

Central service indexing all peers.

**Rejected because**:
- Single point of failure
- Privacy violation (sees all queries)
- Censorship vulnerability

### 2. DHT-Based Lookup

Store peer info in distributed hash table.

**Rejected because**:
- Sybil attacks on key ranges
- Query observable by DHT nodes
- Routing attacks

### 3. Flooding

Broadcast query to entire network.

**Rejected because**:
- O(N) message overhead
- Everyone sees every query
- Bandwidth explosion

### 4. Onion Routing

Route queries through Tor-like anonymity network.

**Rejected because**:
- High latency
- Complex circuit management
- Overkill for our threat model

### 5. Zero-Knowledge Proofs

Prove relationship without revealing identity.

**Considered for future**:
- Would strengthen privacy
- Currently adds complexity
- May add ZK capability tokens later

## References

- **FOAF concept**: Friend-of-a-Friend network analysis
- **Implementation**: `crates/presence/src/foaf.rs`
- **Capability tokens**: `crates/presence/src/capability.rs`
- **Related**: ADR-005 (Rendezvous Shards for distant discovery)
