//! Bounded priority queue.
//!
//! `BinaryHeap` behind a `parking_lot::Mutex`. Entries carry (priority, seq, item) where
//! `seq` is a monotonic submission counter. The heap order is by priority DESC then seq ASC,
//! so the highest-priority item pops first and ties break FIFO.
//!
//! `try_push` enforces a hard capacity. When full it returns `Err` so the caller can decide
//! whether to drop the task, log, or escalate.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use anyhow::{anyhow, Result};
use parking_lot::Mutex;

/// Heap entry. We invert the natural ordering so `BinaryHeap` (a max-heap) yields
/// highest priority first, then earliest seq first.
struct Entry<T> {
    priority: u8,
    seq: u64,
    item: T,
}

impl<T> PartialEq for Entry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.seq == other.seq
    }
}
impl<T> Eq for Entry<T> {}

impl<T> PartialOrd for Entry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Entry<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority => greater. Ties: lower seq => greater (FIFO).
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => other.seq.cmp(&self.seq),
            ord => ord,
        }
    }
}

pub struct PriorityQueue<T> {
    inner: Mutex<BinaryHeap<Entry<T>>>,
    capacity: usize,
    seq: AtomicU64,
}

impl<T> PriorityQueue<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(BinaryHeap::with_capacity(capacity.min(1024))),
            capacity,
            seq: AtomicU64::new(0),
        }
    }

    /// Push with priority. Returns `Err` if queue is at capacity.
    pub fn try_push(&self, item: T, priority: u8) -> Result<()> {
        let mut guard = self.inner.lock();
        if guard.len() >= self.capacity {
            return Err(anyhow!("priority queue full (capacity={})", self.capacity));
        }
        let seq = self.seq.fetch_add(1, AtomicOrdering::Relaxed);
        guard.push(Entry { priority, seq, item });
        Ok(())
    }

    /// Pop highest priority (FIFO within same priority).
    pub fn pop(&self) -> Option<(u8, T)> {
        let mut guard = self.inner.lock();
        guard.pop().map(|e| (e.priority, e.item))
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_highest_priority_first() {
        let q = PriorityQueue::new(16);
        q.try_push("low", 1).unwrap();
        q.try_push("high", 250).unwrap();
        q.try_push("mid", 100).unwrap();
        assert_eq!(q.pop().unwrap().1, "high");
        assert_eq!(q.pop().unwrap().1, "mid");
        assert_eq!(q.pop().unwrap().1, "low");
        assert!(q.pop().is_none());
    }

    #[test]
    fn fifo_within_same_priority() {
        let q = PriorityQueue::new(16);
        q.try_push("a", 50).unwrap();
        q.try_push("b", 50).unwrap();
        q.try_push("c", 50).unwrap();
        assert_eq!(q.pop().unwrap().1, "a");
        assert_eq!(q.pop().unwrap().1, "b");
        assert_eq!(q.pop().unwrap().1, "c");
    }

    #[test]
    fn rejects_when_full() {
        let q = PriorityQueue::new(2);
        q.try_push("a", 1).unwrap();
        q.try_push("b", 1).unwrap();
        assert!(q.try_push("c", 1).is_err());
        assert_eq!(q.len(), 2);
    }
}
