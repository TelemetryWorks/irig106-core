//! L3-BUD/DEFAULT/HOT-008: bounded storage primitives, not reader admission.
use irig106_core::{BlockPool, PoolError, PoolStats, BLOCK_BYTES, DEFAULT_POOL_BUDGET};
use std::{alloc::{GlobalAlloc, Layout, System}, sync::{atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed}, Arc, Barrier, mpsc::sync_channel}, thread, time::{Duration, Instant}};
struct Counter;
static COUNT: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
// Instrumentation only; the library continues to forbid unsafe code.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT.load(Relaxed) { ALLOCS.fetch_add(1, Relaxed); }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout) } }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNT.load(Relaxed) { ALLOCS.fetch_add(1, Relaxed); }
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator] static A: Counter = Counter;
fn waiting(pool: &BlockPool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while pool.stats().unwrap().waiting == 0 {
        assert!(Instant::now() < deadline, "worker did not enter a bounded wait");
        thread::yield_now();
    }
}
fn burst(pool: &BlockPool) {
    let release = Arc::new(Barrier::new(5));
    let returned = Arc::new(Barrier::new(5));
    let finish = Arc::new(Barrier::new(5));
    thread::scope(|scope| {
        let mut inputs = Vec::new();
        for _ in 0..4 {
            let (tx, rx) = sync_channel::<irig106_core::PooledBlock>(4);
            inputs.push(tx);
            let (release, returned, finish) = (release.clone(), returned.clone(), finish.clone());
            scope.spawn(move || {
                for _ in 0..3 {
                    let blocks: [_; 4] = std::array::from_fn(|_| rx.recv().unwrap());
                    release.wait(); drop(blocks); returned.wait();
                }
                finish.wait();
            });
        }
        for round in 0..3 {
            if round == 1 { ALLOCS.store(0, Relaxed); COUNT.store(true, Relaxed); }
            for index in 0..16 {
                let mut assembly = pool.begin(1).unwrap();
                let mut block = assembly.next_block().unwrap();
                block.storage_mut()[0] = index as u8;
                inputs[index % 4].send(block).unwrap();
            }
            release.wait(); returned.wait();
            assert_eq!(pool.stats().unwrap().idle_blocks, 16);
        }
        COUNT.store(false, Relaxed);
        let allocations = ALLOCS.load(Relaxed);
        finish.wait();
        assert_eq!(allocations, 0, "ordinary burst reallocated after warm-up");
    });
}
// One test keeps global allocation instrumentation isolated from other tests.
#[test]
fn bounded_pool_ownership_pressure_cancellation_and_burst_hysteresis() {
    fn send<T: Send>() {} send::<BlockPool>(); send::<irig106_core::PooledBlock>();
    assert_eq!(BLOCK_BYTES, 524_288);
    let pool = BlockPool::with_defaults().unwrap();
    let cold: PoolStats = pool.stats().unwrap();
    assert_eq!(cold.budget_bytes, DEFAULT_POOL_BUDGET);
    assert_eq!(DEFAULT_POOL_BUDGET, 268_435_456);
    assert_eq!((cold.allocated_blocks, cold.idle_blocks, cold.block_allocations), (0, 0, 0));
    assert!(cold.used_bytes < BLOCK_BYTES); // Control metadata only, no payload pretouch.
    assert!(matches!(pool.begin(usize::MAX), Err(PoolError::BudgetTooSmall { .. })));
    assert!(matches!(pool.begin(0), Err(PoolError::InvalidRequest)));
    burst(&pool);
    assert_eq!(pool.stats().unwrap().block_allocations, 16);
    pool.trim_for_pressure().unwrap();
    assert_eq!(pool.stats().unwrap().idle_blocks, 8);
    let mut large = pool.begin(9).unwrap();
    let mut blocks = Vec::new();
    for _ in 0..9 { blocks.push(large.next_block().unwrap()); }
    assert!(matches!(large.next_block(), Err(PoolError::InvalidRequest)));
    drop(large); drop(blocks);
    assert_eq!(pool.stats().unwrap().idle_blocks, 8);
    let small = BlockPool::new(BLOCK_BYTES + 4096).unwrap();
    let mut first = small.begin(1).unwrap();
    let block = first.next_block().unwrap();
    assert_eq!(block.storage().len(), BLOCK_BYTES);
    assert!(matches!(small.begin(2), Err(PoolError::BudgetTooSmall { .. })));
    thread::scope(|scope| {
        let next = scope.spawn(|| small.begin(1));
        waiting(&small); // Exclusive assembly admission must wait.
        drop(first);
        let mut next = next.join().unwrap().unwrap();
        let checkout = scope.spawn(move || next.next_block());
        waiting(&small); // Worker owns the only block; budget cannot grow.
        assert!(small.stats().unwrap().used_bytes <= BLOCK_BYTES + 4096);
        drop(block);
        drop(checkout.join().unwrap().unwrap());
    });
    let mut held = small.begin(1).unwrap();
    let block = held.next_block().unwrap();
    drop(held);
    thread::scope(|scope| {
        let pending = scope.spawn(|| { let mut a = small.begin(1)?; a.next_block() });
        waiting(&small); small.cancel().unwrap();
        assert!(matches!(pending.join().unwrap(), Err(PoolError::Cancelled)));
    });
    drop(block);
    assert!(matches!(small.begin(1), Err(PoolError::Cancelled)));
    assert_eq!(small.stats().unwrap().allocated_blocks, 0);
}
