//! MLS group management
//!
//! Manages MLS groups for secure group communication

use saorsa_gossip_types::TopicId;
use serde::{Deserialize, Serialize};

/// MLS cipher suite (placeholder for saorsa-mls integration)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CipherSuite {
    /// ML-KEM-768 + ML-DSA-65 (default PQC suite)
    MlKem768MlDsa65,
}

/// MLS group context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupContext {
    /// Group/Topic identifier
    pub topic_id: TopicId,
    /// Cipher suite
    pub cipher_suite: CipherSuite,
    /// Current epoch
    pub epoch: u64,
}

impl GroupContext {
    /// Create a new group context
    pub fn new(topic_id: TopicId) -> Self {
        Self {
            topic_id,
            cipher_suite: CipherSuite::MlKem768MlDsa65,
            epoch: 0,
        }
    }

    /// Advance to next epoch
    pub fn next_epoch(&mut self) {
        self.epoch += 1;
    }

    /// Derive exporter secret for presence tags
    pub fn derive_presence_secret(&self, _user_id: &[u8], _time_slice: u64) -> [u8; 32] {
        // Placeholder: KDF(exporter_secret, user_id || time_slice)
        [0u8; 32]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_context() {
        let topic = TopicId::new([1u8; 32]);
        let mut ctx = GroupContext::new(topic);

        assert_eq!(ctx.epoch, 0);
        ctx.next_epoch();
        assert_eq!(ctx.epoch, 1);
    }
}
