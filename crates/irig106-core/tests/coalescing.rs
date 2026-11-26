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
fn load(pool: &BlockPool) -> (PacketBuf, Vec<u8>) {
    let p = BLOCK_BYTES + 28;
    let mut bytes = vec![0x5a; p];
    bytes[..24].fill(0);
    bytes[..2].copy_from_slice(&[0x25, 0xeb]);
    bytes[4..8].copy_from_slice(&(p as u32).to_le_bytes());
    bytes[8..12].copy_from_slice(&((p - 43) as u32).to_le_bytes());
    bytes[14] = 0x83; // Secondary header, DATA of odd length, filler3, checksum4.
    bytes[15] = 1; // Standard setup allowance, no stream trust option.
    let sum = bytes[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    bytes[22..24].copy_from_slice(&sum.to_le_bytes());
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
        // Never wait for an active build while retaining its possible input credits.
        assert!(matches!(
            buf.coalesce_until(until()),
            Err(PoolError::WouldBlock)
        ));
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
