// godmode #8 — Nonce pre-allocation manager.
//
// The executor's parallel path was previously bottlenecked by a `Mutex<()>`
// labeled `in_flight`: even though we spawn N concurrent `process_one` tasks,
// they all serialize on that mutex during broadcast because each broadcast
// needs to ask the chain for the wallet's pending nonce. Replace with a
// monotonically-incrementing `AtomicU64`:
//
//   - At startup: seed `current` with `provider.get_transaction_count(signer,
//     BlockId::Pending)`.
//   - On every broadcast: `nonce_queue.next()` returns a unique nonce, no
//     RPC roundtrip.
//   - On revert or send failure: the nonce IS consumed (Aave reverts still
//     burn the nonce on-chain). Caller does NOT return it.
//   - Periodic reconciliation: every 30s a background task calls
//     `get_transaction_count(signer, Pending)` and `max(local, on_chain)`
//     to recover from drift if external txs were signed by the same wallet.
//
// Test invariants enforced in `crates/chain/tests/nonce_manager.rs`:
//   - 16 concurrent next() calls yield 16 distinct, monotonically increasing
//     u64 values starting from the seed.
//   - reconcile() with `chain_value > local_value` advances local; the
//     reverse case does NOT regress local (we trust local).
//   - A "revert" path simulated via skipping a returned nonce does not
//     re-yield it on the next next() call.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

/// Hands out unique transaction nonces. Cheap to clone.
///
/// CRITICAL DESIGN — the "release pool":
///
/// The naive design (just `Arc<AtomicU64>`) had a wallet-wedge bug: every
/// allocator path that consumed a nonce but failed before broadcast (gas
/// gate rejection, fill() error, sim error) left a permanent hole in the
/// nonce sequence. Ethereum requires strict sequential nonces — a single
/// hole means every higher-nonce tx sits in the mempool forever waiting
/// for a tx that will never arrive.
///
/// The fix: pair every `next()` with either a broadcast or a `release()`.
/// Released nonces go into a sorted `BTreeSet`; the next `next()` call
/// pops the smallest released nonce first, only falling through to the
/// atomic counter when the set is empty. This guarantees we never widen
/// the gap between local and chain pending-nonce when a tx is aborted.
///
/// Concurrency: BTreeSet behind a parking_lot Mutex. The release path is
/// rare (only on broadcast failure); the next() fast path takes a single
/// uncontended mutex acquisition to check the pool, then either pops or
/// falls through to a lock-free atomic fetch_add.
#[derive(Clone, Debug)]
pub struct NonceQueue {
    /// Next nonce to hand out. Monotonically incremented; never decreases.
    current: Arc<AtomicU64>,
    /// Sorted pool of released nonces. Popped first by `next()`. Bounded by
    /// the number of in-flight broadcast failures within a single reconcile
    /// window — typically zero, occasionally a handful.
    released: Arc<Mutex<BTreeSet<u64>>>,
}

impl NonceQueue {
    /// Construct with the wallet's current on-chain pending nonce as seed.
    /// Call this once at executor startup.
    pub fn new(seed: u64) -> Self {
        Self {
            current: Arc::new(AtomicU64::new(seed)),
            released: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    /// Hand out the next nonce. Pops the smallest released nonce first
    /// (via BTreeSet::pop_first); falls through to the atomic counter when
    /// the released pool is empty. Concurrent callers each get a distinct
    /// value.
    pub fn next(&self) -> u64 {
        {
            let mut g = self.released.lock();
            if let Some(n) = g.pop_first() {
                return n;
            }
        }
        self.current.fetch_add(1, Ordering::Relaxed)
    }

    /// Return a previously-allocated nonce to the pool. Call this when a
    /// broadcast is ABORTED (gas gate rejection, sim error, fill failure,
    /// any path where `next()` was called but no tx was actually sent to
    /// the chain). Idempotent — duplicate release is a no-op via the set.
    pub fn release(&self, nonce: u64) {
        self.released.lock().insert(nonce);
    }

    /// Snapshot the current value without consuming it (the next `next()`
    /// call will return THIS value when the released pool is empty).
    pub fn peek(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }

    /// Diagnostic: count of nonces currently sitting in the release pool.
    /// Used by /metrics to surface wedge-risk — a steadily-growing count
    /// signals broadcasts are failing systematically.
    pub fn released_count(&self) -> usize {
        self.released.lock().len()
    }

    /// Reconcile against the chain. If the chain's pending-nonce is ahead
    /// of our local counter (e.g. an out-of-band tx by the same wallet),
    /// advance local. If local is ahead, no-op — we trust our own bookkeeping
    /// over a possibly-stale RPC response. Returns the chosen value.
    pub fn reconcile(&self, chain_pending: u64) -> u64 {
        // CAS loop: bump local up to chain_pending if local is behind.
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
                Err(_) => continue, // someone else bumped local; reread + retry
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
        // 16 threads × 100 nonces each = 1600 unique values
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
        // Simulate: handed out nonce 50, the tx reverted. The next call
        // should yield 51, not 50. A confirmed-then-reverted tx burns its
        // nonce on-chain; caller does NOT release for these.
        let q = NonceQueue::new(50);
        let burned = q.next();
        assert_eq!(burned, 50);
        // Caller decides NOT to return it — that's the intended pattern.
        let next = q.next();
        assert_eq!(next, 51);
    }

    #[test]
    fn release_returns_nonce_to_pool() {
        let q = NonceQueue::new(100);
        let n1 = q.next();
        let n2 = q.next();
        assert_eq!((n1, n2), (100, 101));
        // Pretend n1 was aborted before broadcast — return it.
        q.release(n1);
        assert_eq!(q.released_count(), 1);
        // Next allocation should pop n1 from the released pool, not
        // bump local counter.
        let n3 = q.next();
        assert_eq!(n3, 100, "released nonce should be re-yielded first");
        assert_eq!(q.released_count(), 0);
        // Subsequent allocation falls through to atomic counter.
        let n4 = q.next();
        assert_eq!(n4, 102);
    }

    #[test]
    fn release_pops_smallest_first() {
        let q = NonceQueue::new(10);
        let _ = q.next(); // 10
        let _ = q.next(); // 11
        let _ = q.next(); // 12
        // Release out of order.
        q.release(12);
        q.release(10);
        q.release(11);
        // pop_first gives ascending order.
        assert_eq!(q.next(), 10);
        assert_eq!(q.next(), 11);
        assert_eq!(q.next(), 12);
        // Pool empty → atomic counter resumes at 13.
        assert_eq!(q.next(), 13);
    }

    #[test]
    fn release_is_idempotent() {
        let q = NonceQueue::new(0);
        let n = q.next();
        q.release(n);
        q.release(n); // dup — no-op via set semantics.
        q.release(n); // dup
        assert_eq!(q.released_count(), 1);
        assert_eq!(q.next(), n);
        assert_eq!(q.released_count(), 0);
    }

    #[test]
    fn release_pool_recovers_all_aborted_nonces_at_scale() {
        // Phased scenario: allocate 100 distinct nonces (no releases yet),
        // then release a chosen 34 of them in a batch. Then drain the pool
        // by allocating 34 more — every one should come from the released
        // pool (smallest-first via BTreeSet::pop_first), and the counter
        // should not have advanced past 100. This validates that the
        // release pool prevents wedge: in production, aborts and new fires
        // interleave but the pool always gets drained before the counter
        // is touched.
        let q = NonceQueue::new(0);
        let allocated: Vec<u64> = (0..100u64).map(|_| q.next()).collect();
        assert_eq!(allocated, (0..100u64).collect::<Vec<_>>());
        assert_eq!(q.peek(), 100);

        // Release every 3rd nonce (i=0,3,...,99 → 34 values).
        let aborted: Vec<u64> = allocated.iter().copied().filter(|n| n % 3 == 0).collect();
        for n in &aborted { q.release(*n); }
        assert_eq!(aborted.len(), 34);
        assert_eq!(q.released_count(), 34);

        // Drain — next 34 calls should pop from the pool, ascending.
        let drained: Vec<u64> = (0..34).map(|_| q.next()).collect();
        assert_eq!(q.released_count(), 0);
        let mut aborted_sorted = aborted.clone();
        aborted_sorted.sort();
        assert_eq!(drained, aborted_sorted);

        // Pool now empty. Next allocation continues from the counter's
        // high-water (100 — never regressed during the drain).
        assert_eq!(q.next(), 100);
    }

    #[test]
    fn release_during_next_iteration_drains_immediately() {
        // Documents the OTHER realistic flow: release and re-allocation
        // happen in the SAME iteration tick. The previously-released
        // nonce gets reused on the very next call — which is exactly what
        // we want for sustained-fire scenarios with frequent aborts.
        let q = NonceQueue::new(0);
        let _ = q.next(); // 0
        let _ = q.next(); // 1
        q.release(0);
        assert_eq!(q.released_count(), 1);
        // Next call drains it.
        assert_eq!(q.next(), 0);
        assert_eq!(q.released_count(), 0);
        // And the counter hasn't moved past 2.
        assert_eq!(q.next(), 2);
    }
}
