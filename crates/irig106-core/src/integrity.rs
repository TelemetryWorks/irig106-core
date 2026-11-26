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

/// Packet-data integrity evidence; mismatch never rejects a frame by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataChecksumState {
    /// Flags declare no checksum.
    Absent,
    /// Space is checked, contents are not checked (including zero checksum bytes).
    NotChecked,
    /// Stored and computed arithmetic sums agree; not payload authenticity.
    #[non_exhaustive]
    Verified {
        /// Checksum width in bytes (1, 2 or 4).
        width: u8,
        /// Matching unsigned sum.
        value: u32,
    },
    /// Complete frame delivered with differing stored and computed sums.
    #[non_exhaustive]
    Mismatch {
        /// Checksum width in bytes (1, 2 or 4).
        width: u8,
        /// Stored little-endian unsigned value.
        stored: u32,
        /// Computed arithmetic sum modulo the selected width.
        computed: u32,
    },
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
    /// Checksum evidence; from_header alone cannot verify contents.
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

// Fixed carry supports arbitrary arriving-span boundaries, without copies of
// payloads or a second traversal of an assembled packet.
pub(crate) struct DataChecksum {
    width: usize,
    sum: u32,
    pending: [u8; 4],
    used: usize,
}
impl DataChecksum {
    pub(crate) fn new(width: usize) -> Self {
        Self {
            width,
            sum: 0,
            pending: [0; 4],
            used: 0,
        }
    }
    pub(crate) fn update(&mut self, mut bytes: &[u8]) {
        if self.used > 0 {
            let n = (self.width - self.used).min(bytes.len());
            self.pending[self.used..self.used + n].copy_from_slice(&bytes[..n]);
            self.used += n;
            bytes = &bytes[n..];
            if self.used < self.width {
                return;
            }
            self.sum = self.sum.wrapping_add(u32::from_le_bytes(self.pending));
            self.pending = [0; 4];
            self.used = 0;
        }
        self.sum = match self.width {
            1 => bytes
                .iter()
                .fold(self.sum, |s, &b| s.wrapping_add(u32::from(b))),
            2 => bytes.chunks_exact(2).fold(self.sum, |s, w| {
                s.wrapping_add(u32::from(u16::from_le_bytes([w[0], w[1]])))
            }),
            4 => bytes.chunks_exact(4).fold(self.sum, |s, w| {
                s.wrapping_add(u32::from_le_bytes(w.try_into().unwrap()))
            }),
            _ => unreachable!(),
        };
        let remainder = bytes.len() % self.width;
        self.pending[..remainder].copy_from_slice(&bytes[bytes.len() - remainder..]);
        self.used = remainder;
    }
    pub(crate) fn finish(self, stored: &[u8]) -> DataChecksumState {
        // Header and checksum alignment guarantee complete words at packet end.
        debug_assert_eq!(self.used, 0);
        let mut raw = [0; 4];
        raw[..self.width].copy_from_slice(stored);
        let stored = u32::from_le_bytes(raw);
        let computed = self.sum & (u32::MAX >> (32 - self.width * 8));
        if stored == computed {
            DataChecksumState::Verified {
                width: self.width as u8,
                value: computed,
            }
        } else {
            DataChecksumState::Mismatch {
                width: self.width as u8,
                stored,
                computed,
            }
        }
    }
}
#[cfg(test)]
mod data_checksum_tests {
    use super::*;
    #[test]
    fn carry_is_independent_of_arrival_boundaries() {
        // Independent literal sums: bytes 1..=12 => 78, words => 2A24,
        // longwords 04030201 + 08070605 + 0C0B0A09 => 1815120F.
        for (width, sum) in [(1, 78u32), (2, 0x2A24), (4, 0x1815120F)] {
            for quantum in 1..=13 {
                let mut c = DataChecksum::new(width);
                for bytes in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].chunks(quantum) {
                    c.update(bytes);
                }
                assert!(
                    matches!(c.finish(&sum.to_le_bytes()[..width]), DataChecksumState::Verified{value,..} if value==sum)
                );
            }
        }
    }
}
