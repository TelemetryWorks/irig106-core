//! Explicit copy only, independent full-frame bytes and shared-credit oracles.
#[path = "support/watchdog.rs"]
mod support;
use irig106_core::*;
use std::{
    io::Cursor,
    time::{Duration, Instant},
};
fn until() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn fixture(p: usize) -> Vec<u8> {
    let mut bytes: Vec<u8> = (0..p).map(|i| (i % 251) as u8).collect();
    bytes[..24].fill(0);
    bytes[..2].copy_from_slice(&[0x25, 0xeb]);
    bytes[4..8].copy_from_slice(&(p as u32).to_le_bytes());
    let data = if p >= 44 { p - 43 } else { p - 24 };
    bytes[8..12].copy_from_slice(&(data as u32).to_le_bytes());
    bytes[14] = if p >= 44 { 0x83 } else { 0 };
    bytes[15] = 1;
    let sum = bytes[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    bytes[22..24].copy_from_slice(&sum.to_le_bytes());
    bytes
}
fn load(pool: &BlockPool) -> (PacketBuf, Vec<u8>) {
    let bytes = fixture(BLOCK_BYTES + 28);
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut reader =
        WholePacketReader::stream(Cursor::new(bytes.clone()), WholePolicy::new(), pool.clone())
            .unwrap();
    reader.next_packet_into(&mut buf, until()).unwrap();
    (buf, bytes)
}
#[test]
fn explicit_copy_preserves_bytes_metadata_and_drop_credits() {
    support::watchdog(Duration::from_secs(5), || {
        fn send<T: Send>() {}
        send::<CoalescedPacket>();
        let pool = BlockPool::with_defaults().unwrap();
        let empty = PacketBuf::new(pool.clone()).unwrap();
        assert!(matches!(
            empty.coalesce_until(until()),
            Err(PoolError::InvalidRequest)
        ));
        drop(empty);
        let (mut source, bytes) = load(&pool);
        assert!(source.contiguous().is_none());
        let before = pool.stats().unwrap().used_bytes;
        let copy = source.coalesce_until(until()).unwrap();
        assert_eq!(copy.frame(), bytes);
        assert_eq!(copy.data(), &bytes[36..bytes.len() - 7]);
        assert_eq!(copy.info().header, source.info().unwrap().header);
        assert_eq!(copy.info().ranges, source.info().unwrap().ranges);
        assert_eq!(
            copy.info().secondary_integrity,
            source.info().unwrap().secondary_integrity
        );
        let e = copy.evidence();
        assert_eq!(
            (e.offset, e.copied_bytes, e.source_blocks),
            (0, bytes.len(), 2)
        );
        assert!(e.charged_bytes >= e.copied_bytes + std::mem::size_of::<CoalescedPacket>());
        assert_eq!(pool.stats().unwrap().used_bytes, before + e.charged_bytes);
        assert_eq!(pool.stats().unwrap().coalesced_bytes, e.charged_bytes);
        source.clear(); // The copy owns its data, independent of input recycling.
        assert_eq!(copy.frame(), bytes);
        pool.cancel().unwrap();
        assert_eq!(pool.stats().unwrap().coalesced_bytes, e.charged_bytes);
        drop(copy);
        assert_eq!(pool.stats().unwrap().coalesced_bytes, 0);
    });
}
#[test]
fn impossible_contention_expiry_and_cancel_preserve_source() {
    support::watchdog(Duration::from_secs(5), || {
        let small = BlockPool::new(2 * BLOCK_BYTES + 131072).unwrap();
        let (buf, bytes) = load(&small);
        let before = small.stats().unwrap().used_bytes;
        assert!(matches!(
            buf.coalesce_until(until()),
            Err(PoolError::BudgetTooSmall { .. })
        ));
        assert_eq!(small.stats().unwrap().used_bytes, before);
        assert_eq!(
            buf.frame_spans().flatten().copied().collect::<Vec<_>>(),
            bytes
        );
        let pool = BlockPool::new(4 * BLOCK_BYTES + 131072).unwrap();
        let (buf, _) = load(&pool);
        let mut build = pool.try_begin(2).unwrap();
        // An active build alone is not contention: only shared byte credits matter.
        let concurrent = buf.coalesce_until(until()).unwrap();
        assert!(pool.stats().unwrap().used_bytes <= pool.stats().unwrap().budget_bytes);
        drop(concurrent);
        let held = [
            build.try_next_block().unwrap(),
            build.try_next_block().unwrap(),
        ];
        drop(build);
        assert!(matches!(
            buf.coalesce_until(until()),
            Err(PoolError::WouldBlock)
        ));
        drop(held); // Idle credits may be reclaimed by this explicit pressure operation.
        assert!(matches!(
            buf.coalesce_until(Instant::now()),
            Err(PoolError::DeadlineExceeded)
        ));
        let copy = buf.coalesce_until(until()).unwrap();
        drop(copy);
        pool.cancel().unwrap();
        assert!(matches!(
            buf.coalesce_until(until()),
            Err(PoolError::Cancelled)
        ));
        assert_eq!(pool.stats().unwrap().coalesced_bytes, 0);
        assert!(buf.info().is_some());
    });
}
#[test]
fn coalesced_credits_are_returnable_not_permanent_workspace() {
    support::watchdog(Duration::from_secs(10), || {
        use std::{sync::mpsc::sync_channel, thread};
        let pool = BlockPool::new(4 * BLOCK_BYTES + 131072).unwrap();
        let (mut source, _) = load(&pool);
        let copy = source.coalesce_until(until()).unwrap();
        source.clear();
        let cloned = pool.clone();
        let (send, receive) = sync_channel(1);
        let worker = thread::spawn(move || {
            let mut build = cloned.try_begin(4).unwrap(); // Feasible after copy returns.
            let mut blocks = Vec::new();
            for _ in 0..4 {
                blocks.push(build.next_block_until(until()).unwrap());
            }
            send.send(()).unwrap();
        });
        let bound = until();
        while pool.stats().unwrap().waiting == 0 {
            assert!(Instant::now() < bound);
            thread::yield_now();
        }
        assert!(pool.stats().unwrap().used_bytes <= pool.stats().unwrap().budget_bytes);
        drop(copy);
        receive.recv_timeout(Duration::from_secs(2)).unwrap();
        while !worker.is_finished() {
            assert!(Instant::now() < bound);
            thread::yield_now();
        }
        worker.join().unwrap();
        assert_eq!(pool.stats().unwrap().coalesced_bytes, 0);
    });
}

#[test]
fn exact_fit_counts_workspace_and_rejects_new_constructor() {
    support::watchdog(Duration::from_secs(10), || {
        let (mut low, mut high) = (2 * BLOCK_BYTES + 131072, 4 * BLOCK_BYTES + 131072);
        while low < high {
            let mid = low + (high - low) / 2;
            let pool = BlockPool::new(mid).unwrap();
            let (source, _) = load(&pool);
            match source.coalesce_until(until()) {
                Ok(_) => high = mid,
                Err(PoolError::BudgetTooSmall { .. }) => low = mid + 1,
                other => panic!("unexpected boundary result: {other:?}"),
            }
        }
        let exact = low;
        for budget in [exact, exact - 1] {
            let pool = BlockPool::new(budget).unwrap();
            let (source, _) = load(&pool);
            let result = source.coalesce_until(until());
            if budget == exact {
                let copy = result.unwrap();
                assert_eq!(pool.stats().unwrap().used_bytes, budget);
                let new_owner = PacketBuf::new(pool.clone());
                assert!(matches!(new_owner, Err(PoolError::WouldBlock)));
                assert_eq!(pool.stats().unwrap().used_bytes, budget);
                drop(copy);
            } else {
                assert!(
                    matches!(result, Err(PoolError::BudgetTooSmall {needed, limit, ..})
                    if needed == exact && limit == budget)
                );
            }
        }
    });
}

#[test]
fn refused_copy_keeps_warm_idle_blocks() {
    support::watchdog(Duration::from_secs(5), || {
        let budget = {
            let pool = BlockPool::new(5 * BLOCK_BYTES + 131072).unwrap();
            let (_source, _) = load(&pool);
            let mut build = pool.try_begin(3).unwrap();
            let _held: [_; 3] = std::array::from_fn(|_| build.try_next_block().unwrap());
            pool.stats().unwrap().used_bytes
        };
        let pool = BlockPool::new(budget).unwrap();
        let (source, _) = load(&pool);
        let mut build = pool.try_begin(3).unwrap();
        let mut held: Vec<_> = (0..3).map(|_| build.try_next_block().unwrap()).collect();
        drop(build);
        drop(held.pop());
        let before = pool.stats().unwrap();
        assert_eq!((before.allocated_blocks, before.idle_blocks), (5, 1));
        assert!(matches!(
            source.coalesce_until(until()),
            Err(PoolError::WouldBlock)
        ));
        let after = pool.stats().unwrap();
        assert_eq!(
            (
                after.used_bytes,
                after.allocated_blocks,
                after.idle_blocks,
                after.coalesced_bytes
            ),
            (
                before.used_bytes,
                before.allocated_blocks,
                before.idle_blocks,
                before.coalesced_bytes
            )
        );
    });
}

#[test]
fn varied_spans_and_single_block_copy_are_exact() {
    support::watchdog(Duration::from_secs(10), || {
        for p in [
            28,
            BLOCK_BYTES,
            BLOCK_BYTES + 4,
            BLOCK_BYTES + 36,
            2 * BLOCK_BYTES - 4,
            2 * BLOCK_BYTES,
            3 * BLOCK_BYTES + 12,
            3_221_440,
        ] {
            let pool = BlockPool::with_defaults().unwrap();
            let expected = fixture(p);
            let mut source = PacketBuf::new(pool.clone()).unwrap();
            let mut reader = WholePacketReader::stream(
                Cursor::new(expected.clone()),
                WholePolicy::new(),
                pool.clone(),
            )
            .unwrap();
            reader.next_packet_into(&mut source, until()).unwrap();
            let copy = source.coalesce_until(until()).unwrap();
            assert_eq!(copy.frame(), expected);
            let range = source.info().unwrap().ranges.data.clone();
            assert_eq!(
                copy.data(),
                &expected[range.start as usize..range.end as usize]
            );
            assert_eq!(copy.evidence().source_blocks, p.div_ceil(BLOCK_BYTES));
            if p <= BLOCK_BYTES {
                assert_ne!(copy.frame().as_ptr(), source.contiguous().unwrap().as_ptr());
            }
        }
    });
}

#[test]
fn worker_copies_while_ordered_reader_assembles_with_shared_credits() {
    support::watchdog(Duration::from_secs(15), || {
        use std::{
            io::{self, Read},
            sync::mpsc::{Receiver, SyncSender, sync_channel},
            thread,
        };
        struct Gated {
            bytes: Cursor<Vec<u8>>,
            at: u64,
            paused: bool,
            entered: SyncSender<()>,
            resume: Receiver<()>,
        }
        impl Read for Gated {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if out.is_empty() {
                    return Ok(0);
                }
                let position = self.bytes.position();
                if position == self.at && !self.paused {
                    self.paused = true;
                    self.entered.send(()).map_err(io::Error::other)?;
                    self.resume
                        .recv_timeout(Duration::from_secs(3))
                        .map_err(io::Error::other)?;
                }
                let n = if position < self.at {
                    out.len().min((self.at - position) as usize)
                } else {
                    out.len()
                };
                self.bytes.read(&mut out[..n])
            }
        }
        let pool = BlockPool::new(5 * BLOCK_BYTES + 131072).unwrap();
        let first = fixture(BLOCK_BYTES + 28);
        let second = fixture(2 * BLOCK_BYTES + 28);
        let (entered, reached) = sync_channel(1);
        let (resume, proceed) = sync_channel(1);
        let input = Gated {
            bytes: Cursor::new([first.clone(), second.clone()].concat()),
            at: (first.len() + BLOCK_BYTES) as u64,
            paused: false,
            entered,
            resume: proceed,
        };
        let mut reader =
            WholePacketReader::stream(input, WholePolicy::new(), pool.clone()).unwrap();
        let mut source = PacketBuf::new(pool.clone()).unwrap();
        reader.next_packet_into(&mut source, until()).unwrap();
        let mut next = PacketBuf::new(pool.clone()).unwrap();
        let (done, result) = sync_channel(1);
        let producer = thread::spawn(move || {
            reader.next_packet_into(&mut next, until()).unwrap();
            done.send(next).unwrap();
        });
        reached.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(pool.try_begin(1), Err(PoolError::WouldBlock))); // Real assembly is active.
        let paused = pool.stats().unwrap();
        assert_eq!(paused.allocated_blocks, 4);
        assert!(paused.used_bytes <= paused.budget_bytes);
        let copy = source.coalesce_until(until()).unwrap();
        assert_eq!(copy.frame(), first);
        let check = || {
            let s = pool.stats().unwrap();
            assert!(s.used_bytes <= s.budget_bytes);
            assert_eq!(s.coalesced_bytes, copy.evidence().charged_bytes);
        };
        check();
        resume.send(()).unwrap();
        let bound = until();
        while pool.stats().unwrap().waiting == 0 {
            assert!(Instant::now() < bound);
            thread::yield_now();
        }
        check(); // Reader cannot grow beyond B with both the old input and copy held.
        assert_eq!(pool.stats().unwrap().allocated_blocks, 4);
        source.clear(); // Worker releases input BEFORE any output-merger wait.
        let next = result.recv_timeout(Duration::from_secs(2)).unwrap();
        check();
        assert_eq!(next.info().unwrap().offset, first.len() as u64);
        assert!(
            next.frame_spans()
                .flatten()
                .copied()
                .eq(second.iter().copied())
        );
        while !producer.is_finished() {
            assert!(Instant::now() < bound);
            thread::yield_now();
        }
        producer.join().unwrap();
        drop(copy);
        assert_eq!(pool.stats().unwrap().coalesced_bytes, 0);
        assert!(pool.stats().unwrap().used_bytes <= pool.stats().unwrap().budget_bytes);
    });
}
