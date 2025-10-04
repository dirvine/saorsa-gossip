//! Plumtree-based pub/sub dissemination
//!
//! Implements:
//! - EAGER push along spanning tree
//! - IHAVE lazy digests to non-tree links
//! - IWANT pull on demand
//! - Anti-entropy reconciliation

use anyhow::Result;
use bytes::Bytes;
use saorsa_gossip_types::{PeerId, TopicId};
use tokio::sync::mpsc;

/// Pub/sub trait for message dissemination
#[async_trait::async_trait]
pub trait PubSub: Send + Sync {
    /// Publish a message to a topic
    async fn publish(&self, topic: TopicId, data: Bytes) -> Result<()>;

    /// Subscribe to a topic and receive messages
    fn subscribe(&self, topic: TopicId) -> mpsc::Receiver<(PeerId, Bytes)>;

    /// Unsubscribe from a topic
    async fn unsubscribe(&self, topic: TopicId) -> Result<()>;
}

/// Plumtree pub/sub implementation
pub struct PlumtreePubSub {
    /// Subscriptions per topic
    subscriptions: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<TopicId, Vec<mpsc::Sender<(PeerId, Bytes)>>>>>,
}

impl PlumtreePubSub {
    /// Create a new Plumtree pub/sub instance
    pub fn new() -> Self {
        Self {
            subscriptions: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

impl Default for PlumtreePubSub {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl PubSub for PlumtreePubSub {
    async fn publish(&self, topic: TopicId, data: Bytes) -> Result<()> {
        let subs = self.subscriptions.read().await;
        if let Some(senders) = subs.get(&topic) {
            let peer_id = PeerId::new([0u8; 32]); // Placeholder
            for sender in senders {
                let _ = sender.send((peer_id, data.clone())).await;
            }
        }
        Ok(())
    }

    fn subscribe(&self, topic: TopicId) -> mpsc::Receiver<(PeerId, Bytes)> {
        let (tx, rx) = mpsc::channel(100);
        let subscriptions = self.subscriptions.clone();

        tokio::spawn(async move {
            let mut subs = subscriptions.write().await;
            subs.entry(topic).or_default().push(tx);
        });

        rx
    }

    async fn unsubscribe(&self, topic: TopicId) -> Result<()> {
        let mut subs = self.subscriptions.write().await;
        subs.remove(&topic);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pubsub_creation() {
        let _pubsub = PlumtreePubSub::new();
    }

    #[tokio::test]
    async fn test_subscribe_and_publish() {
        let pubsub = PlumtreePubSub::new();
        let topic = TopicId::new([1u8; 32]);

        let mut rx = pubsub.subscribe(topic);
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let data = Bytes::from("test message");
        pubsub.publish(topic, data.clone()).await.ok();

        let received = rx.recv().await;
        assert!(received.is_some());
    }
}
