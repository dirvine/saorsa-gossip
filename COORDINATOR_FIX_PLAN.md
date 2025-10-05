# Coordinator Fix Plan - TDD Approach

## Overview
Fix 4 critical issues in coordinator crate before proceeding to Rendezvous Shards implementation.

## Issues to Fix (Priority Order)

### 🔴 CRITICAL 1: CBOR Serialization (MUST FIX)
**Current**: Using bincode placeholder
**Required**: RFC 8949 CBOR via ciborium crate
**SPEC2 Reference**: §8 Wire Format

**TDD Approach**:
1. **Write failing tests**:
   - Test CBOR round-trip serialization
   - Test wire format compatibility (known CBOR bytes)
   - Test that bincode bytes != CBOR bytes (prove difference)
   - Test error handling for malformed CBOR

2. **Implementation**:
   - Add `ciborium = "0.2"` to Cargo.toml
   - Replace `bincode::serialize` with `ciborium::into_writer`
   - Replace `bincode::deserialize` with `ciborium::from_reader`
   - Update SignableFields serialization to use CBOR

3. **Verification**:
   - All new tests pass
   - All existing tests still pass
   - Zero compilation warnings

**Files to modify**:
- `coordinator/Cargo.toml` (add ciborium)
- `coordinator/src/lib.rs` (CoordinatorAdvert::to_cbor, from_cbor, sign, verify)

---

### 🔴 CRITICAL 2: FOAF Query Execution (MUST FIX)
**Current**: Returns None for cold cache
**Required**: Actually trigger FOAF query propagation
**SPEC2 Reference**: §7.3, §7.4

**TDD Approach**:
1. **Write failing tests**:
   - Test Bootstrap returns query when cache is cold
   - Test query has correct TTL (3)
   - Test query has correct fanout (3)
   - Test query propagates through handler
   - Test query includes correct origin peer_id

2. **Design decisions**:
   - Bootstrap::find_coordinator() should return `Result<BootstrapResult, BootstrapAction>`
   - `BootstrapAction::SendQuery(FindCoordinatorQuery)` when cache is cold
   - Application layer responsible for sending query via transport
   - Store pending queries and match with responses

3. **Implementation**:
   - Create `BootstrapAction` enum
   - Add `pending_queries: Arc<Mutex<HashMap<QueryId, Instant>>>` to Bootstrap
   - Change `find_coordinator()` to return action instead of Option
   - Add `handle_find_response()` method to process responses
   - Add query timeout logic (30s per SPEC2 §7.3)

4. **Verification**:
   - Test cold cache triggers query action
   - Test warm cache returns coordinator directly
   - Test response handling updates cache
   - Test query timeout cleanup

**Files to modify**:
- `coordinator/src/bootstrap.rs` (add BootstrapAction, change API)
- `coordinator/src/foaf.rs` (ensure query/response work)

---

### 🟡 MODERATE 3: Traversal Method Logic (SHOULD FIX)
**Current**: All methods return same addr_hints.first()
**Required**: Different logic per traversal type
**SPEC2 Reference**: §7.4

**TDD Approach**:
1. **Write failing tests**:
   - Test Direct uses public_addrs field
   - Test Reflexive uses reflexive_addrs field
   - Test Relay uses relay peer addresses
   - Test preference order (try Direct first, then Reflexive, then Relay)
   - Test fallback when preferred method unavailable

2. **Design decisions**:
   - Split PeerCacheEntry addr_hints into:
     - `public_addrs: Vec<SocketAddr>` (for Direct)
     - `reflexive_addrs: Vec<SocketAddr>` (for Reflexive)
     - `relay_peer: Option<PeerId>` (for Relay)
   - Direct: Use public_addrs
   - Reflexive: Use reflexive_addrs (from STUN/hole punching)
   - Relay: Lookup relay peer from cache, return its public address

3. **Implementation**:
   - Update PeerCacheEntry struct with new fields
   - Update AddrHint to include hint_type (public/reflexive/relay)
   - Update get_addr_for_method logic per traversal type
   - Update all tests to use new fields

4. **Verification**:
   - Test each traversal method uses correct address type
   - Test preference order selection
   - Test relay peer lookup works

**Files to modify**:
- `coordinator/src/peer_cache.rs` (PeerCacheEntry fields)
- `coordinator/src/lib.rs` (AddrHint type)
- `coordinator/src/bootstrap.rs` (get_addr_for_method logic)

---

### 🟡 MODERATE 4: Peer Cache Persistence (SHOULD FIX)
**Current**: In-memory only
**Required**: Serialize/deserialize to disk
**SPEC2 Reference**: §7.2 (implied by "peer cache")

**TDD Approach**:
1. **Write failing tests**:
   - Test save() writes valid CBOR to disk
   - Test load() reads CBOR from disk
   - Test round-trip (save → load → same entries)
   - Test load handles missing file gracefully
   - Test load handles corrupted file gracefully
   - Test concurrent access is safe

2. **Design decisions**:
   - Use CBOR format for consistency
   - Store at `~/.saorsa/peer_cache.cbor` by default
   - Add `save(&self, path: &Path) -> Result<()>`
   - Add `load(path: &Path) -> Result<PeerCache>`
   - Auto-save on insert/prune (debounced)
   - Use file locking to prevent corruption

3. **Implementation**:
   - Derive Serialize/Deserialize for PeerCacheEntry (already done)
   - Add persistence module with save/load functions
   - Add optional auto_save flag to PeerCache
   - Add file locking (use fs2 crate)
   - Add debounced save task (use tokio::time)

4. **Verification**:
   - Test file I/O works correctly
   - Test corruption handling
   - Test concurrent access safety
   - Test auto-save functionality

**Files to modify**:
- `coordinator/Cargo.toml` (add fs2 for file locking)
- `coordinator/src/peer_cache.rs` (add save/load methods)

---

## Implementation Order

### Phase 1: CBOR Serialization (Day 1)
1. Add ciborium dependency
2. Write failing CBOR tests
3. Implement CBOR serialization
4. Verify all tests pass
5. **Gate**: Zero warnings, all tests pass

### Phase 2: FOAF Query Execution (Day 1-2)
1. Design BootstrapAction enum
2. Write failing query execution tests
3. Implement query triggering logic
4. Implement response handling
5. **Gate**: Zero warnings, all tests pass

### Phase 3: Traversal Method Logic (Day 2)
1. Design new PeerCacheEntry fields
2. Write failing traversal tests
3. Implement traversal logic
4. Update all existing tests
5. **Gate**: Zero warnings, all tests pass

### Phase 4: Peer Cache Persistence (Day 3)
1. Add fs2 dependency
2. Write failing persistence tests
3. Implement save/load methods
4. Implement auto-save
5. **Gate**: Zero warnings, all tests pass

### Phase 5: Integration Verification (Day 3)
1. Run full test suite (cargo test --all)
2. Run clippy (cargo clippy -- -D warnings)
3. Check documentation (cargo doc)
4. Manual integration test
5. **Gate**: ALL quality gates pass

---

## Quality Gates (MUST PASS)

After each phase:
- ✅ All tests pass (100% pass rate)
- ✅ Zero compilation errors
- ✅ Zero compilation warnings
- ✅ Zero clippy warnings
- ✅ All public APIs documented
- ✅ No forbidden patterns (.unwrap, .expect, panic!)

Final gate before Rendezvous Shards:
- ✅ All 4 issues completely resolved
- ✅ Full test coverage (>85%)
- ✅ SPEC2.md §7-8 fully implemented
- ✅ No placeholders or TODOs
- ✅ Production-ready code

---

## Dependencies to Add

```toml
[dependencies]
ciborium = "0.2"      # CBOR serialization (RFC 8949)
fs2 = "0.4"           # File locking for safe persistence
```

---

## Test Count Estimate

- Phase 1 (CBOR): +6 tests
- Phase 2 (FOAF): +8 tests
- Phase 3 (Traversal): +6 tests
- Phase 4 (Persistence): +8 tests

**Total**: ~28 new tests (78 tests total in coordinator crate)

---

## Success Criteria

Before moving to Rendezvous Shards:
1. ✅ CBOR serialization fully working
2. ✅ FOAF queries actually propagate
3. ✅ Traversal methods use correct addresses
4. ✅ Peer cache persists across restarts
5. ✅ Zero placeholders or shortcuts
6. ✅ All SPEC2.md §7-8 requirements met
7. ✅ Production-ready code quality

---

## Timeline

- **Day 1**: Phase 1 (CBOR) + start Phase 2 (FOAF)
- **Day 2**: Complete Phase 2 (FOAF) + Phase 3 (Traversal)
- **Day 3**: Phase 4 (Persistence) + Phase 5 (Integration)

**Total**: 3 days to production-ready coordinator crate

---

## Notes

- Use TDD throughout - write tests BEFORE implementation
- No shortcuts - implement fully per SPEC2.md
- Zero tolerance for warnings or errors
- Document all decisions in code comments
- Update SPEC2_IMPLEMENTATION_PLAN.md when complete
