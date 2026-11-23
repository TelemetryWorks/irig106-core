//! Slice4: L3-END-004/REC-003/ERR-004; seed framing oracle from §11.2.1.1.
use irig106_core::{ErrorKind, Event, HeaderError, PacketReader, ReaderLimits, TailKind};
use std::io::{self, Read};

fn packet(flags: u8, data: &[u8]) -> Vec<u8> {
    let size = 24 + if flags & 0x80 != 0 { 12 } else { 0 };
    let len = (size + data.len()).next_multiple_of(4);
    let mut b = vec![0xAA; len];
    b[..24].fill(0);
    b[..2].copy_from_slice(&[0x25, 0xEB]);
    b[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    b[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    b[14] = flags;
    b[15] = 9;
    // Fixture-only word arithmetic; never call production checksum/encoder.
    let sum: u32 = b[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    b[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    b[size..size + data.len()].copy_from_slice(data);
    b
}
fn reader(b: &[u8], limit: Option<u64>) -> PacketReader<&[u8]> {
    PacketReader::new(
        b,
        ReaderLimits::new()
            .with_buffer_bytes(3)
            .with_capture_bytes(4)
            .with_recovery_bytes(limit),
    )
    .unwrap()
}

fn recovered(start_offset: u64, scanned_bytes: u64) -> Option<(u64, u64)> {
    Some((start_offset, scanned_bytes))
}

#[test]
fn empty_and_every_first_packet_cut_are_no_complete_packet() {
    for flags in [0, 0x80] {
        let b = packet(flags, &[1; 4]);
        for capture in [0, 4] {
            for cut in 0..b.len() {
                let mut r = reader(&b[..cut], None);
                let error = r.next_packet(|_| capture).unwrap_err();
                let ErrorKind::NoCompletePacket { tail, .. } = error.reason else {
                    panic!("{error:?}")
                };
                if cut == 0 {
                    assert!(tail.is_none());
                } else {
                    let tail = tail.unwrap();
                    assert_eq!((tail.offset, tail.bytes), (0, cut as u64));
                    assert_eq!(
                        tail.kind,
                        if cut < 24 {
                            TailKind::PartialHeader
                        } else {
                            TailKind::TruncatedPacket
                        }
                    );
                }
                assert_eq!(error.position, cut as u64);
                assert!(r.next_packet(|_| 0).unwrap().is_none());
            }
        }
    }
}

#[test]
fn every_established_packet_cut_remains_a_tail() {
    let good = packet(0, &[1; 4]);
    for flags in [0, 0x80] {
        let next = packet(flags, &[2; 4]);
        for capture in [0, 4] {
            for cut in 1..next.len() {
                let b = [good.clone(), next[..cut].to_vec()].concat();
                let mut r = reader(&b, None);
                assert!(matches!(
                    r.next_packet(|_| 0).unwrap(),
                    Some(Event::Packet(_))
                ));
                let Some(Event::Tail {
                    offset,
                    bytes,
                    kind,
                    recovery,
                    ..
                }) = r.next_packet(|_| capture).unwrap()
                else {
                    panic!("tail")
                };
                assert_eq!(
                    (offset, bytes, kind, recovery),
                    (
                        28,
                        cut as u64,
                        if cut < 24 {
                            TailKind::PartialHeader
                        } else {
                            TailKind::TruncatedPacket
                        },
                        None
                    )
                );
                assert!(r.next_packet(|_| 0).unwrap().is_none());
            }
        }
    }
}

#[test]
fn recovery_cap_is_inclusive_and_optional() {
    assert_eq!(ReaderLimits::default().recovery_bytes, None);
    let b = [vec![0xFF; 7], packet(0, &[1; 4])].concat();
    let Some(Event::Packet(p)) = reader(&b, Some(7)).next_packet(|_| 0).unwrap() else {
        panic!("packet")
    };
    assert_eq!((p.offset, p.recovered_bytes), (7, 7));
    let mut r = reader(&b, Some(6));
    let e = r.next_packet(|_| 0).unwrap_err();
    assert!(matches!(
        e.reason,
        ErrorKind::RecoveryLimit { limit: 6, .. }
    ));
    assert_eq!(
        e.recovery.map(|r| (r.start_offset, r.scanned_bytes)),
        recovered(0, 6)
    );
    assert_eq!(e.position, 30); // 24-byte lookahead plus six allowed shifts.
    assert!(r.next_packet(|_| 0).unwrap().is_none());
    assert!(matches!(
        reader(&packet(0, &[1; 4]), Some(0))
            .next_packet(|_| 0)
            .unwrap(),
        Some(Event::Packet(_))
    ));
    let e = reader(&b, Some(0)).next_packet(|_| 0).unwrap_err();
    assert!(matches!(
        e.reason,
        ErrorKind::RecoveryLimit { limit: 0, .. }
    ));
    assert_eq!(e.recovery.unwrap().scanned_bytes, 0);
}

#[test]
fn recovery_budget_resets_per_incident_and_eof_keeps_evidence() {
    let group = [vec![0xFF; 7], packet(0, &[1; 4])].concat();
    let b = group.repeat(2);
    let mut r = reader(&b, Some(7));
    for offset in [7, 42] {
        let Some(Event::Packet(p)) = r.next_packet(|_| 0).unwrap() else {
            panic!("packet")
        };
        assert_eq!((p.offset, p.recovered_bytes), (offset, 7));
    }
    assert!(r.next_packet(|_| 0).unwrap().is_none());
    let junk = [0xFF; 27];
    let e = reader(&junk, None).next_packet(|_| 0).unwrap_err();
    assert!(matches!(e.reason, ErrorKind::NoCompletePacket { .. }));
    assert_eq!(
        e.recovery.map(|r| (r.start_offset, r.scanned_bytes)),
        recovered(0, 3)
    );
    let b = [packet(0, &[1; 4]), junk.to_vec()].concat();
    let mut r = reader(&b, None);
    r.next_packet(|_| 0).unwrap();
    let Some(Event::Tail {
        offset,
        bytes,
        kind,
        recovery,
        ..
    }) = r.next_packet(|_| 0).unwrap()
    else {
        panic!("tail")
    };
    assert_eq!((offset, bytes, kind), (28, 27, TailKind::Unsynchronized));
    assert_eq!(
        recovery.map(|r| (r.start_offset, r.scanned_bytes)),
        recovered(28, 3)
    );
}

#[test]
fn structural_and_io_failures_keep_typed_causes_and_recovery() {
    let mut bad = packet(0, &[]);
    bad[4] = 30;
    bad[22..24].copy_from_slice(&[0x43, 0xF4]); // EB25+001E+0900.
    let b = [packet(0, &[1; 4]), vec![0xFF; 7], bad].concat();
    let mut r = reader(&b, None);
    r.next_packet(|_| 0).unwrap();
    let e = r.next_packet(|_| 0).unwrap_err();
    assert!(matches!(
        e.reason,
        ErrorKind::Header {
            source: HeaderError::UnalignedPacket { length: 30 },
            ..
        }
    ));
    assert_eq!(
        e.recovery.map(|r| (r.start_offset, r.scanned_bytes)),
        recovered(28, 7)
    );
    assert!(
        std::error::Error::source(&e)
            .unwrap()
            .downcast_ref::<HeaderError>()
            .is_some()
    );
    struct FailAtEnd<'a>(&'a [u8]);
    impl Read for FailAtEnd<'_> {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                self.0.read(b)
            }
        }
    }
    let b = [packet(0, &[1; 4]), vec![0xFF; 27]].concat();
    let mut r = PacketReader::new(FailAtEnd(&b), ReaderLimits::default()).unwrap();
    r.next_packet(|_| 0).unwrap();
    let e = r.next_packet(|_| 0).unwrap_err();
    assert!(
        matches!(&e.reason, ErrorKind::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied)
    );
    assert_eq!(
        e.recovery.map(|r| (r.start_offset, r.scanned_bytes)),
        recovered(28, 3)
    );
    assert_eq!(
        std::error::Error::source(&e)
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(r.next_packet(|_| 0).unwrap().is_none());
}

#[test]
fn claimed_truncated_body_is_not_rescanned_for_nested_sync() {
    let good = packet(0, &[1; 4]);
    let claimed = packet(0, &[2; 100]);
    let tail = [claimed[..24].to_vec(), good.clone()].concat();
    let b = [good, tail].concat();
    let mut r = reader(&b, None);
    r.next_packet(|_| 0).unwrap();
    assert!(matches!(
        r.next_packet(|_| 0).unwrap(),
        Some(Event::Tail {
            offset: 28,
            bytes: 52,
            kind: TailKind::TruncatedPacket,
            recovery: None,
            ..
        })
    ));
    assert!(r.next_packet(|_| 0).unwrap().is_none());
}

#[test]
fn caller_limit_errors_are_typed_and_do_not_become_no_complete_packet() {
    let e = PacketReader::new(&[][..], ReaderLimits::new().with_buffer_bytes(0)).unwrap_err();
    assert!(matches!(e.reason, ErrorKind::InvalidBuffer));
    assert_eq!((e.position, e.recovery), (0, None));
    let b = [vec![0xFF; 7], packet(0, &[1; 4])].concat();
    let mut r = PacketReader::new(&b[..], ReaderLimits::new().with_capture_bytes(3)).unwrap();
    let e = r.next_packet(|_| usize::MAX).unwrap_err(); // Clamp to DATA4 first.
    assert!(matches!(
        e.reason,
        ErrorKind::CaptureLimit {
            requested: 4,
            limit: 3,
            ..
        }
    ));
    assert_eq!(
        e.recovery.map(|r| (r.start_offset, r.scanned_bytes)),
        recovered(0, 7)
    );
    assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    assert!(r.next_packet(|_| 0).unwrap().is_none());
}
