//! L3-INV-006/PRES-006: owner-selected budgets, raw header facts only.
use irig106_core::{
    Event, Inspection, InspectionError, InspectionLimits, PacketReader, Presence, ReaderLimits,
    ScanStatus,
};

fn frame(channel: u16, ty: u8, data: usize) -> Vec<u8> {
    let mut b = vec![0; 24 + data];
    b[..2].copy_from_slice(&[0x25, 0xEB]);
    b[2..4].copy_from_slice(&channel.to_le_bytes());
    b[4..8].copy_from_slice(&((24 + data) as u32).to_le_bytes());
    b[8..12].copy_from_slice(&(data as u32).to_le_bytes());
    b[15] = ty;
    let sum: u32 = b[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    b[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    b
}
fn event(channel: u16, ty: u8, data: usize) -> Event {
    PacketReader::new(&frame(channel, ty, data)[..], ReaderLimits::new())
        .unwrap()
        .next_packet(|_| 0)
        .unwrap()
        .unwrap()
}

#[test]
fn interleaved_rows_types_and_declared_bytes_are_exact() {
    let mut a = Inspection::new(InspectionLimits::new(2, 3));
    for (ch, ty, data) in [(2, 9, 4), (1, 0xD9, 8), (2, 9, 12), (2, 1, 4)] {
        a.observe(&event(ch, ty, data)).unwrap();
    }
    let t = a.totals();
    assert_eq!((t.packets, t.data_bytes, t.packet_bytes), (4, 28, 124));
    assert_eq!(a.channel_count(), 2);
    assert_eq!(a.rows().count(), 3);
    let (_, row) = a.rows().find(|(key, _)| *key == (2, 9)).unwrap();
    assert_eq!(
        (
            row.counts.packets,
            row.counts.data_bytes,
            row.counts.packet_bytes,
            row.min_data_length,
            row.max_data_length
        ),
        (2, 16, 64, 4, 12)
    );
    assert_eq!(
        a.data_types()
            .map(|(ty, c)| (ty, c.packets, c.data_bytes, c.packet_bytes))
            .collect::<Vec<_>>(),
        vec![(1, 1, 4, 28), (9, 2, 16, 64), (0xD9, 1, 8, 32)]
    );
    assert_eq!(a.presence().setup_record, Presence::Unknown);
    a.finish();
    assert_eq!(a.status(), ScanStatus::Complete);
    assert_eq!(a.presence().setup_record, Presence::Absent); // type1 on channel2 is not setup routing.
}

#[test]
fn entry_limits_fail_transactionally_and_keep_absence_inconclusive() {
    let mut a = Inspection::new(InspectionLimits::new(1, 3));
    a.observe(&event(7, 9, 4)).unwrap();
    a.observe(&event(7, 0xD9, 4)).unwrap(); // Same channel, another row allowed.
    assert!(matches!(
        a.observe(&event(8, 9, 4)),
        Err(InspectionError::ChannelLimit { limit: 1, .. })
    ));
    assert_eq!(
        (a.totals().packets, a.channel_count(), a.rows().count()),
        (2, 1, 2)
    );
    a.finish();
    assert_eq!(a.status(), ScanStatus::Incomplete);
    assert_eq!(a.presence().recording_index, Presence::Unknown);
    assert!(matches!(
        a.observe(&event(7, 9, 4)),
        Err(InspectionError::Stopped)
    ));
    let mut a = Inspection::new(InspectionLimits::new(2, 1));
    a.observe(&event(7, 9, 4)).unwrap();
    a.observe(&event(7, 9, 4)).unwrap(); // Existing row still works at cap.
    assert!(matches!(
        a.observe(&event(8, 0xD9, 4)),
        Err(InspectionError::RowLimit { limit: 1, .. })
    ));
    assert_eq!(
        (
            a.totals().packets,
            a.channel_count(),
            a.rows().count(),
            a.data_types().count()
        ),
        (2, 1, 1, 1)
    );
    for (channels, rows) in [(0, 1), (1, 0)] {
        let mut a = Inspection::new(InspectionLimits::new(channels, rows));
        assert!(a.observe(&event(1, 9, 4)).is_err());
        assert_eq!(
            (a.totals().packets, a.channel_count(), a.rows().count()),
            (0, 0, 0)
        );
    }
}

#[test]
fn presence_is_header_evidence_and_scan_completion_is_explicit() {
    let mut a = Inspection::new(InspectionLimits::new(4, 4));
    for (ch, ty) in [(0, 1), (5, 2), (6, 3), (7, 9)] {
        a.observe(&event(ch, ty, 4)).unwrap();
    }
    let p = a.presence();
    assert_eq!(
        (p.setup_record, p.recording_events, p.recording_index),
        (Presence::Present, Presence::Present, Presence::Present)
    );
    a.mark_incomplete();
    a.finish();
    assert_eq!(a.status(), ScanStatus::Incomplete);
    assert_eq!(a.presence().setup_record, Presence::Present); // Positive evidence survives.
    let mut a = Inspection::new(InspectionLimits::new(1, 1));
    a.observe(&event(1, 9, 4)).unwrap();
    assert_eq!(a.presence().setup_record, Presence::Unknown);
    a.finish();
    assert_eq!(a.presence().setup_record, Presence::Absent);
    let mut empty = Inspection::new(InspectionLimits::new(0, 0));
    empty.finish();
    assert_eq!(empty.status(), ScanStatus::Incomplete);
    assert_eq!(empty.presence().setup_record, Presence::Unknown);
}

#[test]
fn mid_file_recovery_keeps_unseen_presence_unknown_after_clean_eof() {
    let mut corrupt_index = frame(5, 3, 4);
    corrupt_index[22] ^= 1; // Invalid primary checksum; reader must skip this index.
    let b = [
        frame(0, 1, 4),
        corrupt_index,
        frame(7, 9, 4),
        frame(7, 9, 4), // A later unrecovered packet must not clear the evidence.
    ]
    .concat();
    let mut r = PacketReader::new(&b[..], ReaderLimits::new()).unwrap();
    let mut a = Inspection::new(InspectionLimits::new(2, 2));
    let mut recovery = Vec::new();
    while let Some(e) = r.next_packet(|_| 0).unwrap() {
        match &e {
            Event::Packet(p) => recovery.push((p.offset, p.recovered_bytes)),
            _ => panic!("expected complete packets followed by clean EOF"),
        }
        a.observe(&e).unwrap();
    }
    assert_eq!(recovery, [(0, 0), (56, 28), (84, 0)]);
    a.finish();
    assert_eq!(a.status(), ScanStatus::Complete);
    assert_eq!(a.totals().packets, 3);
    let p = a.presence();
    assert_eq!(p.setup_record, Presence::Present);
    assert_eq!(p.recording_index, Presence::Unknown);
    assert_eq!(p.recording_events, Presence::Unknown);
}

#[test]
fn index_only_complete_scan_does_not_imply_events() {
    let mut a = Inspection::new(InspectionLimits::new(1, 1));
    a.observe(&event(5, 3, 4)).unwrap();
    a.finish();
    assert_eq!(a.status(), ScanStatus::Complete);
    let p = a.presence();
    assert_eq!(p.recording_index, Presence::Present);
    assert_eq!(p.recording_events, Presence::Absent);
    assert_eq!(p.setup_record, Presence::Absent);
}

#[test]
fn events_only_complete_scan_does_not_imply_index() {
    let mut a = Inspection::new(InspectionLimits::new(1, 1));
    a.observe(&event(5, 2, 4)).unwrap();
    a.finish();
    assert_eq!(a.status(), ScanStatus::Complete);
    let p = a.presence();
    assert_eq!(p.recording_index, Presence::Absent);
    assert_eq!(p.recording_events, Presence::Present);
    assert_eq!(p.setup_record, Presence::Absent);
}

#[test]
fn inspection_error_display_is_human_readable() {
    for (channels, rows, text) in [
        (0, 1, "inspection channel limit of 0 exceeded"),
        (1, 0, "inspection row limit of 0 exceeded"),
    ] {
        let mut a = Inspection::new(InspectionLimits::new(channels, rows));
        assert_eq!(a.observe(&event(1, 9, 4)).unwrap_err().to_string(), text);
        assert_eq!(
            a.observe(&event(1, 9, 4)).unwrap_err().to_string(),
            "inspection is no longer accepting events"
        );
    }
    assert_eq!(
        InspectionError::CountOverflow.to_string(),
        "inspection counters exceed u64 range"
    );
}

#[test]
fn terminal_tail_cannot_be_overridden_by_finish() {
    let b = [frame(0, 1, 4), vec![0x25, 0xEB, 0]].concat();
    let mut r = PacketReader::new(&b[..], ReaderLimits::new()).unwrap();
    let mut a = Inspection::new(InspectionLimits::new(1, 1));
    while let Some(e) = r.next_packet(|_| 0).unwrap() {
        a.observe(&e).unwrap();
    }
    a.finish();
    assert_eq!(a.totals().packets, 1);
    assert_eq!(a.status(), ScanStatus::Incomplete);
    assert_eq!(
        (a.presence().setup_record, a.presence().recording_index),
        (Presence::Present, Presence::Unknown)
    );
}

#[test]
fn repeated_packets_do_not_grow_inventory() {
    let events = [
        event(1, 9, 4),
        event(1, 0xD9, 4),
        event(2, 9, 4),
        event(2, 0xD9, 4),
    ];
    let mut a = Inspection::new(InspectionLimits::new(2, 4));
    for e in events.iter().cycle().take(10_000) {
        a.observe(e).unwrap();
    }
    a.finish();
    let t = a.totals();
    assert_eq!(
        (t.packets, t.data_bytes, t.packet_bytes),
        (10_000, 40_000, 280_000)
    );
    assert_eq!(
        (a.channel_count(), a.rows().count(), a.data_types().count()),
        (2, 4, 2)
    );
}

#[path = "support/watchdog.rs"]
mod support;
fn sequence(mut bytes: Vec<u8>, value: u8) -> Vec<u8> {
    bytes[13] = value;
    let sum = bytes[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    bytes[22..24].copy_from_slice(&sum.to_le_bytes());
    bytes
}
#[test]
fn whole_metadata_matches_capture_inventory_without_payload_conversion() {
    support::watchdog(std::time::Duration::from_secs(5), || {
        use irig106_core::{BlockPool, PacketBuf, SequenceKind, WholePacketReader, WholePolicy};
        use std::{
            io::Cursor,
            time::{Duration, Instant},
        };
        let mut input = sequence(frame(1, 9, 4), 254);
        input.extend(sequence(frame(2, 3, 8), 10));
        input.push(0x55);
        input.extend(sequence(frame(2, 2, 4), 11));
        input.extend(sequence(frame(1, 9, 12), 255));
        input.extend(sequence(frame(1, 9, 4), 0));
        let pool = BlockPool::with_defaults().unwrap();
        let mut buffer = PacketBuf::new(pool.clone()).unwrap();
        let mut whole =
            WholePacketReader::stream(Cursor::new(input.clone()), WholePolicy::new(), pool)
                .unwrap();
        let mut capture = PacketReader::new(Cursor::new(input), ReaderLimits::new()).unwrap();
        let mut a = Inspection::new(InspectionLimits::new(2, 3));
        let mut b = Inspection::new(InspectionLimits::new(2, 3));
        for (expected, offset) in [
            (SequenceKind::First, 0),
            (SequenceKind::First, 28),
            (SequenceKind::Unverified, 61),
            (SequenceKind::Unverified, 89),
            (SequenceKind::Contiguous, 125),
        ] {
            whole
                .next_packet_into(&mut buffer, Instant::now() + Duration::from_secs(2))
                .unwrap();
            let observed = a.observe_packet_info(buffer.info().unwrap()).unwrap();
            let legacy = capture.next_packet(|_| 0).unwrap().unwrap();
            let legacy = b.observe_with_sequence(&legacy).unwrap().unwrap();
            assert_eq!(observed, legacy);
            assert_eq!(observed.kind(), expected);
            assert_eq!(observed.offset, offset);
        }
        assert!(
            whole
                .next_packet_into(&mut buffer, Instant::now() + Duration::from_secs(2))
                .unwrap()
                .is_none()
        );
        assert!(capture.next_packet(|_| 0).unwrap().is_none());
        a.finish();
        b.finish();
        assert_eq!(a.totals(), b.totals());
        assert_eq!(
            (
                a.totals().packets,
                a.totals().data_bytes,
                a.totals().packet_bytes
            ),
            (5, 32, 152)
        );
        assert_eq!(a.rows().collect::<Vec<_>>(), b.rows().collect::<Vec<_>>());
        assert_eq!(
            a.data_types().collect::<Vec<_>>(),
            b.data_types().collect::<Vec<_>>()
        );
        assert_eq!(a.presence(), b.presence());
        assert_eq!(a.status(), ScanStatus::Complete);
        assert_eq!(a.presence().setup_record, Presence::Unknown);
        assert_eq!(a.presence().recording_index, Presence::Present);
        assert_eq!(a.presence().recording_events, Presence::Present);
    });
}
#[test]
fn whole_metadata_limits_and_terminal_status_preserve_prior_counts() {
    support::watchdog(std::time::Duration::from_secs(5), || {
        use irig106_core::{BlockPool, PacketBuf, WholePacketReader, WholePolicy};
        use std::{
            io::Cursor,
            time::{Duration, Instant},
        };
        for (channel, kind, channels, rows) in [(2, 9, 1, 2), (1, 3, 2, 1)] {
            let pool = BlockPool::with_defaults().unwrap();
            let mut buffer = PacketBuf::new(pool.clone()).unwrap();
            let bytes = [frame(1, 9, 4), frame(channel, kind, 4)].concat();
            let mut r =
                WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool).unwrap();
            let mut inventory = Inspection::new(InspectionLimits::new(channels, rows));
            r.next_packet_into(&mut buffer, Instant::now() + Duration::from_secs(2))
                .unwrap();
            inventory
                .observe_packet_info(buffer.info().unwrap())
                .unwrap();
            let before = inventory.totals();
            r.next_packet_into(&mut buffer, Instant::now() + Duration::from_secs(2))
                .unwrap();
            let error = inventory
                .observe_packet_info(buffer.info().unwrap())
                .unwrap_err();
            assert!(matches!(
                (channel, error),
                (2, InspectionError::ChannelLimit { .. }) | (1, InspectionError::RowLimit { .. })
            ));
            assert_eq!(inventory.totals(), before);
            assert_eq!(
                (inventory.channel_count(), inventory.rows().count()),
                (1, 1)
            );
            inventory.finish();
            assert_eq!(inventory.status(), ScanStatus::Incomplete);
            assert_eq!(
                inventory.observe_packet_info(buffer.info().unwrap()),
                Err(InspectionError::Stopped)
            );
        }
    });
}

#[test]
fn whole_tail_consumer_keeps_unseen_facts_unknown() {
    support::watchdog(std::time::Duration::from_secs(5), || {
        use irig106_core::{BlockPool, PacketBuf, WholeEvent, WholePacketReader, WholePolicy};
        use std::{
            io::Cursor,
            time::{Duration, Instant},
        };
        let pool = BlockPool::with_defaults().unwrap();
        let mut buffer = PacketBuf::new(pool.clone()).unwrap();
        let mut bytes = frame(1, 9, 4);
        bytes.push(0x55);
        let mut r =
            WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool).unwrap();
        let mut inventory = Inspection::new(InspectionLimits::new(1, 1));
        r.next_packet_into(&mut buffer, Instant::now() + Duration::from_secs(2))
            .unwrap();
        inventory
            .observe_packet_info(buffer.info().unwrap())
            .unwrap();
        assert!(matches!(
            r.next_packet_into(&mut buffer, Instant::now() + Duration::from_secs(2))
                .unwrap(),
            Some(WholeEvent::Tail { .. })
        ));
        assert!(buffer.info().is_none());
        inventory.mark_incomplete();
        inventory.finish();
        assert_eq!(inventory.totals().packets, 1);
        assert_eq!(inventory.status(), ScanStatus::Incomplete);
        assert_eq!(inventory.presence().recording_index, Presence::Unknown);
    });
}
