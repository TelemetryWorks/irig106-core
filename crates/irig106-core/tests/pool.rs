//! L3-BUD/DEFAULT/HOT-008: bounded storage primitives, not reader admission.
use irig106_core::{BLOCK_BYTES, BlockPool, DEFAULT_POOL_BUDGET, PoolError, PoolStats};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
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
fn burst(pool: &BlockPool) {
    let warm_admission = pool.begin(1).unwrap();
    let warm_ready = Arc::new(Barrier::new(5));
    let slots = Arc::new(Mutex::new(std::array::from_fn::<_, 16, _>(|_| None)));
    let start = Arc::new(Barrier::new(5));
    let release = Arc::new(Barrier::new(5));
    let returned = Arc::new(Barrier::new(5));
    let finish = Arc::new(Barrier::new(5));
    thread::scope(|scope| {
        for worker in 0..4 {
            let (slots, start, release, returned, finish) = (
                slots.clone(),
                start.clone(),
                release.clone(),
                returned.clone(),
                finish.clone(),
            );
            let warm_ready = warm_ready.clone();
            scope.spawn(move || {
                drop(pool.begin(1).unwrap());
                warm_ready.wait();
                for _ in 0..3 {
                    start.wait();
                    let blocks: [irig106_core::PooledBlock; 4] = {
                        let mut slots = slots.lock().unwrap();
                        std::array::from_fn(|i| slots[4 * worker + i].take().unwrap())
                    };
                    release.wait();
                    measured(|| drop(blocks));
                    returned.wait();
                }
                finish.wait();
            });
        }
        waiting(pool, 4);
        drop(warm_admission);
        warm_ready.wait();
        for round in 0..3 {
            if round == 1 {
                TOTAL.store(0, Relaxed);
                ALLOCS.store(0, Relaxed);
                COUNT.store(true, Relaxed);
            }
            {
                let mut slots = slots.lock().unwrap();
                for index in 0..16 {
                    measured(|| {
                        let mut assembly = pool.begin(1).unwrap();
                        let mut block = assembly.next_block().unwrap();
                        block.storage_mut()[0] = index as u8;
                        slots[index] = Some(block);
                    });
                }
            }
            start.wait();
            release.wait();
            returned.wait();
        }
        COUNT.store(false, Relaxed);
        let allocations = ALLOCS.load(Relaxed);
        finish.wait();
        assert_eq!(
            allocations,
            0,
            "pool reallocated after warm-up; process/harness allocations: {}",
            TOTAL.load(Relaxed)
        );
        assert_eq!(pool.stats().unwrap().idle_blocks, 16);
    });
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
        pool.begin(usize::MAX),
        Err(PoolError::BudgetTooSmall { .. })
    ));
    assert!(matches!(pool.begin(0), Err(PoolError::InvalidRequest)));
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
    let mut large = pool.begin(9).unwrap();
    let mut blocks = Vec::new();
    for _ in 0..9 {
        blocks.push(large.next_block().unwrap());
    }
    assert!(matches!(large.next_block(), Err(PoolError::InvalidRequest)));
    drop(large);
    drop(blocks);
    assert_eq!(pool.stats().unwrap().idle_blocks, 9);
    pool.trim_after_return(9).unwrap();
    assert_eq!(pool.stats().unwrap().idle_blocks, 8);
    let small = BlockPool::new(BLOCK_BYTES + 4096).unwrap();
    let mut first = small.begin(1).unwrap();
    let block = first.next_block().unwrap();
    assert_eq!(block.storage().len(), BLOCK_BYTES);
    assert!(matches!(
        small.begin(2),
        Err(PoolError::BudgetTooSmall { .. })
    ));
    thread::scope(|scope| {
        let next = scope.spawn(|| small.begin(1));
        waiting(&small, 1); // Exclusive assembly admission must wait.
        drop(first);
        let mut next = next.join().unwrap().unwrap();
        let checkout = scope.spawn(move || next.next_block());
        waiting(&small, 1); // Worker owns the only block; budget cannot grow.
        assert!(small.stats().unwrap().used_bytes <= BLOCK_BYTES + 4096);
        drop(block);
        drop(checkout.join().unwrap().unwrap());
    });
    let admission = BlockPool::with_defaults().unwrap();
    let owner = admission.begin(1).unwrap();
    thread::scope(|scope| {
        let pending = scope.spawn(|| admission.begin(1));
        waiting(&admission, 1);
        admission.cancel().unwrap();
        assert!(matches!(pending.join().unwrap(), Err(PoolError::Cancelled)));
    });
    drop(owner);
    let mut held = small.begin(1).unwrap();
    let block = held.next_block().unwrap();
    drop(held);
    thread::scope(|scope| {
        let pending = scope.spawn(|| {
            let mut a = small.begin(1)?;
            a.next_block()
        });
        waiting(&small, 1);
        small.cancel().unwrap();
        assert!(matches!(pending.join().unwrap(), Err(PoolError::Cancelled)));
    });
    drop(block);
    assert!(matches!(small.begin(1), Err(PoolError::Cancelled)));
    assert_eq!(small.stats().unwrap().allocated_blocks, 0);
}
