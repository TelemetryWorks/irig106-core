//! L3-BUD/DEFAULT/HOT-008: bounded storage primitives, not reader admission.
use irig106_core::{BLOCK_BYTES, BlockPool, DEFAULT_POOL_BUDGET, PoolError, PoolStats};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
        mpsc::sync_channel,
    },
    thread,
    time::{Duration, Instant},
};
struct Counter;
static COUNT: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
thread_local! { static IN_POOL: Cell<bool> = const { Cell::new(false) }; }
fn measured<T>(operation: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            IN_POOL.with(|flag| flag.set(false));
        }
    }
    IN_POOL.with(|flag| flag.set(true));
    let _reset = Reset;
    operation()
}
fn allocation() {
    if COUNT.load(Relaxed) {
        TOTAL.fetch_add(1, Relaxed);
        if IN_POOL.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.fetch_add(1, Relaxed);
        }
    }
}
// Instrumentation only; the library continues to forbid unsafe code.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static A: Counter = Counter;
fn waiting(pool: &BlockPool, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while pool.stats().unwrap().waiting < count {
        assert!(
            Instant::now() < deadline,
            "worker did not enter a bounded wait"
        );
        thread::yield_now();
    }
}
fn joined<T>(handle: thread::JoinHandle<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !handle.is_finished() {
        assert!(Instant::now() < deadline, "worker did not complete in time");
        thread::yield_now();
    }
    handle.join().unwrap()
}
fn burst(pool: &BlockPool) {
    let warm_admission = pool.try_begin(1).unwrap();
    let (done, completed) = sync_channel(4);
    let (mut inputs, mut releases, mut workers) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..4 {
        let (tx, rx) = sync_channel::<irig106_core::PooledBlock>(4);
        let (release, go) = sync_channel(1);
        inputs.push(tx);
        releases.push(release);
        let (pool, done) = (pool.clone(), done.clone());
        workers.push(thread::spawn(move || {
            drop(
                pool.begin_until(1, Instant::now() + Duration::from_secs(5))
                    .unwrap(),
            );
            done.send(()).unwrap();
            for _ in 0..3 {
                let blocks: [_; 4] =
                    std::array::from_fn(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap());
                go.recv_timeout(Duration::from_secs(5)).unwrap();
                measured(|| drop(blocks));
                done.send(()).unwrap();
            }
            go.recv_timeout(Duration::from_secs(5)).unwrap();
        }));
    }
    waiting(pool, 4);
    drop(warm_admission);
    for _ in 0..4 {
        completed.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    for round in 0..3 {
        if round == 1 {
            TOTAL.store(0, Relaxed);
            ALLOCS.store(0, Relaxed);
            COUNT.store(true, Relaxed);
        }
        for index in 0..16 {
            let block = measured(|| {
                let mut assembly = pool.try_begin(1).unwrap();
                let mut block = assembly.try_next_block().unwrap();
                block.storage_mut()[0] = index as u8;
                block
            });
            inputs[index % 4].send(block).unwrap();
        }
        for release in &releases {
            release.send(()).unwrap();
        }
        for _ in 0..4 {
            completed.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
    COUNT.store(false, Relaxed);
    let allocations = ALLOCS.load(Relaxed);
    for release in releases {
        release.send(()).unwrap();
    }
    for worker in workers {
        joined(worker);
    }
    assert_eq!(
        allocations,
        0,
        "pool reallocated after warm-up; process/harness allocations: {}",
        TOTAL.load(Relaxed)
    );
    assert_eq!(pool.stats().unwrap().idle_blocks, 16);
}
// One test keeps global allocation instrumentation isolated from other tests.
#[test]
fn bounded_pool_ownership_pressure_cancellation_and_burst_hysteresis() {
    fn send<T: Send>() {}
    send::<BlockPool>();
    send::<irig106_core::PooledBlock>();
    assert_eq!(BLOCK_BYTES, 524_288);
    assert!(matches!(
        BlockPool::new(0),
        Err(PoolError::BudgetTooSmall { .. })
    ));
    assert!(matches!(
        BlockPool::new(usize::MAX),
        Err(PoolError::InvalidRequest)
    ));
    let pool = BlockPool::with_defaults().unwrap();
    let cold: PoolStats = pool.stats().unwrap();
    assert_eq!(cold.budget_bytes, DEFAULT_POOL_BUDGET);
    assert_eq!(DEFAULT_POOL_BUDGET, 268_435_456);
    assert_eq!(
        (
            cold.allocated_blocks,
            cold.idle_blocks,
            cold.block_allocations
        ),
        (0, 0, 0)
    );
    assert!(cold.used_bytes < BLOCK_BYTES); // Control metadata only, no payload pretouch.
    assert!(matches!(
        pool.try_begin(usize::MAX),
        Err(PoolError::BudgetTooSmall { .. })
    ));
    assert!(matches!(pool.try_begin(0), Err(PoolError::InvalidRequest)));
    let capacity = pool.try_begin(511).unwrap();
    assert!(matches!(
        pool.try_begin(512),
        Err(PoolError::BudgetTooSmall { .. })
    ));
    drop(capacity);
    burst(&pool);
    assert_eq!(pool.stats().unwrap().block_allocations, 16);
    let retained = pool.stats().unwrap();
    assert_eq!(retained.allocated_blocks, retained.idle_blocks);
    assert!(retained.used_bytes >= 16 * BLOCK_BYTES);
    assert!(retained.used_bytes <= DEFAULT_POOL_BUDGET);
    pool.trim_after_return(1).unwrap();
    assert_eq!(pool.stats().unwrap().idle_blocks, 16);
    pool.trim_for_pressure().unwrap();
    assert_eq!(pool.stats().unwrap().idle_blocks, 8);
    let mut large = pool.try_begin(9).unwrap();
    let mut blocks = Vec::new();
    for _ in 0..9 {
        blocks.push(large.try_next_block().unwrap());
    }
    assert!(matches!(
        large.try_next_block(),
        Err(PoolError::InvalidRequest)
    ));
    drop(large);
    drop(blocks);
    assert_eq!(pool.stats().unwrap().idle_blocks, 9);
    pool.trim_after_return(9).unwrap();
    assert_eq!(pool.stats().unwrap().idle_blocks, 8);
    let small = BlockPool::new(BLOCK_BYTES + 4096).unwrap();
    let mut first = small.try_begin(1).unwrap();
    let block = first.try_next_block().unwrap();
    assert_eq!(block.storage().len(), BLOCK_BYTES);
    assert!(matches!(
        small.try_begin(2),
        Err(PoolError::BudgetTooSmall { .. })
    ));
    assert!(matches!(small.try_begin(1), Err(PoolError::WouldBlock)));
    let next_pool = small.clone();
    let next =
        thread::spawn(move || next_pool.begin_until(1, Instant::now() + Duration::from_secs(30)));
    waiting(&small, 1);
    drop(first);
    let mut next = joined(next).unwrap();
    // Explicit nonblocking policy, without inferring who owns the earlier block.
    assert!(matches!(next.try_next_block(), Err(PoolError::WouldBlock)));
    assert_eq!(small.stats().unwrap().waiting, 0);
    let checkout =
        thread::spawn(move || next.next_block_until(Instant::now() + Duration::from_secs(30)));
    waiting(&small, 1);
    assert!(small.stats().unwrap().used_bytes <= BLOCK_BYTES + 4096);
    drop(block);
    drop(joined(checkout).unwrap());
    let admission = BlockPool::with_defaults().unwrap();
    let owner = admission.try_begin(1).unwrap();
    assert!(matches!(
        admission.begin_until(1, Instant::now()),
        Err(PoolError::DeadlineExceeded)
    ));
    let p = admission.clone();
    let pending = thread::spawn(move || p.begin_until(1, Instant::now() + Duration::from_secs(30)));
    waiting(&admission, 1);
    admission.cancel().unwrap();
    assert!(matches!(joined(pending), Err(PoolError::Cancelled)));
    drop(owner);
    let mut held = small.try_begin(1).unwrap();
    let block = held.try_next_block().unwrap();
    drop(held);
    let p = small.clone();
    let pending = thread::spawn(move || {
        let mut a = p.try_begin(1)?;
        a.next_block_until(Instant::now() + Duration::from_secs(30))
    });
    waiting(&small, 1);
    small.cancel().unwrap();
    assert!(matches!(joined(pending), Err(PoolError::Cancelled)));
    drop(block);
    assert!(matches!(small.try_begin(1), Err(PoolError::Cancelled)));
    assert_eq!(small.stats().unwrap().allocated_blocks, 0);
    // Cancel must clear idle storage as well as outstanding-return handling.
    assert_eq!(pool.stats().unwrap().idle_blocks, 8);
    pool.cancel().unwrap();
    let cleared = pool.stats().unwrap();
    assert_eq!((cleared.idle_blocks, cleared.allocated_blocks), (0, 0));
    assert_eq!(cleared.used_bytes, cold.used_bytes);
}
