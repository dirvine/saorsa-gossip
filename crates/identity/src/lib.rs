//! ML-DSA identity and key management
//!
//! Manages long-term ML-DSA identities

use anyhow::Result;
use saorsa_gossip_types::PeerId;
use serde::{Deserialize, Serialize};

/// ML-DSA key pair (placeholder for saorsa-pqc integration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MlDsaKeyPair {
    /// Public key bytes
    pub public_key: Vec<u8>,
    /// Secret key bytes (to be secured)
    secret_key: Vec<u8>,
}

impl MlDsaKeyPair {
    /// Generate a new ML-DSA key pair (placeholder)
    pub fn generate() -> Result<Self> {
        // Placeholder: would use saorsa-pqc for real ML-DSA-65
        Ok(Self {
            public_key: vec![0u8; 64], // Placeholder size
            secret_key: vec![0u8; 128], // Placeholder size
        })
    }

    /// Get public key
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Derive PeerId from public key
    pub fn peer_id(&self) -> PeerId {
        PeerId::from_pubkey(&self.public_key)
    }

    /// Sign a message (placeholder)
    pub fn sign(&self, _message: &[u8]) -> Result<Vec<u8>> {
        // Placeholder: would use saorsa-pqc for ML-DSA signing
        Ok(vec![0u8; 64])
    }

    /// Verify a signature (placeholder)
    pub fn verify(public_key: &[u8], _message: &[u8], _signature: &[u8]) -> Result<bool> {
        // Placeholder: would use saorsa-pqc for ML-DSA verification
        let _ = public_key;
        Ok(true)
    }
}

/// Identity with human-readable alias
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// ML-DSA key pair
    key_pair: MlDsaKeyPair,
    /// Human-readable alias
    alias: String,
}

impl Identity {
    /// Create a new identity with alias
    pub fn new(alias: String) -> Result<Self> {
        Ok(Self {
            key_pair: MlDsaKeyPair::generate()?,
            alias,
        })
    }

    /// Get the alias
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Get the PeerId
    pub fn peer_id(&self) -> PeerId {
        self.key_pair.peer_id()
    }

    /// Get the key pair
    pub fn key_pair(&self) -> &MlDsaKeyPair {
        &self.key_pair
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = MlDsaKeyPair::generate();
        assert!(keypair.is_ok());
    }

    #[test]
    fn test_identity_creation() {
        let identity = Identity::new("Alice".to_string());
        assert!(identity.is_ok());

        if let Ok(id) = identity {
            assert_eq!(id.alias(), "Alice");
        }
    }

    #[test]
    fn test_peer_id_derivation() {
        let keypair = MlDsaKeyPair::generate().ok();
        if let Some(kp) = keypair {
            let peer_id = kp.peer_id();
            assert_eq!(peer_id.as_bytes().len(), 32);
        }
    }
}
