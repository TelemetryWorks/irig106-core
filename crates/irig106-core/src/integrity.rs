//! Structural integrity evidence; no payload/time interpretation.
use crate::{Header, HeaderError};
use std::ops::Range;

/// Raw secondary checksum/reserved evidence (106-24R1 §11.2.1.2 c).
/// RDG-008 selects the byte sum but retains the suspect alternative reading.
/// ```compile_fail,E0639
/// use irig106_core::SecondaryIntegrity;
/// fn rebuild(value: SecondaryIntegrity) { let _ = SecondaryIntegrity { ..value }; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecondaryIntegrity {
    /// Stored little-endian checksum.
    pub stored: u16,
    /// Arithmetic sum of the first ten bytes.
    pub byte_sum: u16,
    /// Alternative sum of five little-endian words, modulo 65536.
    pub word_sum: u16,
    /// Raw reserved word at bytes 8–9; nonzero is separate evidence.
    pub reserved: u16,
}
impl SecondaryIntegrity {
    /// Inspect exactly twelve bytes, excluding the stored word from both sums.
    pub fn inspect(raw: &[u8; 12]) -> Self {
        Self {
            stored: u16::from_le_bytes([raw[10], raw[11]]),
            byte_sum: raw[..10].iter().map(|&b| u16::from(b)).sum(),
            word_sum: raw[..10].chunks_exact(2).fold(0u16, |s, w| {
                s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
            }),
            reserved: u16::from_le_bytes([raw[8], raw[9]]),
        }
    }
    /// Whether the selected byte reading matches; not time/payload validity.
    pub fn byte_valid(&self) -> bool {
        self.stored == self.byte_sum
    }
    /// Whether the alternative word reading matches; never silently selected.
    pub fn word_valid(&self) -> bool {
        self.stored == self.word_sum
    }
}

/// Packet-data checksum contents are deliberately not validated in 0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataChecksumState {
    /// Flags declare no checksum.
    Absent,
    /// Space is checked, contents are not checked (including zero checksum bytes).
    NotChecked,
}

/// Disjoint byte ranges relative to the packet start, not absolute file offsets.
/// Variable filler is permitted (§11.2.1.4); its contents are not validated.
/// ```compile_fail,E0639
/// use irig106_core::PacketRanges;
/// fn rebuild(value: PacketRanges) { let _ = PacketRanges { ..value }; }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PacketRanges {
    /// DATA only, excluding all filler/checksum bytes.
    pub data: Range<u32>,
    /// Filler between DATA and the checksum (possibly empty).
    pub filler: Range<u32>,
    /// Final 0/1/2/4 checksum bytes, selected by flags bits 1–0.
    pub checksum: Range<u32>,
    /// Explicitly absent or unvalidated contents.
    pub data_checksum: DataChecksumState,
}
impl PacketRanges {
    /// Check trailer space after primary framing; does not require packet bytes.
    pub fn from_header(header: &Header) -> Result<Self, HeaderError> {
        let width = match header.flags() & 3 {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        let minimum = header.header_length() + width;
        let available =
            header
                .packet_length()
                .checked_sub(minimum)
                .ok_or(HeaderError::PacketTooShort {
                    length: header.packet_length(),
                    minimum,
                })?;
        if header.data_length() > available {
            return Err(HeaderError::DataTooLong {
                length: header.data_length(),
                available,
            });
        }
        // Bounds above prove these additions/subtractions cannot wrap.
        let data = header.header_length()..header.header_length() + header.data_length();
        let checksum = header.packet_length() - width..header.packet_length();
        Ok(Self {
            filler: data.end..checksum.start,
            data,
            checksum,
            data_checksum: if width == 0 {
                DataChecksumState::Absent
            } else {
                DataChecksumState::NotChecked
            },
        })
    }
}
