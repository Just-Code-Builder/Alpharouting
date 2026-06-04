// Nonce allocator with wallet-wedge prevention.
//
// Naive AtomicU64 design leaves holes: any path that allocates a nonce but
// aborts before broadcast (gas gate, sim error, fill failure) creates a gap
// in the sequence. Ethereum requires strict ordering — one missing nonce parks
// every higher-nonce tx in the mempool until it either arrives or is cancelled.
//
// Fix: pair every next() with either a commit (tx sent) or a release() (abort).
// Released nonces go into a BTreeSet; next() drains smallest-first before
// touching the atomic counter, so the sequence stays gapless under any abort
// pattern.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

/// Lock-free nonce allocator. Cheap to clone — all state is Arc-wrapped.
#[derive(Clone, Debug)]
pub struct NonceQueue {
    current: Arc<AtomicU64>,
    /// Released nonces waiting to be re-issued. Typically empty; grows only
    /// on broadcast abort. Bounded by in-flight failures within one reconcile
    /// window — a steadily-growing count signals systematic broadcast failure.
    released: Arc<Mutex<BTreeSet<u64>>>,
}

impl NonceQueue {
    pub fn new(seed: u64) -> Self {
        Self {
            current: Arc::new(AtomicU64::new(seed)),
            released: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    /// Returns the next nonce. Drains the release pool (ascending) before
    /// falling through to the atomic counter. Concurrent callers get distinct
    /// values.
    pub fn next(&self) -> u64 {
        {
            let mut g = self.released.lock();
            if let Some(n) = g.pop_first() {
                return n;
            }
        }
        self.current.fetch_add(1, Ordering::Relaxed)
    }

    /// Return a nonce allocated but never broadcast. Idempotent via set semantics.
    pub fn release(&self, nonce: u64) {
        self.released.lock().insert(nonce);
    }

    pub fn peek(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }

    pub fn released_count(&self) -> usize {
        self.released.lock().len()
    }

    /// Advance the local counter to match the chain's pending nonce if behind.
    /// Never regresses — local bookkeeping is trusted over a potentially stale
    /// RPC response.
    pub fn reconcile(&self, chain_pending: u64) -> u64 {
        loop {
            let local = self.current.load(Ordering::Relaxed);
            if local >= chain_pending {
                return local;
            }
            match self.current.compare_exchange(
                local,
                chain_pending,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return chain_pending,
                Err(_) => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn next_is_monotonic() {
        let q = NonceQueue::new(100);
        assert_eq!(q.next(), 100);
        assert_eq!(q.next(), 101);
        assert_eq!(q.next(), 102);
        assert_eq!(q.peek(), 103);
    }

    #[test]
    fn concurrent_nexts_are_unique() {
        use std::thread;
        let q = Arc::new(NonceQueue::new(0));
        let mut handles = vec![];
        for _ in 0..16 {
            let q2 = q.clone();
            handles.push(thread::spawn(move || {
                (0..100).map(|_| q2.next()).collect::<Vec<u64>>()
            }));
        }
        let mut seen: HashSet<u64> = HashSet::new();
        for h in handles {
            for n in h.join().unwrap() {
                assert!(seen.insert(n), "duplicate nonce: {n}");
            }
        }
        assert_eq!(seen.len(), 1600);
    }

    #[test]
    fn reconcile_advances_when_chain_is_ahead() {
        let q = NonceQueue::new(50);
        let v = q.reconcile(75);
        assert_eq!(v, 75);
        assert_eq!(q.peek(), 75);
    }

    #[test]
    fn reconcile_does_not_regress_when_local_is_ahead() {
        let q = NonceQueue::new(100);
        let v = q.reconcile(50);
        assert_eq!(v, 100);
        assert_eq!(q.peek(), 100);
    }

    #[test]
    fn revert_does_not_re_yield_nonce() {
        // A confirmed-then-reverted tx burns its nonce on-chain.
        // Caller must NOT release it — the next allocation moves forward.
        let q = NonceQueue::new(50);
        let burned = q.next();
        assert_eq!(burned, 50);
        assert_eq!(q.next(), 51);
    }

    #[test]
    fn release_returns_nonce_to_pool() {
        let q = NonceQueue::new(100);
        let n1 = q.next();
        let n2 = q.next();
        assert_eq!((n1, n2), (100, 101));
        q.release(n1);
        assert_eq!(q.released_count(), 1);
        assert_eq!(q.next(), 100, "released nonce should be re-yielded first");
        assert_eq!(q.released_count(), 0);
        assert_eq!(q.next(), 102);
    }

    #[test]
    fn release_pops_smallest_first() {
        let q = NonceQueue::new(10);
        let _ = q.next(); // 10
        let _ = q.next(); // 11
        let _ = q.next(); // 12
        q.release(12);
        q.release(10);
        q.release(11);
        assert_eq!(q.next(), 10);
        assert_eq!(q.next(), 11);
        assert_eq!(q.next(), 12);
        assert_eq!(q.next(), 13);
    }

    #[test]
    fn release_is_idempotent() {
        let q = NonceQueue::new(0);
        let n = q.next();
        q.release(n);
        q.release(n); // dup — no-op via set semantics
        q.release(n);
        assert_eq!(q.released_count(), 1);
        assert_eq!(q.next(), n);
        assert_eq!(q.released_count(), 0);
    }

    #[test]
    fn release_pool_recovers_all_aborted_nonces_at_scale() {
        // Allocate 100, abort every 3rd (34 total), then drain the pool.
        // Counter must not advance during the drain.
        let q = NonceQueue::new(0);
        let allocated: Vec<u64> = (0..100u64).map(|_| q.next()).collect();
        assert_eq!(q.peek(), 100);

        let aborted: Vec<u64> = allocated.iter().copied().filter(|n| n % 3 == 0).collect();
        for n in &aborted { q.release(*n); }
        assert_eq!(aborted.len(), 34);

        let drained: Vec<u64> = (0..34).map(|_| q.next()).collect();
        assert_eq!(q.released_count(), 0);
        let mut sorted = aborted.clone();
        sorted.sort();
        assert_eq!(drained, sorted);

        assert_eq!(q.next(), 100);
    }

    #[test]
    fn release_during_next_iteration_drains_immediately() {
        let q = NonceQueue::new(0);
        let _ = q.next(); // 0
        let _ = q.next(); // 1
        q.release(0);
        assert_eq!(q.next(), 0);
        assert_eq!(q.released_count(), 0);
        assert_eq!(q.next(), 2);
    }
}
