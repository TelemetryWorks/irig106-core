//! Slice 2 storage only: no packet admission, framing or automatic coalescing.
use std::{
    collections::TryReserveError,
    fmt, mem,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Instant,
};
/// Fixed payload capacity, including future packet headers/trailer.
pub const BLOCK_BYTES: usize = 524_288;
/// Default shared storage budget, including accounted control/lease metadata.
pub const DEFAULT_POOL_BUDGET: usize = 268_435_456;
const IDLE_TARGET: usize = 8;

/// Typed storage failure. Cancellation is terminal for the shared pool.
#[derive(Debug)]
#[non_exhaustive]
pub enum PoolError {
    /// The requested working set cannot fit even after all other blocks return.
    #[non_exhaustive]
    BudgetTooSmall {
        /// Conservative required bytes, usize::MAX if arithmetic overflowed.
        needed: usize,
        /// Configured aggregate limit.
        limit: usize,
    },
    /// Zero working set, exhausted assembly, or unsupported target-sized limit.
    InvalidRequest,
    /// No outstanding assembly or block exists that could release resources.
    BudgetUnavailable,
    /// An explicit nonblocking acquisition encountered temporary contention.
    WouldBlock,
    /// An explicit absolute wait deadline expired while resources were unavailable.
    DeadlineExceeded,
    /// The caller cancelled this pool and all pending/future acquisitions.
    Cancelled,
    /// A synchronization failure; the pool must not be used again.
    Poisoned,
    /// Allocator supplied a different capacity than the fixed block geometry.
    UnexpectedCapacity,
    /// Fallible storage/control reservation failed.
    #[non_exhaustive]
    Allocation {
        /// Original allocator error.
        source: TryReserveError,
    },
}
impl fmt::Display for PoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetTooSmall { needed, limit } => {
                write!(f, "storage needs {needed} bytes; budget is {limit}")
            }
            Self::InvalidRequest => f.write_str("invalid pool working set or target limit"),
            Self::BudgetUnavailable => {
                f.write_str("storage unavailable with no outstanding resource returner")
            }
            Self::WouldBlock => f.write_str("storage acquisition would block"),
            Self::DeadlineExceeded => f.write_str("storage wait deadline exceeded"),
            Self::Cancelled => f.write_str("storage pool cancelled"),
            Self::Poisoned => f.write_str("storage pool synchronization failed"),
            Self::UnexpectedCapacity => {
                f.write_str("allocator returned an unexpected block capacity")
            }
            Self::Allocation { source } => write!(f, "storage reservation failed: {source}"),
        }
    }
}
impl std::error::Error for PoolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Allocation { source } => Some(source),
            _ => None,
        }
    }
}
impl From<TryReserveError> for PoolError {
    fn from(source: TryReserveError) -> Self {
        Self::Allocation { source }
    }
}
#[derive(Debug)]
struct State {
    idle: Vec<Vec<u8>>,
    allocated: usize,
    assembling: bool,
    cancelled: bool,
    waiting: usize,
    allocations: u64,
    #[cfg(test)]
    wakes: usize,
}
#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    budget: usize,
    fixed: usize,
    max_blocks: usize,
}
// Conservative lease/workspace charge; allocator-private overhead needs RSS margin.
const UNIT: usize = BLOCK_BYTES + mem::size_of::<PooledBlock>();
const CONTROL: usize =
    mem::size_of::<Shared>() + mem::size_of::<BlockAssembly>() + 4 * mem::size_of::<usize>();

/// Current accounted storage. This is not a process-RSS measurement.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct PoolStats {
    /// Aggregate configured bytes.
    pub budget_bytes: usize,
    /// Control capacity plus all outstanding/idle blocks and lease allowance.
    pub used_bytes: usize,
    /// Blocks currently owned by the pool or workers.
    pub allocated_blocks: usize,
    /// Reusable blocks; ordinary returns may retain more than eight.
    pub idle_blocks: usize,
    /// Successful cold payload allocations (saturates at u64::MAX).
    pub block_allocations: u64,
    /// Current condition-variable waiters.
    pub waiting: usize,
}
/// Shared lazy block pool. Cloning a handle never allocates payload storage.
#[derive(Debug, Clone)]
pub struct BlockPool {
    shared: Arc<Shared>,
}
impl BlockPool {
    /// Construct with the default 256 MiB budget and zero payload blocks.
    pub fn with_defaults() -> Result<Self, PoolError> {
        Self::new(DEFAULT_POOL_BUDGET)
    }
    /// Reserve only bounded control descriptors; no payload is allocated/touched.
    /// Descriptor capacity is charged up front so returning blocks cannot allocate.
    pub fn new(budget: usize) -> Result<Self, PoolError> {
        if budget > isize::MAX as usize {
            return Err(PoolError::InvalidRequest);
        }
        let mut idle = Vec::<Vec<u8>>::new();
        idle.try_reserve_exact(budget / BLOCK_BYTES)?;
        let fixed = CONTROL + idle.capacity() * mem::size_of::<Vec<u8>>();
        let max_blocks = budget.saturating_sub(fixed) / UNIT;
        if max_blocks == 0 {
            return Err(PoolError::BudgetTooSmall {
                needed: fixed + UNIT,
                limit: budget,
            });
        }
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    idle,
                    allocated: 0,
                    assembling: false,
                    cancelled: false,
                    waiting: 0,
                    allocations: 0,
                    #[cfg(test)]
                    wakes: 0,
                }),
                changed: Condvar::new(),
                budget,
                fixed,
                max_blocks,
            }),
        })
    }
    fn lock(&self) -> Result<MutexGuard<'_, State>, PoolError> {
        self.shared.state.lock().map_err(|_| PoolError::Poisoned)
    }
    fn wait<'a>(
        &self,
        mut state: MutexGuard<'a, State>,
        deadline: Option<Instant>,
    ) -> Result<MutexGuard<'a, State>, PoolError> {
        // No ownership/thread inference: only absence of all possible returners
        // proves that waiting cannot help. Callers check readiness before this.
        if !state.assembling && state.allocated == state.idle.len() {
            return Err(PoolError::BudgetUnavailable);
        }
        let deadline = deadline.ok_or(PoolError::WouldBlock)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or(PoolError::DeadlineExceeded)?;
        state.waiting += 1;
        let (mut state, _) = self
            .shared
            .changed
            .wait_timeout(state, remaining)
            .map_err(|_| PoolError::Poisoned)?;
        state.waiting -= 1;
        #[cfg(test)]
        {
            state.wakes += 1;
        }
        // The caller rechecks readiness/cancel and this same absolute deadline.
        Ok(state)
    }
    /// Acquire the sole assembly permit, after checking eventual feasibility.
    /// `blocks` is a maximum working set, not a reservation/allocation request.
    /// Explicitly nonblocking: contention returns WouldBlock. No thread ownership
    /// inference is made. Use begin_until for deadline-bounded waiting.
    pub fn try_begin(&self, blocks: usize) -> Result<BlockAssembly, PoolError> {
        self.begin_using(blocks, None)
    }
    /// Wait until the absolute deadline when another owner can release admission.
    /// Readiness is checked first; the deadline bounds waiting, not allocation/I/O.
    pub fn begin_until(
        &self,
        blocks: usize,
        deadline: Instant,
    ) -> Result<BlockAssembly, PoolError> {
        self.begin_using(blocks, Some(deadline))
    }
    fn begin_using(
        &self,
        blocks: usize,
        deadline: Option<Instant>,
    ) -> Result<BlockAssembly, PoolError> {
        if blocks == 0 {
            return Err(PoolError::InvalidRequest);
        }
        if blocks > self.shared.max_blocks {
            return Err(PoolError::BudgetTooSmall {
                needed: blocks
                    .checked_mul(UNIT)
                    .and_then(|n| n.checked_add(self.shared.fixed))
                    .unwrap_or(usize::MAX),
                limit: self.shared.budget,
            });
        }
        let mut state = self.lock()?;
        loop {
            if state.cancelled {
                return Err(PoolError::Cancelled);
            }
            if !state.assembling {
                state.assembling = true;
                break;
            }
            state = self.wait(state, deadline)?;
        }
        Ok(BlockAssembly {
            pool: self.clone(),
            remaining: blocks,
        })
    }
    /// Trim surplus idle storage under explicit external budget pressure.
    pub fn trim_for_pressure(&self) -> Result<(), PoolError> {
        let mut state = self.lock()?;
        while state.idle.len() > IDLE_TARGET {
            drop(state.idle.pop());
            state.allocated -= 1;
        }
        self.shared.changed.notify_all();
        Ok(())
    }
    /// Notify after returning one packet's actual blocks (never its declaration).
    /// Only a working set larger than eight triggers hysteresis trimming.
    pub fn trim_after_return(&self, packet_blocks: usize) -> Result<(), PoolError> {
        if packet_blocks > IDLE_TARGET {
            self.trim_for_pressure()?;
        }
        Ok(())
    }
    /// Permanently cancel and wake all waiters; returned blocks are freed.
    pub fn cancel(&self) -> Result<(), PoolError> {
        let mut state = self.lock()?;
        state.cancelled = true;
        let idle = state.idle.len();
        state.idle.clear();
        state.allocated -= idle;
        self.shared.changed.notify_all();
        Ok(())
    }
    /// Inspect accounted storage without allocating or retaining history.
    pub fn stats(&self) -> Result<PoolStats, PoolError> {
        let state = self.lock()?;
        Ok(PoolStats {
            budget_bytes: self.shared.budget,
            used_bytes: self.shared.fixed + state.allocated * UNIT,
            allocated_blocks: state.allocated,
            idle_blocks: state.idle.len(),
            block_allocations: state.allocations,
            waiting: state.waiting,
        })
    }
}
/// Exclusive assembly admission, released on drop, including errors/early EOF.
/// This primitive does not yet parse or validate packets.
#[derive(Debug)]
pub struct BlockAssembly {
    pool: BlockPool,
    remaining: usize,
}
impl BlockAssembly {
    /// Obtain one initialized final-storage block without waiting for credits.
    /// Explicitly nonblocking: WouldBlock means other blocks retain the credits.
    /// The caller must only acquire after filling the preceding block; this
    /// storage primitive has no knowledge of input arrival or valid frame length.
    pub fn try_next_block(&mut self) -> Result<PooledBlock, PoolError> {
        self.next_block_using(None, Self::allocate)
    }
    /// Wait until an absolute deadline when another owner can return credits.
    /// A timeout does not cancel the pool or consume an assembly slot.
    pub fn next_block_until(&mut self, deadline: Instant) -> Result<PooledBlock, PoolError> {
        self.next_block_using(Some(deadline), Self::allocate)
    }
    fn allocate() -> Result<Vec<u8>, PoolError> {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(BLOCK_BYTES)?;
        Ok(bytes)
    }
    fn next_block_using(
        &mut self,
        deadline: Option<Instant>,
        allocate: impl FnOnce() -> Result<Vec<u8>, PoolError>,
    ) -> Result<PooledBlock, PoolError> {
        if self.remaining == 0 {
            return Err(PoolError::InvalidRequest);
        }
        let mut state = self.pool.lock()?;
        let bytes = loop {
            if state.cancelled {
                return Err(PoolError::Cancelled);
            }
            if let Some(bytes) = state.idle.pop() {
                break bytes;
            }
            if state.allocated < self.pool.shared.max_blocks {
                state.allocated += 1; // Claim before reservation/initialization.
                drop(state); // Cold allocation/zeroing must not block worker returns.
                let result = allocate().and_then(|mut bytes| {
                    if bytes.capacity() != BLOCK_BYTES {
                        return Err(PoolError::UnexpectedCapacity);
                    }
                    bytes.resize(BLOCK_BYTES, 0);
                    Ok(bytes)
                });
                state = self.pool.lock()?;
                if state.cancelled {
                    drop(result); // Free before returning the provisional credit.
                    state.allocated -= 1;
                    self.pool.shared.changed.notify_all();
                    return Err(PoolError::Cancelled);
                }
                match result {
                    Ok(bytes) => {
                        state.allocations = state.allocations.saturating_add(1);
                        break bytes;
                    }
                    Err(error) => {
                        state.allocated -= 1;
                        self.pool.shared.changed.notify_all();
                        return Err(error);
                    }
                }
            }
            state = self.pool.wait(state, deadline)?;
        };
        self.remaining -= 1;
        Ok(PooledBlock {
            pool: self.pool.clone(),
            bytes,
        })
    }
}
impl Drop for BlockAssembly {
    fn drop(&mut self) {
        if let Ok(mut state) = self.pool.lock() {
            state.assembling = false;
            self.pool.shared.changed.notify_all();
        }
    }
}
/// Owned initialized storage, returned automatically without allocating.
/// Bytes persist across reuse; this is capacity, **not** a validated packet view.
/// ```compile_fail,E0505
/// use irig106_core::BlockPool;
/// let pool = BlockPool::with_defaults().unwrap();
/// let mut assembly = pool.try_begin(1).unwrap();
/// let block = assembly.try_next_block().unwrap();
/// let view = block.storage();
/// drop(block);
/// println!("{}", view[0]);
/// ```
#[derive(Debug)]
pub struct PooledBlock {
    pool: BlockPool,
    bytes: Vec<u8>,
}
impl PooledBlock {
    /// Initialized capacity; contents are not necessarily valid packet bytes.
    pub fn storage(&self) -> &[u8] {
        &self.bytes
    }
    /// Final direct-read destination; later reader integration tracks valid length.
    pub fn storage_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}
impl Drop for PooledBlock {
    fn drop(&mut self) {
        let bytes = mem::take(&mut self.bytes);
        if let Ok(mut state) = self.pool.lock() {
            if state.cancelled || state.idle.len() == state.idle.capacity() {
                drop(bytes);
                state.allocated -= 1;
            } else {
                state.idle.push(bytes);
            }
            self.pool.shared.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_reservation_and_capacity_mismatch_roll_back_credits() {
        let pool = BlockPool::with_defaults().unwrap();
        let before = pool.stats().unwrap().used_bytes;
        let mut assembly = pool.try_begin(1).unwrap();
        let error = assembly.next_block_using(None, || {
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(usize::MAX)?;
            Ok(bytes)
        });
        assert!(matches!(error, Err(PoolError::Allocation { .. })));
        assert_eq!(pool.stats().unwrap().used_bytes, before);
        assert!(matches!(
            assembly.next_block_using(None, || Ok(Vec::new())),
            Err(PoolError::UnexpectedCapacity)
        ));
        assert_eq!(pool.stats().unwrap().used_bytes, before);
        drop(assembly.try_next_block().unwrap());
        assert_eq!(pool.stats().unwrap().allocated_blocks, 1);
    }
    #[test]
    fn cancel_clears_idle_blocks_and_credits() {
        let pool = BlockPool::with_defaults().unwrap();
        let fixed = pool.stats().unwrap().used_bytes;
        let mut a = pool.try_begin(2).unwrap();
        let blocks = [a.try_next_block().unwrap(), a.try_next_block().unwrap()];
        drop(a);
        drop(blocks);
        assert_eq!(pool.stats().unwrap().idle_blocks, 2);
        pool.cancel().unwrap();
        let cancelled = pool.stats().unwrap();
        assert_eq!((cancelled.allocated_blocks, cancelled.idle_blocks), (0, 0));
        assert_eq!(cancelled.used_bytes, fixed);
        pool.cancel().unwrap();
        assert_eq!(pool.stats().unwrap().used_bytes, fixed);
    }
    fn bounded<T: Send + 'static>(operation: impl FnOnce() -> T + Send + 'static) -> T {
        let worker = std::thread::spawn(operation);
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !worker.is_finished() {
            assert!(
                Instant::now() < deadline,
                "pool operation exceeded watchdog"
            );
            std::thread::yield_now();
        }
        worker.join().unwrap()
    }
    #[test]
    fn readiness_after_wake_wins_over_expired_deadline() {
        bounded(|| {
            let pool = BlockPool::with_defaults().unwrap();
            let owner = pool.try_begin(1).unwrap();
            let deadline = Instant::now() + std::time::Duration::from_secs(1);
            let p = pool.clone();
            let worker = std::thread::spawn(move || p.begin_until(1, deadline));
            let watchdog = Instant::now() + std::time::Duration::from_secs(3);
            loop {
                let mut state = pool.lock().unwrap();
                if state.waiting == 1 {
                    // Retain the mutex until expiry: even a timeout/spurious wake
                    // cannot return the guard until readiness is already true.
                    state.assembling = false;
                    while Instant::now() <= deadline {
                        std::thread::yield_now();
                    }
                    pool.shared.changed.notify_all();
                    break;
                }
                assert!(Instant::now() < watchdog);
                drop(state);
                std::thread::yield_now();
            }
            drop(bounded(move || worker.join().unwrap().unwrap()));
            drop(owner);
        });
    }
    #[test]
    fn no_returner_is_provable_only_without_outstanding_resources() {
        bounded(|| {
            let pool = BlockPool::with_defaults().unwrap();
            // Exercise the wait boundary directly: public acquisition is ready in
            // this state, so it never needs to wait. No owner can release anything.
            let state = pool.lock().unwrap();
            assert!(matches!(
                pool.wait(state, Some(Instant::now())),
                Err(PoolError::BudgetUnavailable)
            ));
            assert_eq!(pool.stats().unwrap().waiting, 0);
            // An outstanding permit is a possible returner even on this thread.
            let owner = pool.try_begin(1).unwrap();
            assert!(matches!(pool.try_begin(1), Err(PoolError::WouldBlock)));
            assert!(matches!(
                pool.begin_until(1, Instant::now()),
                Err(PoolError::DeadlineExceeded)
            ));
            drop(owner);
        });
    }
    #[test]
    fn deadlines_expire_without_cancelling_or_consuming_resources() {
        bounded(|| {
            use std::time::Duration;
            let pool = BlockPool::new(BLOCK_BYTES + 4096).unwrap();
            let mut owner = pool.try_begin(1).unwrap();
            assert!(matches!(
                pool.begin_until(1, Instant::now() + Duration::from_millis(1)),
                Err(PoolError::DeadlineExceeded)
            ));
            let block = owner.try_next_block().unwrap();
            drop(owner);
            let mut next = pool.try_begin(1).unwrap();
            assert!(matches!(
                next.next_block_until(Instant::now() + Duration::from_millis(1)),
                Err(PoolError::DeadlineExceeded)
            ));
            assert_eq!(pool.stats().unwrap().waiting, 0);
            drop(block);
            // Readiness is checked before the wait deadline; the failed attempt kept its slot.
            drop(next.next_block_until(Instant::now()).unwrap());
        });
    }
    #[test]
    fn spurious_notification_rechecks_predicate_and_cancellation() {
        use std::{thread, time::Duration};
        let pool = BlockPool::with_defaults().unwrap();
        let owner = pool.try_begin(1).unwrap();
        let p = pool.clone();
        let worker =
            thread::spawn(move || p.begin_until(1, Instant::now() + Duration::from_secs(30)));
        let wait_for = |predicate: fn(&State) -> bool| {
            let limit = Instant::now() + Duration::from_secs(5);
            while !predicate(&pool.lock().unwrap()) {
                assert!(Instant::now() < limit);
                thread::yield_now();
            }
        };
        wait_for(|s| s.waiting == 1);
        pool.shared.changed.notify_all(); // No resource became available.
        wait_for(|s| s.wakes > 0 && s.waiting == 1);
        pool.cancel().unwrap();
        let limit = Instant::now() + Duration::from_secs(5);
        while !worker.is_finished() {
            assert!(Instant::now() < limit);
            thread::yield_now();
        }
        assert!(matches!(worker.join().unwrap(), Err(PoolError::Cancelled)));
        drop(owner);
    }
    #[test]
    fn cold_allocation_unlocks_and_cancel_rolls_back() {
        let pool = BlockPool::with_defaults().unwrap();
        let fixed = pool.stats().unwrap().used_bytes;
        let mut assembly = pool.try_begin(1).unwrap();
        let result = assembly.next_block_using(None, || {
            assert!(pool.shared.state.try_lock().is_ok());
            assert_eq!(pool.stats().unwrap().allocated_blocks, 1);
            pool.cancel()?;
            BlockAssembly::allocate()
        });
        assert!(matches!(result, Err(PoolError::Cancelled)));
        assert_eq!(pool.stats().unwrap().used_bytes, fixed);
    }
    #[test]
    fn poison_is_typed_and_destructors_do_not_panic() {
        let pool = BlockPool::with_defaults().unwrap();
        let mut assembly = pool.try_begin(1).unwrap();
        let block = assembly.try_next_block().unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = pool.lock().unwrap();
            panic!("poison probe");
        }));
        assert!(matches!(pool.try_begin(1), Err(PoolError::Poisoned)));
        assert!(matches!(pool.cancel(), Err(PoolError::Poisoned)));
        drop(block);
        drop(assembly);
    }
}
