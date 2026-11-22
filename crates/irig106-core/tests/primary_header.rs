//! Independent 106-24R1 Ch11 Table11-2 / §11.2.1.1 fixtures.
//! L3-HDR-001, L3-LEN-001; raw-code preservation clause of L3-EDN-005.
use irig106_core::{Header, HeaderError, PRIMARY_HEADER_LEN, Rtc, primary_checksum};

// Eleven words (checksum excluded): EB25+1234+0028+0000+0005+0000+
// A7FE+D904+0201+0403+0605 = 28A91, modulo 65536 = 8A91.
// Deliberately unknown type/version; bit2 set without bit7.
const DISTINCT: [u8; 24] = [
    0x25, 0xEB, 0x34, 0x12, 0x28, 0, 0, 0, 5, 0, 0, 0, 0xFE, 0xA7, 4, 0xD9, 1, 2, 3, 4, 5, 6, 0x91,
    0x8A,
];

// Hand-pack only structural fields. The independent scalar arithmetic below
// never invokes core's checksum or any packet encoder to make test input.
fn packed(length: u32, data: u32, flags: u8) -> [u8; 24] {
    let mut b = [0; 24];
    b[..2].copy_from_slice(&[0x25, 0xEB]);
    b[4..8].copy_from_slice(&length.to_le_bytes());
    b[8..12].copy_from_slice(&data.to_le_bytes());
    b[14] = flags;
    b[15] = 9;
    let sum = 0xEB25u32
        + (length & 0xFFFF)
        + (length >> 16)
        + (data & 0xFFFF)
        + (data >> 16)
        + u32::from(flags)
        + 0x0900;
    b[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    b
}

#[test]
fn literal_header_preserves_fields_and_shared_rtc() {
    let h = Header::parse(&DISTINCT).unwrap();
    assert_eq!(PRIMARY_HEADER_LEN, 24);
    assert_eq!(primary_checksum(&DISTINCT), 0x8A91);
    assert_eq!(h.channel(), 0x1234);
    assert_eq!(h.packet_length(), 40);
    assert_eq!(h.data_length(), 5);
    assert_eq!(h.data_type_version(), 0xFE);
    assert_eq!(h.sequence(), 0xA7);
    assert_eq!(h.flags(), 4);
    assert_eq!(h.data_type(), 0xD9);
    assert_eq!(h.rtc_bytes(), [1, 2, 3, 4, 5, 6]);
    let rtc: irig106_types::Rtc = h.rtc();
    assert_eq!(rtc, Rtc::from_raw(0x0605_0403_0201));
    assert_eq!(h.as_bytes(), &DISTINCT);
    assert!(!h.has_secondary_header());
    assert_eq!(h.header_length(), 24);
}

#[test]
fn checksum_wraps_excludes_stored_word_and_covers_every_primary_word() {
    // Eleven FFFF words = AFFF5; modulo 65536 = FFF5.
    assert_eq!(primary_checksum(&[0xFF; 24]), 0xFFF5);
    let mut b = DISTINCT;
    b[22..24].fill(0);
    assert_eq!(primary_checksum(&b), 0x8A91);
    for i in 2..22 {
        let mut b = DISTINCT;
        b[i] ^= 1;
        assert!(
            matches!(Header::parse(&b), Err(HeaderError::ChecksumMismatch { .. })),
            "byte {i}"
        );
    }
    for i in 22..24 {
        let mut b = DISTINCT;
        b[i] ^= 1;
        assert!(matches!(
            Header::parse(&b),
            Err(HeaderError::ChecksumMismatch { .. })
        ));
    }
}

#[test]
fn short_sync_and_checksum_failures_are_typed() {
    for n in 0..24 {
        assert_eq!(
            Header::parse(&DISTINCT[..n]),
            Err(HeaderError::TooShort { actual: n })
        );
    }
    let mut bad = DISTINCT;
    bad[..2].copy_from_slice(&[0xEB, 0x25]);
    assert_eq!(
        Header::parse(&bad),
        Err(HeaderError::InvalidSync { found: 0x25EB })
    );
    let mut bad = DISTINCT;
    bad[22] ^= 1;
    assert_eq!(
        Header::parse(&bad),
        Err(HeaderError::ChecksumMismatch {
            stored: 0x8A90,
            computed: 0x8A91
        })
    );
}

#[test]
fn all_flags_use_bit7_alone_for_secondary_presence() {
    for flags in 0..=255 {
        let h = Header::parse(&packed(36, 0, flags)).unwrap();
        assert_eq!(h.flags(), flags);
        assert_eq!(h.has_secondary_header(), flags & 0x80 != 0);
        assert_eq!(h.header_length(), if flags & 0x80 == 0 { 24 } else { 36 });
    }
    assert!(matches!(
        Header::parse(&packed(24, 0, 0x80)),
        Err(HeaderError::PacketTooShort {
            length: 24,
            minimum: 36
        })
    ));
}

#[test]
fn checksum_qualified_lengths_isolate_minimum_alignment_and_data_bounds() {
    // Nonzero upper words within the ordinary 524288-byte maximum.
    let wide = Header::parse(&packed(524_288, 65_537, 0)).unwrap();
    assert_eq!(wide.packet_length(), 524_288);
    assert_eq!(wide.data_length(), 65_537);
    for (flags, minimum) in [(0, 24), (0x80, 36)] {
        assert_eq!(
            Header::parse(&packed(minimum - 4, 0, flags)),
            Err(HeaderError::PacketTooShort {
                length: minimum - 4,
                minimum
            })
        );
        assert!(Header::parse(&packed(minimum, 0, flags)).is_ok());
        assert_eq!(
            Header::parse(&packed(minimum + 2, 0, flags)),
            Err(HeaderError::UnalignedPacket {
                length: minimum + 2
            })
        );
        assert!(Header::parse(&packed(minimum + 4, 4, flags)).is_ok());
        assert_eq!(
            Header::parse(&packed(minimum + 4, 5, flags)),
            Err(HeaderError::DataTooLong {
                length: 5,
                available: 4
            })
        );
    }
    assert!(matches!(
        Header::parse(&packed(0, 0, 0)),
        Err(HeaderError::PacketTooShort { .. })
    ));
    assert!(matches!(
        Header::parse(&packed(24, u32::MAX, 0)),
        Err(HeaderError::DataTooLong { .. })
    ));
    assert!(matches!(
        Header::parse(&packed(u32::MAX, 0, 0)),
        Err(HeaderError::UnalignedPacket { .. })
    ));
}

#[test]
fn primary_parse_does_not_require_body_or_read_following_bytes() {
    // parse consumes a primary header, not a packet: 24 bytes suffice even
    // when the declared secondary/body is absent. Streaming EOF is slice4.
    let b = packed(40, 4, 0x80);
    let h = Header::parse(&b).unwrap();
    let mut followed = b.to_vec();
    followed.extend_from_slice(&[0xFF; 40]);
    assert_eq!(Header::parse(&followed).unwrap(), h);
    let mut unaligned = vec![0xAA];
    unaligned.extend_from_slice(&DISTINCT);
    assert_eq!(
        Header::parse(&unaligned[1..]).unwrap().rtc_bytes(),
        [1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn all_four_length_bytes_survive_field_access() {
    // Unlike the proposed 0x04030208 length, 0x04030228 leaves room for the
    // 24-byte header plus DATA. Header-only vector, not a conforming packet.
    let h = Header::parse(&packed(0x0403_0228, 0x0403_0201, 0)).unwrap();
    assert_eq!(h.packet_length(), 0x0403_0228);
    assert_eq!(h.data_length(), 0x0403_0201);
}
