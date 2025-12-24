# ADR-002: Post-Quantum Cryptography First

## Status

Accepted (2025-12-24)

## Context

Cryptographic choices in a P2P protocol are foundational decisions that affect:

1. **Security lifetime**: Networks may operate for decades
2. **Interoperability**: All peers must agree on algorithms
3. **Performance**: Signature/encryption overhead affects latency
4. **Migration cost**: Changing cryptography later is extremely difficult

The quantum computing threat creates urgency:

| Timeline | Threat Level |
|----------|--------------|
| Today | "Harvest now, decrypt later" attacks collecting encrypted traffic |
| 5-10 years | Cryptographically Relevant Quantum Computers (CRQCs) may break classical crypto |
| 10+ years | Stored classical-encrypted data becomes fully readable |

Most P2P systems today use classical cryptography (Ed25519, X25519, AES) with vague plans for "future PQC migration." This approach has problems:

- **Migration is hard**: Protocol changes require coordinated network upgrades
- **Hybrid complexity**: Running both classical and PQC adds overhead and attack surface
- **Harvest attacks**: Data encrypted today is already vulnerable if later decrypted

## Decision

Adopt **pure post-quantum cryptography from day one** with no classical fallback:

### Cryptographic Stack

| Layer | Algorithm | Standard | Security Level |
|-------|-----------|----------|----------------|
| Signatures | ML-DSA-65 | FIPS 204 | ~128-bit |
| Key Exchange | ML-KEM-768 | FIPS 203 | ~128-bit |
| Symmetric Encryption | ChaCha20-Poly1305 | RFC 8439 | 256-bit |
| Hashing/KDF | BLAKE3 | - | 256-bit |

### Identity as Cryptographic Binding

```rust
// PeerId derived from public key, not random
let keypair = MlDsaKeyPair::generate()?;
let peer_id = PeerId::from_pubkey(keypair.public_key());
// peer_id = BLAKE3(ml_dsa_pubkey)[0..32]
```

**Why**: Prevents key confusion attacks. PeerId cannot be claimed without corresponding private key.

### Message Authentication

Every control message is signed:

```rust
struct SignedMessage {
    header: MessageHeader,      // topic, kind, TTL, hop count
    payload: Option<Bytes>,     // message content
    signature: MlDsaSignature,  // 2448 bytes (ML-DSA-65)
    public_key: MlDsaPublicKey, // sender's public key
}
```

**Verification flow**:
1. Extract sender's public key from message
2. Derive expected PeerId from public key
3. Verify ML-DSA signature over (header || payload)
4. Check PeerId matches expected sender

### Key Exchange (QUIC TLS 1.3)

QUIC handshake uses ML-KEM-768 for key encapsulation:

```
Client                                  Server
  |                                        |
  |  ClientHello + ML-KEM public key       |
  |--------------------------------------->|
  |                                        |
  |  ServerHello + ML-KEM ciphertext       |
  |  + Encrypted Extensions                |
  |  + Certificate (ML-DSA)                |
  |  + CertificateVerify (ML-DSA sig)      |
  |<---------------------------------------|
  |                                        |
  |  Finished (ChaCha20-Poly1305)          |
  |--------------------------------------->|
  |                                        |
  |     Application Data (encrypted)       |
  |<-------------------------------------->|
```

### Algorithms Explicitly Excluded

We **do not support** and **never will support**:

| Algorithm | Type | Why Excluded |
|-----------|------|--------------|
| Ed25519 | Signature | Broken by Shor's algorithm |
| Ed448 | Signature | Broken by Shor's algorithm |
| X25519 | Key Exchange | Broken by Shor's algorithm |
| X448 | Key Exchange | Broken by Shor's algorithm |
| RSA | Signature/KE | Broken by Shor's algorithm |
| ECDSA | Signature | Broken by Shor's algorithm |
| AES-GCM | Encryption | No weakness, but ChaCha20 preferred for performance |
| SHA-256 | Hash | BLAKE3 faster and more modern |

**No hybrid mode**: We don't offer "PQC + classical" because:
- Adds implementation complexity
- Increases attack surface
- False security if classical portion has bugs
- Migration complete from day one

## Consequences

### Benefits

1. **Future-proof**: Immune to quantum computer attacks
2. **No migration needed**: Correct cryptography from launch
3. **Simpler code**: One cryptographic path, not two
4. **FIPS compliance**: Uses NIST-standardized algorithms
5. **Harvest resistance**: Encrypted traffic safe even if stored for decades

### Trade-offs

1. **Larger signatures**: ML-DSA-65 signatures are 2448 bytes vs 64 bytes for Ed25519
2. **Larger keys**: ML-DSA public keys are 1952 bytes vs 32 bytes
3. **No classical interop**: Cannot communicate with Ed25519-based systems
4. **Newer algorithms**: Less deployment experience than classical crypto

### Size Impact

| Operation | Classical (Ed25519) | Post-Quantum (ML-DSA-65) | Increase |
|-----------|---------------------|--------------------------|----------|
| Signature | 64 bytes | 2448 bytes | 38x |
| Public Key | 32 bytes | 1952 bytes | 61x |
| Key Exchange | 32 bytes | 1088 bytes (ML-KEM-768) | 34x |

**Mitigation**:
- Signatures only on control messages, not bulk data
- Key exchange once per connection, amortized over message stream
- Bandwidth overhead acceptable on modern networks

### Performance

ML-DSA-65 operations on modern hardware:

| Operation | Time |
|-----------|------|
| Key Generation | ~50 microseconds |
| Sign | ~100 microseconds |
| Verify | ~50 microseconds |

At 1000 messages/second, signature overhead is ~100ms total CPU time, negligible.

## Alternatives Considered

### 1. Classical Crypto with Future PQC Migration

Start with Ed25519/X25519, migrate later.

**Rejected because**:
- "Harvest now, decrypt later" attacks already occurring
- Migration coordination is extremely difficult
- Hybrid period would double complexity
- Delay is not justified by performance concerns

### 2. Hybrid Classical + PQC

Use both Ed25519 and ML-DSA together.

**Rejected because**:
- Doubles signature size and verification cost
- Implementation complexity increases attack surface
- Classical portion adds no security if PQC is working
- Eventual migration to pure PQC still required

### 3. SPHINCS+ (Hash-Based Signatures)

Use SPHINCS+ instead of ML-DSA.

**Rejected because**:
- Much larger signatures (7856-49856 bytes)
- Slower signing (~10x)
- ML-DSA sufficient for our security requirements

### 4. ML-KEM-1024 / ML-DSA-87

Use highest security levels.

**Rejected because**:
- ML-DSA-65 provides ~128-bit security (sufficient)
- Larger keys/signatures with marginal benefit
- May upgrade if cryptanalysis suggests need

## References

- **FIPS 203**: ML-KEM (Module-Lattice-Based Key Encapsulation Mechanism)
  - https://csrc.nist.gov/pubs/fips/203/final
- **FIPS 204**: ML-DSA (Module-Lattice-Based Digital Signature Algorithm)
  - https://csrc.nist.gov/pubs/fips/204/final
- **Implementation**: `crates/identity/src/` (ML-DSA keypairs)
- **Dependency**: `saorsa-pqc` crate wrapping pqcrypto
- **ant-quic ADR-003**: Pure Post-Quantum Cryptography
