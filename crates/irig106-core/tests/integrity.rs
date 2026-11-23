//! L3-SEC-005/CHK-005; 106-24R1 §§11.2.1.2/11.2.1.4 and RDG-008.
use irig106_core::{
    DataChecksumState, ErrorKind, Event, Header, HeaderError, PacketRanges, PacketReader,
    ReaderLimits, SecondaryIntegrity,
};
use std::io;

fn frame(flags: u8, data: &[u8], len: usize) -> Vec<u8> {
    let start = if flags & 0x80 == 0 { 24 } else { 36 };
    let mut b = vec![0xFF; len];
    b[..24].fill(0);
    b[..2].copy_from_slice(&[0x25, 0xEB]);
    b[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    b[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    b[14] = flags;
    b[15] = 9;
    b[16..22].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
    let sum: u32 = b[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    b[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    b[start..start + data.len()].copy_from_slice(data);
    b
}

#[test]
fn secondary_byte_word_both_and_neither_are_distinct_evidence() {
    // Bytes1..10 sum to 0037; words0201+0403+0605+0807+0A09 = 1E19.
    for (stored, byte_valid, word_valid) in [
        (0x0037u16, true, false),
        (0x1E19, false, true),
        (0x1234, false, false),
    ] {
        let mut raw = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0, 0];
        raw[10..12].copy_from_slice(&stored.to_le_bytes());
        let e = SecondaryIntegrity::inspect(&raw);
        assert_eq!(
            (e.stored, e.byte_sum, e.word_sum, e.reserved),
            (stored, 0x0037, 0x1E19, 0x0A09)
        );
        assert_eq!((e.byte_valid(), e.word_valid()), (byte_valid, word_valid));
        let mut b = frame(0x80, &[1; 4], 40);
        b[24..36].copy_from_slice(&raw);
        let mut r = PacketReader::new(&b[..], ReaderLimits::new()).unwrap();
        let Some(Event::Packet(p)) = r.next_packet(|_| 4).unwrap() else {
            panic!("packet")
        };
        assert_eq!(p.secondary, Some(raw));
        assert_eq!(p.secondary_integrity, Some(e));
        assert_eq!(p.header.rtc().as_raw(), 0x0605_0403_0201);
    }
    let zero = SecondaryIntegrity::inspect(&[0; 12]);
    assert!(zero.byte_valid() && zero.word_valid());
    let high = SecondaryIntegrity::inspect(&[0xFF; 12]);
    assert_eq!((high.byte_sum, high.word_sum), (2550, 0xFFFB));
}

#[test]
fn every_trailer_width_keeps_data_filler_checksum_disjoint() {
    for secondary in [0, 0x80] {
        for (flag, width) in [(0, 0), (1, 1), (2, 2), (3, 4)] {
            let b = frame(secondary | flag, &[1, 2, 3, 4, 5], 48);
            let mut r = PacketReader::new(
                &b[..],
                ReaderLimits::new()
                    .with_buffer_bytes(3)
                    .with_capture_bytes(5),
            )
            .unwrap();
            let Some(Event::Packet(p)) = r.next_packet(|_| usize::MAX).unwrap() else {
                panic!("packet")
            };
            let start = if secondary == 0 { 24 } else { 36 };
            assert_eq!(p.ranges.data, start..start + 5);
            assert_eq!(p.ranges.filler, start + 5..48 - width);
            assert_eq!(p.ranges.checksum, 48 - width..48);
            assert_eq!(
                p.ranges.data_checksum,
                if width == 0 {
                    DataChecksumState::Absent
                } else {
                    DataChecksumState::NotChecked
                }
            );
            assert_eq!(p.body_prefix, [1, 2, 3, 4, 5]); // FF filler/checksum never DATA.
            assert_eq!(p.secondary_integrity.is_some(), secondary != 0);
            let e = PacketReader::new(&b[..47], ReaderLimits::default())
                .unwrap()
                .next_packet(|_| 5)
                .unwrap_err();
            assert!(
                matches!(e.reason, ErrorKind::NoCompletePacket {tail:Some(t)} if t.kind == irig106_core::TailKind::TruncatedPacket && t.bytes == 47)
            );
        }
    }
}

#[test]
fn insufficient_trailer_space_stops_before_capture() {
    for secondary in [0, 0x80] {
        let start = if secondary == 0 { 24 } else { 36 };
        for (flag, width) in [(1, 1), (2, 2), (3, 4)] {
            let b = frame(secondary | flag, &[1; 4], start + 4);
            let h = Header::parse(&b).unwrap(); // Primary-only bounds still pass.
            assert_eq!(
                PacketRanges::from_header(&h).unwrap_err(),
                HeaderError::DataTooLong {
                    length: 4,
                    available: 4 - width
                }
            );
            let mut r = PacketReader::new(&b[..], ReaderLimits::default()).unwrap();
            let e = r
                .next_packet(|_| panic!("invalid layout must stop before capture"))
                .unwrap_err();
            assert!(matches!(
                e.reason,
                ErrorKind::Header(HeaderError::DataTooLong { .. })
            ));
            assert_eq!(e.position, 24);
            assert!(r.next_packet(|_| 0).unwrap().is_none());
        }
    }
    let b = frame(3, &[], 24);
    assert_eq!(
        PacketRanges::from_header(&Header::parse(&b).unwrap()).unwrap_err(),
        HeaderError::PacketTooShort {
            length: 24,
            minimum: 28
        }
    );
}

#[test]
fn limit_builders_and_error_display_are_consumer_apis() {
    let defaults = ReaderLimits::default();
    assert_eq!(
        (
            defaults.buffer_bytes,
            defaults.capture_bytes,
            defaults.recovery_bytes
        ),
        (8192, 65536, None)
    );
    let limits = ReaderLimits::new()
        .with_buffer_bytes(3)
        .with_capture_bytes(5)
        .with_recovery_bytes(Some(7))
        .with_recovery_bytes(None);
    assert_eq!(
        (
            limits.buffer_bytes,
            limits.capture_bytes,
            limits.recovery_bytes
        ),
        (3, 5, None)
    );
    assert_eq!(
        ReaderLimits::new().recovery_bytes,
        ReaderLimits::default().recovery_bytes
    );
    for (reason, text) in [
        (
            ErrorKind::Io(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "reader blocked",
            )),
            "input read failed: reader blocked",
        ),
        (
            ErrorKind::Header(HeaderError::UnalignedPacket { length: 30 }),
            "invalid packet framing: packet length 30 is not divisible by four",
        ),
        (
            ErrorKind::NoCompletePacket { tail: None },
            "no complete Chapter 10 packet found",
        ),
        (
            ErrorKind::RecoveryLimit { limit: 7 },
            "recovery search exhausted caller limit of 7 bytes",
        ),
        (
            ErrorKind::InvalidBuffer,
            "reader buffer must contain at least one byte",
        ),
        (
            ErrorKind::CaptureLimit {
                requested: 5,
                limit: 4,
            },
            "requested DATA prefix 5 bytes exceeds capture limit 4 bytes",
        ),
        (
            ErrorKind::OffsetOverflow,
            "input byte offset exceeds u64 range",
        ),
    ] {
        assert_eq!(reason.to_string(), text);
    }
    let allocation = Vec::<u8>::new().try_reserve(usize::MAX).unwrap_err(); // No allocation attempted.
    assert!(
        ErrorKind::Allocation(allocation)
            .to_string()
            .starts_with("could not allocate DATA prefix: ")
    );
    let e = PacketReader::new(&[][..], ReaderLimits::default())
        .unwrap()
        .next_packet(|_| 0)
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "packet reader at 0: no complete Chapter 10 packet found"
    );
}

#[test]
fn range_arithmetic_handles_the_largest_aligned_length_without_packet_allocation() {
    let mut b = frame(0, &[], 24); // Only a primary header is ever allocated.
    b[4..8].copy_from_slice(&0xFFFF_FFFCu32.to_le_bytes());
    b[8..12].copy_from_slice(&0xFFFF_FFE4u32.to_le_bytes());
    let sum: u32 = b[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    b[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    let ranges = PacketRanges::from_header(&Header::parse(&b).unwrap()).unwrap();
    assert_eq!(ranges.data, 24..0xFFFF_FFFC);
    assert_eq!(ranges.filler, 0xFFFF_FFFC..0xFFFF_FFFC);
    assert_eq!(ranges.checksum, 0xFFFF_FFFC..0xFFFF_FFFC);
}
