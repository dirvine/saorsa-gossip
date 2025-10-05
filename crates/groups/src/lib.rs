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

    /// Create a new group context from an entity identifier string
    ///
    /// This is a convenience constructor that derives the TopicId from the entity_id.
    /// Equivalent to `GroupContext::new(TopicId::from_entity(entity_id)?)`
    ///
    /// # Arguments
    /// * `entity_id` - String identifier for the entity (channel, project, org, etc.)
    ///
    /// # Returns
    /// * `Result<Self>` - GroupContext with topic_id derived from entity_id
    pub fn from_entity(entity_id: &str) -> Result<Self, anyhow::Error> {
        let topic_id = TopicId::from_entity(entity_id)?;
        Ok(Self::new(topic_id))
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

    // TDD: New failing tests for GroupContext::from_entity

    #[test]
    fn test_group_context_from_entity() {
        // RED: This should fail because from_entity doesn't exist yet
        let entity_id = "channel-general";
        let ctx = GroupContext::from_entity(entity_id).expect("should create from entity");

        assert_eq!(ctx.epoch, 0);
        assert!(matches!(ctx.cipher_suite, CipherSuite::MlKem768MlDsa65));
    }

    #[test]
    fn test_group_context_from_entity_deterministic() {
        // Same entity ID should produce same topic ID
        let entity_id = "project-alpha";
        let ctx1 = GroupContext::from_entity(entity_id).expect("should create");
        let ctx2 = GroupContext::from_entity(entity_id).expect("should create");

        assert_eq!(ctx1.topic_id, ctx2.topic_id, "Same entity should produce same topic");
    }

    #[test]
    fn test_group_context_from_entity_vs_new() {
        // from_entity should be equivalent to new(TopicId::from_entity(...))
        let entity_id = "org-acme";
        let ctx_from_entity = GroupContext::from_entity(entity_id).expect("should create");
        let topic = TopicId::from_entity(entity_id).expect("should create topic");
        let ctx_from_new = GroupContext::new(topic);

        assert_eq!(ctx_from_entity.topic_id, ctx_from_new.topic_id);
        assert_eq!(ctx_from_entity.epoch, ctx_from_new.epoch);
    }
}
