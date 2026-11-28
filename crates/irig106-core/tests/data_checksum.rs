//! Independent weighted-byte oracle; production must not supply fixture sums.
use irig106_core::*;
use std::{
    io::{self, Cursor, Read},
    time::{Duration, Instant},
};
#[path = "support/watchdog.rs"]
mod support;
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn oracle(bytes: &[u8], width: usize) -> u32 {
    let sum: u64 = bytes
        .iter()
        .enumerate()
        .map(|(i, &b)| u64::from(b) << (8 * (i % width)))
        .sum();
    (sum & (u64::MAX >> (64 - width * 8))) as u32
}
fn primary(b: &mut [u8]) {
    let sum: u32 = b[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    b[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
}
fn fixture(p: usize, width: usize, secondary: bool, data: usize) -> (Vec<u8>, u32) {
    let h = if secondary { 36 } else { 24 };
    let mut b: Vec<u8> = (0..p).map(|i| ((i * 37 + 19) % 256) as u8).collect();
    b[..24].fill(0);
    b[..2].copy_from_slice(&[0x25, 0xeb]);
    b[4..8].copy_from_slice(&(p as u32).to_le_bytes());
    b[8..12].copy_from_slice(&(data as u32).to_le_bytes());
    b[14] = (if secondary { 0x80 } else { 0 })
        | match width {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 3,
        };
    b[15] = 1; // channel0 setup permits multi-block fixtures without stream opt-in.
    b[h + data..p - width].fill(0xff);
    primary(&mut b);
    let sum = if width == 0 {
        0
    } else {
        oracle(&b[h..p - width], width)
    };
    b[p - width..].copy_from_slice(&sum.to_le_bytes()[..width]);
    (b, sum)
}
struct Short {
    bytes: Cursor<Vec<u8>>,
    quantum: usize,
    interrupt: bool,
}
impl Read for Short {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.interrupt = !self.interrupt;
        if self.interrupt {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = out.len().min(self.quantum);
        self.bytes.read(&mut out[..n])
    }
}
#[test]
fn all_widths_match_independent_oracle_across_filler_blocks_and_read_strategies() {
    support::watchdog(Duration::from_secs(15), || {
        for width in [1, 2, 4] {
            for secondary in [false, true] {
                for p in [48, BLOCK_BYTES, BLOCK_BYTES + 4, 2 * BLOCK_BYTES + 28] {
                    let data = p - if secondary { 36 } else { 24 } - width - 5;
                    let (bytes, computed) = fixture(p, width, secondary, data);
                    for direct in [false, true] {
                        let pool = BlockPool::with_defaults().unwrap();
                        let mut buf = PacketBuf::new(pool.clone()).unwrap();
                        let policy = if direct {
                            WholePolicy::new()
                                .with_read_ahead_bytes(24)
                                .with_small_body_cutoff(0)
                        } else {
                            WholePolicy::new()
                        };
                        let mut r =
                            WholePacketReader::stream(Cursor::new(bytes.clone()), policy, pool)
                                .unwrap();
                        assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_some());
                        assert!(
                            matches!(buf.info().unwrap().ranges.data_checksum, DataChecksumState::Verified {width: w, value, ..} if w == width as u8 && value == computed)
                        );
                        let copy = buf.coalesce_until(deadline()).unwrap();
                        assert_eq!(
                            copy.info().ranges.data_checksum,
                            buf.info().unwrap().ranges.data_checksum
                        );
                        assert_eq!(copy.frame(), bytes);
                    }
                }
                for quantum in [1, 3, 7, 17] {
                    let (bytes, sum) = fixture(60, width, secondary, 5);
                    let pool = BlockPool::with_defaults().unwrap();
                    let mut buf = PacketBuf::new(pool.clone()).unwrap();
                    let input = Short {
                        bytes: Cursor::new(bytes),
                        quantum,
                        interrupt: false,
                    };
                    let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
                    r.next_packet_into(&mut buf, deadline()).unwrap();
                    assert!(
                        matches!(buf.info().unwrap().ranges.data_checksum, DataChecksumState::Verified {value,..} if value==sum)
                    );
                }
            }
        }
    });
}
#[test]
fn literal_sums_mismatch_evidence_and_exclusions_do_not_drop_packets() {
    for (width, expected) in [(1, 10), (2, 0x0604), (4, 0x04030201)] {
        let (mut bytes, _) = fixture(48, width, true, 4);
        bytes[36..48 - width].fill(0);
        bytes[36..40].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(oracle(&bytes[36..48 - width], width), expected);
        bytes[48 - width..].copy_from_slice(&expected.to_le_bytes()[..width]);
        for changed in [None, Some(16), Some(24), Some(36), Some(43), Some(47)] {
            let mut input = bytes.clone();
            if let Some(i) = changed {
                input[i] ^= 1;
                primary(&mut input);
            }
            let pool = BlockPool::with_defaults().unwrap();
            let mut buf = PacketBuf::new(pool.clone()).unwrap();
            let mut r =
                WholePacketReader::stream(Cursor::new(input.clone()), WholePolicy::new(), pool)
                    .unwrap();
            assert!(matches!(
                r.next_packet_into(&mut buf, deadline()).unwrap(),
                Some(WholeEvent::Packet)
            ));
            let e = buf.info().unwrap().ranges.data_checksum;
            if matches!(changed, None | Some(16) | Some(24)) {
                assert!(matches!(e,DataChecksumState::Verified {value,..} if value==expected));
            } else {
                let stored = u32::from_le_bytes({
                    let mut raw = [0; 4];
                    raw[..width].copy_from_slice(&input[48 - width..]);
                    raw
                });
                let computed = oracle(&input[36..48 - width], width);
                assert!(
                    matches!(e,DataChecksumState::Mismatch {stored:s,computed:c,width:w,..} if s==stored && c==computed && w==width as u8)
                );
            }
            assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
        }
    }
}
#[test]
fn disabled_capture_absent_and_inventory_coverage_are_explicit() {
    let mut inventory = Inspection::new(InspectionLimits::new(1, 1));
    for (width, verify, corrupt) in [
        (0, true, false),
        (1, true, false),
        (2, true, true),
        (4, false, false),
    ] {
        let (mut bytes, _) = fixture(48, width, false, 5);
        if corrupt {
            bytes[25] ^= 1;
        }
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut r = WholePacketReader::stream(
            Cursor::new(bytes.clone()),
            WholePolicy::new().with_data_checksum_verification(verify),
            pool,
        )
        .unwrap();
        r.next_packet_into(&mut buf, deadline()).unwrap();
        inventory.observe_packet_info(buf.info().unwrap()).unwrap();
        if !verify {
            assert_eq!(
                buf.info().unwrap().ranges.data_checksum,
                DataChecksumState::NotChecked
            );
        }
        if width == 0 {
            assert_eq!(
                buf.info().unwrap().ranges.data_checksum,
                DataChecksumState::Absent
            );
        }
        let mut capture = PacketReader::new(&bytes[..], ReaderLimits::new()).unwrap();
        let event = capture.next_packet(|_| 0).unwrap().unwrap();
        if let Event::Packet(p) = &event {
            assert_eq!(
                p.ranges.data_checksum,
                if width == 0 {
                    DataChecksumState::Absent
                } else {
                    DataChecksumState::NotChecked
                }
            );
        }
        inventory.observe(&event).unwrap();
    }
    let c = inventory.data_checksum_counts();
    assert_eq!(
        (c.absent, c.not_checked, c.verified, c.mismatch),
        (2, 4, 1, 1)
    );
    assert_eq!(inventory.totals().packets, 8);
    inventory.finish();
    assert_eq!(inventory.status(), ScanStatus::Complete);
}

#[test]
fn mismatch_continues_and_truncated_frame_never_claims_verification() {
    let (valid, _) = fixture(48, 4, false, 5);
    let mut bad = valid.clone();
    bad[24] ^= 1;
    let input = [bad.as_slice(), valid.as_slice(), &valid[..47]].concat();
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::stream(Cursor::new(input), WholePolicy::new(), pool).unwrap();
    let mut a = Inspection::new(InspectionLimits::new(1, 1));
    for (offset, mismatch) in [(0, true), (48, false)] {
        r.next_packet_into(&mut buf, deadline()).unwrap();
        let info = buf.info().unwrap();
        assert_eq!((info.offset, info.recovered_bytes), (offset, 0));
        assert_eq!(
            matches!(
                info.ranges.data_checksum,
                DataChecksumState::Mismatch { .. }
            ),
            mismatch
        );
        a.observe_packet_info(info).unwrap();
    }
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap(),
        Some(WholeEvent::Tail { .. })
    ));
    assert!(buf.info().is_none());
    a.mark_incomplete();
    assert_eq!(
        (
            a.data_checksum_counts().verified,
            a.data_checksum_counts().mismatch
        ),
        (1, 1)
    );
}
