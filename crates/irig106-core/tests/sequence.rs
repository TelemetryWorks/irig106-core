//! L3-SEQ-006: independent primary fixtures and exact per-channel comparisons.
use irig106_core::{
    Inspection, InspectionError, InspectionLimits, PacketReader, Presence, ReaderLimits,
    SequenceKind,
};

fn frame(channel: u16, ty: u8, sequence: u8) -> Vec<u8> {
    let mut b = vec![0; 28];
    b[..2].copy_from_slice(&[0x25, 0xEB]);
    b[2..4].copy_from_slice(&channel.to_le_bytes());
    b[4] = 28;
    b[8] = 4;
    b[13] = sequence;
    b[15] = ty;
    let sum = b[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    b[22..24].copy_from_slice(&sum.to_le_bytes());
    b
}

#[test]
fn interleaved_channels_share_sequence_across_types_and_wrap() {
    use SequenceKind::{Contiguous, Discontinuity, Duplicate, First};
    let input = [
        (1, 9, 17),
        (2, 9, 254),
        (1, 0xD9, 18),
        (2, 9, 255),
        (2, 3, 0),
        (1, 9, 18),
        (1, 9, 23),
        (1, 9, 24),
        (1, 9, 22),
    ];
    let b: Vec<_> = input.iter().flat_map(|&(c, t, s)| frame(c, t, s)).collect();
    let mut r = PacketReader::new(&b[..], ReaderLimits::new()).unwrap();
    let mut a = Inspection::new(InspectionLimits::new(2, 4));
    let expected = [
        (None, None, First),
        (None, None, First),
        (Some(17), Some(18), Contiguous),
        (Some(254), Some(255), Contiguous),
        (Some(255), Some(0), Contiguous),
        (Some(18), Some(19), Duplicate),
        (Some(18), Some(19), Discontinuity),
        (Some(23), Some(24), Contiguous),
        (Some(24), Some(25), Discontinuity),
    ];
    for (i, ((ch, _, seq), (previous, next, kind))) in input.into_iter().zip(expected).enumerate() {
        let e = r.next_packet(|_| 0).unwrap().unwrap();
        let s = a.observe_with_sequence(&e).unwrap().unwrap();
        assert_eq!(
            (s.channel, s.offset, s.observed, s.recovered_bytes),
            (ch, i as u64 * 28, seq, 0)
        );
        assert_eq!((s.previous, s.expected(), s.kind()), (previous, next, kind));
    }
    assert!(r.next_packet(|_| 0).unwrap().is_none());
    assert_eq!(
        (a.channel_count(), a.rows().count(), a.totals().packets),
        (2, 4, 9)
    );
}

#[test]
fn one_byte_recovery_is_sticky_and_does_not_reset_sequence() {
    let b = [frame(0, 1, 8), vec![0xFF], frame(0, 1, 10), frame(0, 1, 11)].concat();
    let mut r = PacketReader::new(&b[..], ReaderLimits::new()).unwrap();
    let mut a = Inspection::new(InspectionLimits::new(1, 1));
    a.observe(&r.next_packet(|_| 0).unwrap().unwrap()).unwrap();
    for (offset, previous, observed, recovered, kind) in [
        (29, 8, 10, 1, SequenceKind::Discontinuity),
        (57, 10, 11, 0, SequenceKind::Contiguous),
    ] {
        let s = a
            .observe_with_sequence(&r.next_packet(|_| 0).unwrap().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                s.offset,
                s.previous,
                s.observed,
                s.recovered_bytes,
                s.kind()
            ),
            (offset, Some(previous), observed, recovered, kind)
        );
    }
    assert!(r.next_packet(|_| 0).unwrap().is_none());
    a.finish();
    let p = a.presence();
    assert_eq!(
        (p.setup_record, p.recording_index, p.recording_events),
        (Presence::Present, Presence::Unknown, Presence::Unknown)
    );
}

#[test]
fn tail_and_rejected_packet_return_no_sequence_observation() {
    let b = [frame(1, 9, 70), vec![0x25]].concat();
    let mut r = PacketReader::new(&b[..], ReaderLimits::new()).unwrap();
    let mut a = Inspection::new(InspectionLimits::new(1, 1));
    assert_eq!(
        a.observe_with_sequence(&r.next_packet(|_| 0).unwrap().unwrap())
            .unwrap()
            .unwrap()
            .kind(),
        SequenceKind::First
    );
    assert!(
        a.observe_with_sequence(&r.next_packet(|_| 0).unwrap().unwrap())
            .unwrap()
            .is_none()
    );
    let b = frame(2, 9, 0);
    let e = PacketReader::new(&b[..], ReaderLimits::new())
        .unwrap()
        .next_packet(|_| 0)
        .unwrap()
        .unwrap();
    assert_eq!(a.observe_with_sequence(&e), Err(InspectionError::Stopped));
    let mut limited = Inspection::new(InspectionLimits::new(0, 0));
    assert!(matches!(
        limited.observe_with_sequence(&e),
        Err(InspectionError::ChannelLimit { limit: 0, .. })
    ));
    assert_eq!((limited.channel_count(), limited.totals().packets), (0, 0));
}
