# saorsa-mls 0.3.0 Capabilities for SPEC2.md

**Date**: 2025-10-05
**Status**: Ready for Integration
**Version**: saorsa-mls 0.3.0 (local path dependency)

---

## Executive Summary

`saorsa-mls 0.3.0` provides all the necessary MLS (Message Layer Security) capabilities required by SPEC2.md, including:

✅ **PQC-Only Mode**: ML-KEM-768 + ML-DSA-65 + ChaCha20-Poly1305
✅ **MLS Exporter API**: For presence beacon key derivation
✅ **Group Management**: TreeKEM for scalable key distribution
✅ **Credential Validation**: Proper authentication and policy enforcement

---

## Key Features Relevant to SPEC2

### 1. Cipher Suite (PQC-Only)

**Configured**: `SPEC2_MLS_128_MLKEM768_CHACHA20POLY1305_SHA256_MLDSA65` (0x0B01)

- **KEM**: ML-KEM-768 (FIPS 203)
- **AEAD**: ChaCha20-Poly1305 (from saorsa-pqc)
- **Hash**: SHA256
- **Signature**: ML-DSA-65 (FIPS 204)

**No hybrid algorithms** - fully compliant with SPEC2 §1 invariants.

---

### 2. MLS Exporter API (RFC 9420 §8.5)

**Required for**: Presence beacons (SPEC2 §10)

#### API Usage

```rust
use saorsa_mls::{MlsGroup, GroupConfig};

// Create MLS group
let config = GroupConfig::default();
let mut group = MlsGroup::new(config, creator_identity).await?;

// Export secret for presence tag derivation
let exporter_secret = group.export_secret(
    "presence-tag",           // label
    &[],                       // context (can include user_id || time_slice)
    32                         // output length in bytes
)?;

// Derive presence tag per SPEC2 §10
let user_id = b"user123";
let time_slice = current_time_slice();
let context = [user_id, &time_slice.to_be_bytes()].concat();

let presence_tag = group.export_secret("presence", &context, 32)?;
```

**Implementation**: `src/group.rs` - `MlsGroup::export_secret()`

---

### 3. Group Operations

#### Create Group
```rust
use saorsa_mls::{MlsGroup, GroupConfig, MemberIdentity, MemberId};

let config = GroupConfig::default();
let creator = MemberIdentity::generate(MemberId::generate())?;
let mut group = MlsGroup::new(config, creator).await?;
```

#### Add Member
```rust
let new_member = MemberIdentity::generate(MemberId::generate())?;
let welcome = group.add_member(&new_member).await?;

// Send welcome message to new member via QUIC
// New member processes welcome to join group
```

#### Remove Member
```rust
let member_id = MemberId::from_bytes(&[...])?;
group.remove_member(&member_id).await?;
```

#### Encrypt/Decrypt Messages
```rust
// Encrypt message to group
let plaintext = b"Hello, group!";
let ciphertext = group.encrypt_message(plaintext)?;

// Decrypt received message
let decrypted = group.decrypt_message(&ciphertext)?;
assert_eq!(plaintext, &decrypted[..]);
```

---

### 4. Credential Validation

**Security Fix** (v0.3.0): Critical credential verification bypass fixed

```rust
use saorsa_mls::{Credential, CredentialType, TrustStore};

// Validate member credentials
let trust_store = TrustStore::new();
let credential = Credential::new(CredentialType::Basic, member_pubkey)?;

if !trust_store.validate(&credential)? {
    return Err(MlsError::Unauthorized("Invalid credential".into()));
}
```

---

### 5. Key Rotation

**Automatic**: Groups advance epochs on member changes
**Manual**: Can force rotation for forward secrecy

```rust
// Advance epoch manually
group.rotate_keys().await?;

// Get current epoch
let epoch = group.current_epoch();
```

**Relevance**: SPEC2 §10 presence beacons rotate per MLS epoch

---

### 6. Private Site Encryption (SPEC2 §11)

#### Derive Site Encryption Key

```rust
// Use MLS exporter to derive ChaCha20-Poly1305 key for site blocks
let site_id = b"site-abc123";
let exporter_label = "saorsa-site-encryption";
let context = site_id;

let encryption_key = group.export_secret(exporter_label, context, 32)?;

// Create ChaCha20-Poly1305 cipher
use saorsa_pqc::{SymmetricKey, ChaCha20Poly1305Cipher};

let key = SymmetricKey::from_bytes(encryption_key)?;
let cipher = ChaCha20Poly1305Cipher::new(&key);

// Encrypt site block
let block_data = b"<html>...</html>";
let (ciphertext, nonce) = cipher.encrypt(block_data, None)?;

// Decrypt site block (recipient)
let decrypted = cipher.decrypt(&ciphertext, &nonce, None)?;
```

---

## Integration Points with saorsa-gossip

### 1. Groups Crate

**File**: `crates/groups/src/lib.rs`

**Current Status**: Basic MLS wrapper (40% complete)

**Required Updates**:
```rust
use saorsa_mls::{MlsGroup, GroupConfig, MemberIdentity};

pub struct GroupContext {
    pub topic_id: TopicId,
    pub mls_group: MlsGroup, // Add MLS group instance
    pub cipher_suite: CipherSuite,
    pub epoch: u64,
}

impl GroupContext {
    // Update to use saorsa-mls
    pub async fn new_mls_group(entity_id: &str) -> Result<Self> {
        let topic_id = TopicId::from_entity(entity_id)?;

        let config = GroupConfig::default();
        let identity = MemberIdentity::generate(MemberId::generate())?;
        let mls_group = MlsGroup::new(config, identity).await?;

        Ok(Self {
            topic_id,
            mls_group,
            cipher_suite: CipherSuite::MlKem768MlDsa65,
            epoch: 0,
        })
    }

    // Update presence secret derivation
    pub fn derive_presence_secret(&self, user_id: &[u8], time_slice: u64) -> Result<[u8; 32]> {
        let context = [user_id, &time_slice.to_be_bytes()].concat();
        let secret = self.mls_group.export_secret("presence", &context, 32)?;

        let mut output = [0u8; 32];
        output.copy_from_slice(&secret);
        Ok(output)
    }
}
```

---

### 2. Presence Crate

**File**: `crates/presence/src/lib.rs`

**Required Integration**:
```rust
use saorsa_gossip_groups::GroupContext;
use saorsa_pqc::{ChaCha20Poly1305Cipher, SymmetricKey};

pub async fn beacon(&self, topic: TopicId, group: &GroupContext) -> Result<()> {
    // Get user ID
    let user_id = self.identity.peer_id().to_bytes();

    // Get time slice (e.g., current_time / 300_000 for 5-min slices)
    let time_slice = current_time_ms() / 300_000;

    // Derive presence tag from MLS exporter
    let presence_tag = group.derive_presence_secret(&user_id, time_slice)?;

    // Create beacon
    let beacon = PresenceBeacon {
        tag: presence_tag,
        addr_hints: self.get_addr_hints(),
        since: current_time_ms(),
        expires: current_time_ms() + 600_000, // 10 min
        seq: self.next_seq(),
    };

    // Sign with ML-DSA
    let signature = self.identity.sign(&beacon.to_bytes())?;

    // Encrypt to group with ChaCha20-Poly1305
    let group_key = group.derive_encryption_key()?;
    let cipher = ChaCha20Poly1305Cipher::new(&SymmetricKey::from_bytes(&group_key)?);
    let (ciphertext, nonce) = cipher.encrypt(&beacon.to_bytes(), None)?;

    // Gossip encrypted beacon
    self.pubsub.publish(topic, ciphertext.into()).await?;

    Ok(())
}
```

---

### 3. Sites Crate (New)

**File**: `crates/sites/src/private.rs`

**Private Site Encryption**:
```rust
use saorsa_gossip_groups::GroupContext;
use saorsa_pqc::ChaCha20Poly1305Cipher;

pub struct PrivateSite {
    pub site_id: SiteId,
    pub mls_group: GroupContext,
    pub manifest: SiteManifest,
}

impl PrivateSite {
    /// Encrypt a block for private site storage
    pub fn encrypt_block(&self, block_data: &[u8]) -> Result<EncryptedBlock> {
        // Derive encryption key from MLS exporter
        let key_material = self.mls_group.mls_group.export_secret(
            "saorsa-site-block",
            self.site_id.as_bytes(),
            32
        )?;

        let key = SymmetricKey::from_bytes(&key_material)?;
        let cipher = ChaCha20Poly1305Cipher::new(&key);

        let (ciphertext, nonce) = cipher.encrypt(block_data, None)?;

        Ok(EncryptedBlock {
            cid: blake3::hash(block_data).into(),
            ciphertext,
            nonce,
        })
    }

    /// Decrypt a block from private site
    pub fn decrypt_block(&self, block: &EncryptedBlock) -> Result<Vec<u8>> {
        let key_material = self.mls_group.mls_group.export_secret(
            "saorsa-site-block",
            self.site_id.as_bytes(),
            32
        )?;

        let key = SymmetricKey::from_bytes(&key_material)?;
        let cipher = ChaCha20Poly1305Cipher::new(&key);

        let plaintext = cipher.decrypt(&block.ciphertext, &block.nonce, None)?;

        // Verify CID matches
        let computed_cid = blake3::hash(&plaintext);
        if computed_cid.as_bytes() != block.cid.as_bytes() {
            return Err(SiteError::CidMismatch);
        }

        Ok(plaintext)
    }
}
```

---

## Deprecation Warnings

The following warnings appear during compilation but are **safe to ignore**:

```
warning: use of deprecated unit variant `crypto::MlsKem::HybridX25519MlKem768`
warning: use of deprecated unit variant `crypto::CipherSuiteId::MLS_128_MLKEM768_CHACHA20POLY1305_SHA256_MLDSA65`
```

**Reason**: SPEC2 mandates PQC-only mode. Hybrid variants are deprecated but still present for backward compatibility. The preferred cipher suite is:

`CipherSuiteId::SPEC2_MLS_128_MLKEM768_CHACHA20POLY1305_SHA256_MLDSA65` (0x0B01)

---

## Testing Requirements

### Unit Tests
- [ ] MLS group creation
- [ ] Member add/remove
- [ ] Message encryption/decryption
- [ ] Exporter secret derivation
- [ ] Epoch advancement
- [ ] Credential validation

### Integration Tests
- [ ] Multi-node group creation
- [ ] Presence beacon generation and verification
- [ ] Private site block encryption/decryption
- [ ] Key rotation and epoch sync

### Performance Tests
- [ ] Group creation latency
- [ ] Message encryption throughput
- [ ] Exporter secret derivation time
- [ ] TreeKEM update cost with N members

---

## Known Issues

### Issue 1: saorsa-mls 0.3.0 Not Published

**Status**: Local path dependency used
**Location**: `Cargo.toml` workspace

```toml
saorsa-mls = { version = "0.3.0", path = "../saorsa-mls" }
```

**Action Required**: Publish saorsa-mls 0.3.0 to crates.io before final release

**Publishing Checklist**:
```bash
cd ../saorsa-mls
git add -A
git commit -m "chore: prepare 0.3.0 release"
cargo publish --dry-run
cargo publish
```

---

## API Compatibility

### saorsa-mls 0.3.0 vs 0.2.0

**Breaking Changes**:
- MLS exporter API added (new feature)
- Credential validation fixed (security fix)
- Cipher suite IDs updated for SPEC2 compliance

**Migration Guide**:
```rust
// Old (0.2.0)
let group = MlsGroup::default();

// New (0.3.0)
let config = GroupConfig::default();
let identity = MemberIdentity::generate(MemberId::generate())?;
let group = MlsGroup::new(config, identity).await?;
```

---

## Dependencies

### saorsa-mls 0.3.0 Dependencies

```toml
saorsa-pqc = "0.3.14"        # ML-KEM, ML-DSA, ChaCha20-Poly1305
ant-quic = "0.8.13"          # QUIC transport (note: different version than gossip)
chacha20poly1305 = "0.10"    # AEAD cipher
```

**Note**: `saorsa-gossip` uses `ant-quic 0.10.1`, while `saorsa-mls` uses `0.8.13`. This is **not a problem** as they are compatible versions and `ant-quic` is only used internally by each crate.

---

## Next Steps

1. **Immediate**:
   - [ ] Update `crates/groups/src/lib.rs` to use MlsGroup
   - [ ] Test MLS exporter API with presence beacon derivation

2. **Week 1**:
   - [ ] Integrate MLS group management into groups crate
   - [ ] Update presence crate to use MLS exporter

3. **Week 4-5**:
   - [ ] Implement private site encryption with MLS exporter
   - [ ] Test epoch rotation and key updates

---

## Resources

- **saorsa-mls Source**: `/Users/davidirvine/Desktop/Devel/projects/saorsa-mls/`
- **saorsa-pqc Docs**: https://docs.rs/saorsa-pqc/0.3.14
- **RFC 9420 (MLS)**: https://datatracker.ietf.org/doc/rfc9420/
- **SPEC2.md**: Full protocol specification

---

**Status**: ✅ All SPEC2.md MLS requirements can be implemented with saorsa-mls 0.3.0
**Ready to Start**: Yes - begin with coordinator adverts, then integrate MLS for presence/sites

**Last Updated**: 2025-10-05
