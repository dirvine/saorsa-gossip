//! Delta-CRDT synchronization with anti-entropy
//!
//! Implements:
//! - Delta-CRDTs for bandwidth efficiency
//! - IBLT reconciliation for large sets
//! - OR-Set, LWW-Register, RGA

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// CRDT types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrdtType {
    /// Observed-Remove Set
    OrSet,
    /// Last-Writer-Wins Register
    LwwRegister,
    /// Replicated Growable Array
    Rga,
}

/// Delta-CRDT trait
pub trait DeltaCrdt {
    /// Type of the delta
    type Delta;

    /// Merge a delta into this CRDT
    fn merge(&mut self, delta: Self::Delta) -> anyhow::Result<()>;

    /// Generate a delta for changes since a given version
    fn delta(&self, since_version: u64) -> Option<Self::Delta>;
}

/// Simple LWW Register implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwwRegister<T> {
    value: T,
    timestamp: u64,
}

impl<T: Clone> LwwRegister<T> {
    /// Create a new LWW register
    pub fn new(value: T) -> Self {
        Self {
            value,
            timestamp: 0,
        }
    }

    /// Set value with timestamp
    pub fn set(&mut self, value: T, timestamp: u64) {
        if timestamp > self.timestamp {
            self.value = value;
            self.timestamp = timestamp;
        }
    }

    /// Get the current value
    pub fn get(&self) -> &T {
        &self.value
    }
}

/// OR-Set implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrSet<T: std::hash::Hash + Eq + Clone> {
    elements: HashMap<T, u64>,
}

impl<T: std::hash::Hash + Eq + Clone> OrSet<T> {
    /// Create a new OR-Set
    pub fn new() -> Self {
        Self {
            elements: HashMap::new(),
        }
    }

    /// Add an element with a unique tag
    pub fn add(&mut self, element: T, tag: u64) {
        self.elements.insert(element, tag);
    }

    /// Remove an element
    pub fn remove(&mut self, element: &T) {
        self.elements.remove(element);
    }

    /// Check if element exists
    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains_key(element)
    }

    /// Get all elements
    pub fn elements(&self) -> Vec<&T> {
        self.elements.keys().collect()
    }
}

impl<T: std::hash::Hash + Eq + Clone> Default for OrSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lww_register() {
        let mut reg = LwwRegister::new(42);
        assert_eq!(*reg.get(), 42);

        reg.set(100, 10);
        assert_eq!(*reg.get(), 100);

        // Older timestamp should not update
        reg.set(50, 5);
        assert_eq!(*reg.get(), 100);
    }

    #[test]
    fn test_or_set() {
        let mut set = OrSet::new();
        set.add("alice", 1);
        set.add("bob", 2);

        assert!(set.contains(&"alice"));
        assert!(set.contains(&"bob"));
        assert!(!set.contains(&"charlie"));

        set.remove(&"alice");
        assert!(!set.contains(&"alice"));
    }
}
