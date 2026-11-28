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
    sum: u64,
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
            self.sum = self
                .sum
                .wrapping_add(u64::from(u32::from_le_bytes(self.pending)));
            self.pending = [0; 4];
            self.used = 0;
        }
        let sum = match self.width {
            1 => byte_sum(bytes),
            2 => word_sum(bytes),
            4 => longword_sum(bytes),
            _ => unreachable!(),
        };
        self.sum = self.sum.wrapping_add(sum);
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
        let computed = (self.sum & (u64::MAX >> (64 - self.width * 8))) as u32;
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
// Independent modular lanes over 128-byte groups avoid widening every input
// element. Each lane is congruent to its mathematical wide sum modulo the
// selected width; the final horizontal reduction therefore has the same low
// bits. Fixed lane counts allow LLVM to use vector loads/adds. Byte decoding is
// alignment-independent; partial checksum words remain in update's fixed carry.
macro_rules! lane_sum {
    ($name:ident, $word:ty, $width:expr) => {
        fn $name(bytes: &[u8]) -> u64 {
            // Short spans avoid lane initialization/horizontal-reduction cost.
            if bytes.len() < 1024 {
                return bytes.chunks_exact($width).fold(0u64, |s, word| {
                    s.wrapping_add(u64::from(<$word>::from_le_bytes(word.try_into().unwrap())))
                });
            }
            // Keep checksum-word boundaries even when source alignment cannot
            // be reached (e.g. a u16 word stored at an odd address).
            let prefix = bytes.as_ptr().align_offset(64).min(bytes.len()) / $width * $width;
            let (head, bytes) = bytes.split_at(prefix);
            let mut sum = head.chunks_exact($width).fold(0u64, |s, word| {
                s.wrapping_add(u64::from(<$word>::from_le_bytes(word.try_into().unwrap())))
            });
            let mut lanes = [0 as $word; 128 / $width];
            let mut groups = bytes.chunks_exact(128);
            for group in &mut groups {
                for (lane, word) in lanes.iter_mut().zip(group.chunks_exact($width)) {
                    *lane = lane.wrapping_add(<$word>::from_le_bytes(word.try_into().unwrap()));
                }
            }
            sum = lanes
                .into_iter()
                .fold(sum, |s, v| s.wrapping_add(u64::from(v)));
            for word in groups.remainder().chunks_exact($width) {
                sum = sum.wrapping_add(u64::from(<$word>::from_le_bytes(word.try_into().unwrap())));
            }
            sum
        }
    };
}
lane_sum!(byte_sum, u8, 1);
lane_sum!(word_sum, u16, 2);
lane_sum!(longword_sum, u32, 4);
#[cfg(test)]
mod data_checksum_tests {
    use super::*;
    #[test]
    fn randomized_differential_over_unaligned_spans_and_odd_tails() {
        fn random(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }
        let mut seed = 0x1062_0240_0c5e_cafe;
        for width in [1, 2, 4] {
            for case in 0..300 {
                let length = match case % 5 {
                    0 => case,
                    1 => 4096 + case,
                    2 => crate::BLOCK_BYTES + case,
                    _ => random(&mut seed) as usize % 8192,
                } / width
                    * width;
                let offset = case % 32;
                let mut storage = vec![0; length + offset];
                for b in &mut storage {
                    *b = if case % 11 == 0 {
                        0xff
                    } else {
                        random(&mut seed) as u8
                    };
                }
                let bytes = &storage[offset..];
                // Naive weighted-byte oracle deliberately differs from word
                // decoding, lane grouping and production carry logic.
                let sum = bytes.iter().enumerate().fold(0u64, |s, (i, &b)| {
                    s.wrapping_add(u64::from(b) << (8 * (i % width)))
                });
                let expected = (sum & (u64::MAX >> (64 - width * 8))) as u32;
                let mut c = DataChecksum::new(width);
                let mut at = 0;
                while at < length {
                    c.update(&[]);
                    let n = (1 + random(&mut seed) as usize % (1 + case * 17)).min(length - at);
                    c.update(&bytes[at..at + n]);
                    at += n;
                }
                assert!(
                    matches!(c.finish(&expected.to_le_bytes()[..width]),
                    DataChecksumState::Verified{value,..} if value==expected),
                    "case={case} width={width} length={length} offset={offset}"
                );
            }
        }
    }
    #[test]
    fn maximum_lane_values_wrap_without_cross_lane_carries() {
        for width in [1, 2, 4] {
            let bytes = vec![0xff; 1_048_576];
            let mut c = DataChecksum::new(width);
            c.update(&bytes);
            let expected =
                (0u32.wrapping_sub((bytes.len() / width) as u32)) & (u32::MAX >> (32 - width * 8));
            assert!(matches!(c.finish(&expected.to_le_bytes()[..width]),
                DataChecksumState::Verified{value,..} if value==expected));
        }
    }
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
