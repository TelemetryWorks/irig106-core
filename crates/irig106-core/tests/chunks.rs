//! Explicit provisional chunks; independent sums and staged consumer oracles.
use irig106_core::*;
use std::{
    io::{self, Cursor, Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
#[path = "support/watchdog.rs"]
mod support;
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn fixture(p: usize, width: usize, secondary: bool) -> (Vec<u8>, u32) {
    let h = if secondary { 36 } else { 24 };
    let data = (p - h - width - 4) | 1;
    let mut b: Vec<_> = (0..p).map(|i| ((i * 37 + 19) % 256) as u8).collect();
    b[..24].fill(0);
    b[..2].copy_from_slice(&[0x25, 0xeb]);
    b[4..8].copy_from_slice(&(p as u32).to_le_bytes());
    b[8..12].copy_from_slice(&(data as u32).to_le_bytes());
    b[14] = if secondary { 0x80 } else { 0 }
        | match width {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 3,
        };
    b[15] = 1;
    b[h + data..p - width].fill(0xff);
    let sum: u64 = if width == 0 {
        0
    } else {
        b[h..p - width]
            .iter()
            .enumerate()
            .map(|(i, &x)| u64::from(x) << (8 * (i % width)))
            .sum()
    };
    b[p - width..].copy_from_slice(&(sum as u32).to_le_bytes()[..width]);
    let primary: u32 = b[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    b[22..24].copy_from_slice(&(primary as u16).to_le_bytes());
    (
        b,
        (sum & if width == 0 {
            0
        } else {
            u64::MAX >> (64 - width * 8)
        }) as u32,
    )
}
struct Short {
    input: Cursor<Vec<u8>>,
    interrupt: bool,
}
impl Read for Short {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.interrupt = !self.interrupt;
        if self.interrupt {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = out.len().min(3);
        self.input.read(&mut out[..n])
    }
}
#[test]
fn ranges_bytes_checksums_and_commit_are_exact() {
    support::watchdog(Duration::from_secs(10), || {
        for width in [0, 1, 2, 4] {
            for secondary in [false, true] {
                let (bytes, sum) = fixture(2 * 4096 + 44, width, secondary);
                let pool = BlockPool::with_defaults().unwrap();
                let mut r = WholePacketReader::stream(
                    Short {
                        input: Cursor::new(bytes.clone()),
                        interrupt: false,
                    },
                    WholePolicy::new(),
                    pool.clone(),
                )
                .unwrap();
                let mut staged = Vec::new();
                let mut data = Vec::new();
                let mut begins = 0;
                let mut aborts = 0;
                let outcome = r
                    .next_chunks(deadline(), |event| {
                        match event {
                            ChunkEvent::Begin(start) => {
                                begins += 1;
                                assert_eq!(start.chunk_bytes, CHUNK_BYTES);
                                assert_eq!(start.offset, 0);
                                assert_eq!(start.hard_limit, DEFAULT_HARD_LIMIT);
                            }
                            ChunkEvent::Chunk(part) => {
                                assert_eq!(part.range.start, staged.len() as u64);
                                assert!(part.bytes.len() <= CHUNK_BYTES);
                                assert_eq!(
                                    part.range.end - part.range.start,
                                    part.bytes.len() as u64
                                );
                                assert_eq!(
                                    part.bytes,
                                    &bytes[part.range.start as usize..part.range.end as usize]
                                );
                                assert_eq!(
                                    part.data,
                                    &bytes[part.data_range.start as usize
                                        ..part.data_range.end as usize]
                                );
                                staged.extend_from_slice(part.bytes);
                                data.extend_from_slice(part.data);
                            }
                            ChunkEvent::Abort(_) => aborts += 1,
                            _ => panic!("unexpected chunk event"),
                        };
                        Ok::<(), io::Error>(())
                    })
                    .unwrap();
                let Some(ChunkOutcome::End(end)) = outcome else {
                    panic!("end")
                };
                assert_eq!((begins, aborts), (1, 0));
                assert_eq!(staged, bytes);
                assert_eq!(
                    data,
                    &bytes[end.ranges.data.start as usize..end.ranges.data.end as usize]
                );
                assert_eq!(
                    (end.frame_bytes, end.delivered_bytes, end.data_bytes),
                    (bytes.len() as u64, bytes.len() as u64, data.len() as u64)
                );
                if width == 0 {
                    assert_eq!(end.ranges.data_checksum, DataChecksumState::Absent);
                } else {
                    assert!(
                        matches!(end.ranges.data_checksum,DataChecksumState::Verified{width:w,value,..}if w==width as u8&&value==sum)
                    );
                }
                assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
                assert!(
                    r.next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
                        .unwrap()
                        .is_none()
                );
            }
        }
    });
}
struct Stable {
    input: Cursor<Vec<u8>>,
    revision: Arc<AtomicU64>,
}
impl Read for Stable {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.input.read(b)
    }
}
impl Seek for Stable {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.input.seek(p)
    }
}
impl StableSource for Stable {
    fn source_stamp(&mut self) -> io::Result<SourceStamp> {
        Ok(SourceStamp::new(
            self.input.get_ref().len() as u64,
            1,
            self.revision.load(Ordering::Relaxed) as u128,
        ))
    }
}
#[test]
fn explicit_chunk_mode_handles_whole_budget_refusal_and_pending_probe() {
    support::watchdog(Duration::from_secs(10), || {
        let (mut first, _) = fixture(BLOCK_BYTES + 4, 4, false); // Setup allowance, then force ordinary type to need corroboration.
        first[15] = 9;
        let sum: u32 = first[..22]
            .chunks_exact(2)
            .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
            .sum();
        first[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
        let (next, _) = fixture(48, 0, false);
        let bytes = [first.clone(), next].concat();
        let pool = BlockPool::new(BLOCK_BYTES + 131072).unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut r = WholePacketReader::seek_checked(
            Stable {
                input: Cursor::new(bytes),
                revision: Arc::new(AtomicU64::new(0)),
            },
            WholePolicy::new(),
            pool.clone(),
        )
        .unwrap();
        assert!(matches!(
            r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
            ErrorKind::Pool {
                source: PoolError::BudgetTooSmall { .. },
                ..
            }
        ));
        let mut at = 0;
        let end=r.next_chunks(deadline(),|e|{match e{ChunkEvent::Begin(s)=>assert!(matches!(s.admission.authorization,Authorization::NextHeader{offset,..}if offset==first.len()as u64)),ChunkEvent::Chunk(p)=>{assert_eq!(p.bytes,&first[at..at+p.bytes.len()]);at+=p.bytes.len();},_=>{}}Ok::<(),io::Error>(())}).unwrap();
        assert!(matches!(end, Some(ChunkOutcome::End(_))));
        assert_eq!(at, first.len());
        assert!(r.refused_header().is_none());
        assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
        r.next_packet_into(&mut buf, deadline()).unwrap();
        assert_eq!(buf.info().unwrap().offset, first.len() as u64);
    });
}
#[test]
fn truncation_rolls_back_and_no_successful_end_exists() {
    let (mut bytes, _) = fixture(64, 4, false);
    bytes.truncate(29);
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool).unwrap();
    let mut staged = 0;
    let mut aborted = None;
    let e = r
        .next_chunks(deadline(), |event| {
            match event {
                ChunkEvent::Chunk(p) => staged += p.data.len(),
                ChunkEvent::Abort(a) => {
                    aborted = Some(a.clone());
                    staged = 0;
                }
                _ => {}
            }
            Ok::<(), io::Error>(())
        })
        .unwrap_err();
    assert_eq!(staged, 0);
    let a = aborted.unwrap();
    assert_eq!(a.kind, ChunkAbortKind::Truncated);
    assert_eq!(a.received_bytes, 29);
    assert_eq!(a.delivered_bytes, 29);
    assert!(matches!(
        e.failure,
        ChunkFailure::Core {
            error: Error {
                reason: ErrorKind::NoCompletePacket { .. },
                ..
            },
            ..
        }
    ));
    assert!(
        r.next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
            .unwrap()
            .is_none()
    );
}
#[test]
fn sink_failure_preserves_primary_and_abort_notification_failure() {
    let (bytes, _) = fixture(48, 4, false);
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool).unwrap();
    let e: ChunkError<io::Error> = r
        .next_chunks(deadline(), |e| match e {
            ChunkEvent::Chunk(_) => Err(io::ErrorKind::PermissionDenied.into()),
            ChunkEvent::Abort(_) => Err(io::ErrorKind::BrokenPipe.into()),
            _ => Ok(()),
        })
        .unwrap_err();
    assert!(
        matches!(e.failure,ChunkFailure::Sink{error,..}if error.kind()==io::ErrorKind::PermissionDenied)
    );
    assert_eq!(
        e.abort_sink_error.unwrap().kind(),
        io::ErrorKind::BrokenPipe
    );
    let a = e.abort.unwrap();
    assert_eq!(
        (a.kind, a.received_bytes, a.offered_bytes, a.delivered_bytes),
        (ChunkAbortKind::Sink, 24, 24, 0)
    );
}
#[test]
fn cancellation_and_source_change_after_callbacks_abort_before_end() {
    for mutate in [false, true] {
        let (bytes, _) = fixture(48, 4, false);
        let pool = BlockPool::with_defaults().unwrap();
        let revision = Arc::new(AtomicU64::new(0));
        let mut r = WholePacketReader::seek_checked(
            Stable {
                input: Cursor::new(bytes),
                revision: revision.clone(),
            },
            WholePolicy::new(),
            pool.clone(),
        )
        .unwrap();
        let mut aborts = 0;
        let e = r
            .next_chunks(deadline(), |e| {
                match e {
                    ChunkEvent::Chunk(p) if p.range.start > 0 => {
                        if mutate {
                            revision.store(1, Ordering::Relaxed);
                        } else {
                            pool.cancel().unwrap();
                        }
                    }
                    ChunkEvent::Abort(_) => aborts += 1,
                    _ => {}
                }
                Ok::<(), io::Error>(())
            })
            .unwrap_err();
        assert_eq!(aborts, 1);
        assert_eq!(
            e.abort.unwrap().kind,
            if mutate {
                ChunkAbortKind::SourceChanged
            } else {
                ChunkAbortKind::Cancelled
            }
        );
    }
}

#[test]
fn mismatch_is_end_evidence_and_opt_out_stays_not_checked() {
    for verify in [true, false] {
        let (mut bytes, stored) = fixture(48, 4, false);
        bytes[24] ^= 1;
        let computed = bytes[24..44]
            .iter()
            .enumerate()
            .fold(0u64, |s, (i, &b)| s + (u64::from(b) << (8 * (i % 4))))
            as u32;
        let pool = BlockPool::with_defaults().unwrap();
        let mut r = WholePacketReader::stream(
            Cursor::new(bytes),
            WholePolicy::new().with_data_checksum_verification(verify),
            pool,
        )
        .unwrap();
        let Some(ChunkOutcome::End(end)) = r
            .next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
            .unwrap()
        else {
            panic!("end")
        };
        if verify {
            assert!(
                matches!(end.ranges.data_checksum,DataChecksumState::Mismatch{stored:s,computed:c,..}if s==stored&&c==computed)
            );
        } else {
            assert_eq!(end.ranges.data_checksum, DataChecksumState::NotChecked);
        }
    }
}
#[test]
fn known_short_path_never_begins_and_other_admission_is_explicit() {
    let (mut bytes, _) = fixture(64, 4, false);
    bytes.truncate(29);
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::seek_checked(
        Stable {
            input: Cursor::new(bytes),
            revision: Arc::new(AtomicU64::new(0)),
        },
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let e = r
        .next_chunks(deadline(), |_| -> Result<(), io::Error> {
            panic!("known-short must not Begin")
        })
        .unwrap_err();
    assert!(e.abort.is_none());
    assert!(
        matches!(e.failure,ChunkFailure::Core{error:Error{reason:ErrorKind::NoCompletePacket{tail:Some(t),..},..},..}if t.bytes==29)
    );
    let (bytes, _) = fixture(BLOCK_BYTES + 4, 0, false);
    let mut ordinary = bytes.clone();
    ordinary[15] = 9;
    let sum: u32 = ordinary[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    ordinary[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    let mut r = WholePacketReader::stream(
        Cursor::new(ordinary.clone()),
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let e = r
        .next_chunks(deadline(), |_| -> Result<(), io::Error> {
            panic!("no authorization")
        })
        .unwrap_err();
    assert!(matches!(
        e.failure,
        ChunkFailure::Core {
            error: Error {
                reason: ErrorKind::Admission {
                    source: AdmissionError::Uncorroborated,
                    ..
                },
                ..
            },
            ..
        }
    ));
    assert!(e.abort.is_none());
    let mut r = WholePacketReader::stream(
        Cursor::new(ordinary.clone()),
        WholePolicy::new().with_stream_opt_in(true),
        pool.clone(),
    )
    .unwrap();
    let Some(ChunkOutcome::End(end)) = r
        .next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
        .unwrap()
    else {
        panic!("end")
    };
    assert_eq!(end.admission.authorization, Authorization::StreamOptIn);
    let mut r = WholePacketReader::seek_checked(
        Stable {
            input: Cursor::new(ordinary),
            revision: Arc::new(AtomicU64::new(0)),
        },
        WholePolicy::new(),
        pool,
    )
    .unwrap();
    let Some(ChunkOutcome::End(end)) = r
        .next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
        .unwrap()
    else {
        panic!("end")
    };
    assert_eq!(
        end.admission.authorization,
        Authorization::Unconfirmed(UnconfirmedReason::ExactEof)
    );
}
#[test]
fn real_io_failure_keeps_received_but_unoffered_progress() {
    struct Fault {
        bytes: Cursor<Vec<u8>>,
    }
    impl Read for Fault {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let pos = self.bytes.position();
            if pos >= 31 {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            let n = out.len().min((31 - pos) as usize);
            self.bytes.read(&mut out[..n])
        }
    }
    let (bytes, _) = fixture(64, 4, false);
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(
        Fault {
            bytes: Cursor::new(bytes),
        },
        WholePolicy::new().with_read_ahead_bytes(24),
        pool,
    )
    .unwrap();
    let e = r
        .next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
        .unwrap_err();
    assert!(
        matches!(e.failure,ChunkFailure::Core{error,..}if error.kind()==io::ErrorKind::PermissionDenied)
    );
    let a = e.abort.unwrap();
    assert_eq!(
        (a.kind, a.received_bytes, a.offered_bytes, a.delivered_bytes),
        (ChunkAbortKind::Input, 31, 24, 24)
    );
}
#[test]
fn panic_fails_closed_and_empty_input_has_no_begin() {
    let pool = BlockPool::with_defaults().unwrap();
    let (bytes, _) = fixture(48, 0, false);
    let mut r =
        WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool.clone()).unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = r.next_chunks(deadline(), |e| -> Result<(), io::Error> {
            if matches!(e, ChunkEvent::Chunk(_)) {
                panic!("sink panic");
            }
            Ok(())
        });
    }));
    assert!(panic.is_err());
    assert!(
        r.next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
            .unwrap()
            .is_none()
    );
    let mut r = WholePacketReader::stream(&b""[..], WholePolicy::new(), pool).unwrap();
    let e = r
        .next_chunks(deadline(), |_| -> Result<(), io::Error> {
            panic!("empty input")
        })
        .unwrap_err();
    assert!(matches!(
        e.failure,
        ChunkFailure::Core {
            error: Error {
                reason: ErrorKind::NoCompletePacket { tail: None, .. },
                ..
            },
            ..
        }
    ));
}
#[test]
fn recovery_is_not_payload_and_hard_refusal_stays_positioned() {
    let (first, _) = fixture(48, 0, false);
    let input = [vec![0xff; 7], first.clone(), vec![0xff; 25]].concat();
    let pool = BlockPool::with_defaults().unwrap();
    let mut r =
        WholePacketReader::stream(Cursor::new(input), WholePolicy::new(), pool.clone()).unwrap();
    let mut staged = Vec::new();
    let Some(ChunkOutcome::End(end)) = r
        .next_chunks(deadline(), |e| {
            if let ChunkEvent::Chunk(p) = e {
                staged.extend_from_slice(p.bytes);
            }
            Ok::<(), io::Error>(())
        })
        .unwrap()
    else {
        panic!("end")
    };
    assert_eq!(staged, first);
    assert_eq!((end.offset, end.recovery.unwrap().scanned_bytes), (7, 7));
    assert!(matches!(
        r.next_chunks(deadline(), |_| -> Result<(), io::Error> { panic!("tail") })
            .unwrap(),
        Some(ChunkOutcome::Tail {
            tail: Tail {
                kind: TailKind::Unsynchronized,
                bytes: 25,
                ..
            },
            ..
        })
    ));
    let mut r = WholePacketReader::stream(
        Cursor::new(first),
        WholePolicy::new().with_hard_limit(32),
        pool,
    )
    .unwrap();
    let e = r
        .next_chunks(deadline(), |_| -> Result<(), io::Error> {
            panic!("H refusal")
        })
        .unwrap_err();
    assert!(e.abort.is_none());
    assert!(r.refused_header().is_some());
    assert_eq!(r.position(), 24);
    assert!(
        matches!(r.drain_next(deadline()).unwrap(),Some(DrainEvent::Report(d))if d.consumed_bytes==48)
    );
}

#[test]
fn active_full_pool_does_not_block_fixed_workspace_chunks() {
    let pool = BlockPool::new(2 * BLOCK_BYTES + 131072).unwrap();
    let (bytes, _) = fixture(8236, 4, false);
    let mut r =
        WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool.clone()).unwrap();
    let mut assembly = pool.try_begin(2).unwrap();
    let held = [
        assembly.try_next_block().unwrap(),
        assembly.try_next_block().unwrap(),
    ];
    let before = pool.stats().unwrap().used_bytes;
    assert!(matches!(
        r.next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
            .unwrap(),
        Some(ChunkOutcome::End(_))
    ));
    let after = pool.stats().unwrap();
    assert_eq!(after.used_bytes, before);
    assert_eq!(after.waiting, 0);
    assert!(after.used_bytes <= after.budget_bytes);
    drop((held, assembly));
}

#[test]
fn huge_claim_after_target_or_budget_refusal_is_chunked_not_allocated() {
    let (mut bytes, _) = fixture(48, 0, false);
    bytes[4..8].copy_from_slice(&0xffff_fffcu32.to_le_bytes());
    let sum: u32 = bytes[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    bytes[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(
        Cursor::new(bytes),
        WholePolicy::new()
            .with_hard_limit(u32::MAX)
            .with_stream_opt_in(true),
        pool.clone(),
    )
    .unwrap();
    r.next_packet_into(&mut PacketBuf::new(pool.clone()).unwrap(), deadline())
        .unwrap_err();
    assert_eq!(r.refused_header().unwrap().packet_length(), 0xffff_fffc);
    let mut delivered = 0;
    let e = r
        .next_chunks(deadline(), |e| {
            if let ChunkEvent::Chunk(c) = e {
                delivered += c.bytes.len();
            }
            Ok::<(), io::Error>(())
        })
        .unwrap_err();
    let a = e.abort.unwrap();
    assert_eq!(
        (
            a.requested_bytes,
            a.received_bytes,
            a.delivered_bytes,
            delivered
        ),
        (0xffff_fffc, 48, 48, 48)
    );
    assert_eq!(a.kind, ChunkAbortKind::Truncated);
    assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
}
#[test]
fn begin_rejection_and_header_only_final_source_check_fail_closed() {
    let (bytes, _) = fixture(48, 0, false);
    let pool = BlockPool::with_defaults().unwrap();
    let mut r =
        WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool.clone()).unwrap();
    let mut aborts = 0;
    let e = r
        .next_chunks(deadline(), |e| match e {
            ChunkEvent::Begin(_) => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            ChunkEvent::Abort(_) => {
                aborts += 1;
                Ok(())
            }
            _ => panic!("no bytes after Begin rejection"),
        })
        .unwrap_err();
    assert_eq!(aborts, 1);
    let a = e.abort.unwrap();
    assert_eq!(
        (a.received_bytes, a.offered_bytes, a.delivered_bytes),
        (24, 0, 0)
    );
    assert!(
        r.next_chunks(deadline(), |_| Ok::<(), io::Error>(()))
            .unwrap()
            .is_none()
    );
    let revision = Arc::new(AtomicU64::new(0));
    let mut bytes = vec![0; 24];
    bytes[..2].copy_from_slice(&[0x25, 0xeb]);
    bytes[4] = 24;
    let sum: u32 = bytes[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    bytes[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    let mut r = WholePacketReader::seek_checked(
        Stable {
            input: Cursor::new(bytes),
            revision: revision.clone(),
        },
        WholePolicy::new(),
        pool,
    )
    .unwrap();
    let e = r
        .next_chunks(deadline(), |e| {
            if let ChunkEvent::Chunk(_) = e {
                revision.store(1, Ordering::Relaxed);
            }
            Ok::<(), io::Error>(())
        })
        .unwrap_err();
    assert_eq!(e.abort.unwrap().kind, ChunkAbortKind::SourceChanged);
}
