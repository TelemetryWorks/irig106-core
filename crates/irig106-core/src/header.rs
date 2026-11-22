//! Primary framing (106-24R1 §11.2.1.1, Table 11-2).
use crate::Rtc;
use std::fmt;

/// Primary header size in bytes, excluding any secondary header.
pub const PRIMARY_HEADER_LEN: usize = 24;

/// Sum the eleven little-endian words before the stored checksum, modulo 65536.
///
/// Does not validate sync or lengths. The fixed input size prevents short-input
/// indexing; the stored checksum word is excluded (§11.2.1.1 j).
/// **Traces:** L3-HDR-001 ← L2-HDR-001 ← L1-CORE-001.
pub fn primary_checksum(bytes: &[u8; PRIMARY_HEADER_LEN]) -> u16 {
    bytes[..22].chunks_exact(2).fold(0u16, |sum, word| {
        sum.wrapping_add(u16::from_le_bytes([word[0], word[1]]))
    })
}

/// Primary bytes with sync, checksum and basic framing lengths checked.
///
/// Immutable raw bytes preserve unknown routing codes. This is not evidence of
/// a complete packet, valid contents, or complete standard conformance.
/// **Traces:** L3-HDR-001, L3-LEN-001, raw-code clause of L3-EDN-005.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    bytes: [u8; PRIMARY_HEADER_LEN],
}

impl Header {
    /// Parse only the first 24 bytes, ignoring any following secondary/body bytes.
    ///
    /// Checks sync, then primary checksum, then minimum/alignment/DATA bounds.
    /// No allocation or body access occurs. In particular, header success does
    /// not establish stream input; EOF/completion is the future reader's job.
    /// Trailer checksum space and other conformance rules are not checked here.
    /// **Traces:** L3-HDR-001 ← L2-HDR-001; L3-LEN-001 ← L2-LEN-001.
    pub fn parse(input: &[u8]) -> Result<Self, HeaderError> {
        let bytes: [u8; PRIMARY_HEADER_LEN] = input
            .get(..PRIMARY_HEADER_LEN)
            .and_then(|s| s.try_into().ok())
            .ok_or(HeaderError::TooShort {
                actual: input.len(),
            })?;
        let found = u16::from_le_bytes([bytes[0], bytes[1]]);
        if found != 0xEB25 {
            return Err(HeaderError::InvalidSync { found });
        }
        let stored = u16::from_le_bytes([bytes[22], bytes[23]]);
        let computed = primary_checksum(&bytes);
        if stored != computed {
            return Err(HeaderError::ChecksumMismatch { stored, computed });
        }
        let header = Self { bytes };
        let length = header.packet_length();
        let minimum = header.header_length();
        if length < minimum {
            return Err(HeaderError::PacketTooShort { length, minimum });
        }
        if length % 4 != 0 {
            return Err(HeaderError::UnalignedPacket { length });
        }
        // Minimum checked first, so subtraction cannot wrap or underflow.
        let available = length - minimum;
        if header.data_length() > available {
            return Err(HeaderError::DataTooLong {
                length: header.data_length(),
                available,
            });
        }
        Ok(header)
    }

    /// Original primary bytes, including the stored checksum.
    pub fn as_bytes(&self) -> &[u8; PRIMARY_HEADER_LEN] {
        &self.bytes
    }
    /// Raw channel identifier; no multiplexer/source interpretation.
    pub fn channel(&self) -> u16 {
        u16::from_le_bytes([self.bytes[2], self.bytes[3]])
    }
    /// Declared complete packet size, not the number of supplied bytes.
    pub fn packet_length(&self) -> u32 {
        u32::from_le_bytes([self.bytes[4], self.bytes[5], self.bytes[6], self.bytes[7]])
    }
    /// Declared DATA size, excluding trailer filler/checksum.
    pub fn data_length(&self) -> u32 {
        u32::from_le_bytes([self.bytes[8], self.bytes[9], self.bytes[10], self.bytes[11]])
    }
    /// Raw data-type layout version; never an inferred recording edition.
    pub fn data_type_version(&self) -> u8 {
        self.bytes[12]
    }
    /// Raw per-channel sequence number; continuity is checked downstream.
    pub fn sequence(&self) -> u8 {
        self.bytes[13]
    }
    /// All packet flags, including reserved/unsupported combinations.
    pub fn flags(&self) -> u8 {
        self.bytes[14]
    }
    /// Raw routing data type; unknown codes remain unchanged.
    pub fn data_type(&self) -> u8 {
        self.bytes[15]
    }
    /// Six wire RTC bytes, before conversion to the shared counter type.
    pub fn rtc_bytes(&self) -> [u8; 6] {
        let b = &self.bytes;
        [b[16], b[17], b[18], b[19], b[20], b[21]]
    }
    /// Shared 48-bit counter; no calendar or session inference.
    pub fn rtc(&self) -> Rtc {
        Rtc::from_le_bytes(self.rtc_bytes())
    }
    /// Bit 7 alone selects the optional twelve-byte secondary header.
    pub fn has_secondary_header(&self) -> bool {
        self.flags() & 0x80 != 0
    }
    /// Combined primary/secondary size declared by the presence flag (24 or 36).
    pub fn header_length(&self) -> u32 {
        if self.has_secondary_header() { 36 } else { 24 }
    }
}

/// Primary framing failure; stream recovery/classification belongs to the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HeaderError {
    /// Fewer than 24 primary bytes were supplied.
    TooShort {
        /// Supplied size.
        actual: usize,
    },
    /// The leading little-endian word was not EB25.
    InvalidSync {
        /// Observed word.
        found: u16,
    },
    /// Stored and computed primary sums disagree.
    ChecksumMismatch {
        /// Sum in bytes 22–23.
        stored: u16,
        /// Sum of the eleven preceding words.
        computed: u16,
    },
    /// Packet Length cannot contain its declared headers.
    PacketTooShort {
        /// Declared Packet Length.
        length: u32,
        /// Required primary/secondary size.
        minimum: u32,
    },
    /// Packet Length is not divisible by four.
    UnalignedPacket {
        /// Declared Packet Length.
        length: u32,
    },
    /// DATA extends beyond space following the declared headers.
    DataTooLong {
        /// Declared Data Length.
        length: u32,
        /// Space remaining after primary/secondary headers, before trailer checks.
        available: u32,
    },
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { actual } => write!(f, "primary header needs 24 bytes, got {actual}"),
            Self::InvalidSync { found } => write!(f, "invalid primary sync {found:#06X}"),
            Self::ChecksumMismatch { stored, computed } => write!(
                f,
                "primary checksum {stored:#06X}, expected {computed:#06X}"
            ),
            Self::PacketTooShort { length, minimum } => {
                write!(f, "packet length {length} is below header size {minimum}")
            }
            Self::UnalignedPacket { length } => {
                write!(f, "packet length {length} is not divisible by four")
            }
            Self::DataTooLong { length, available } => write!(
                f,
                "data length {length} exceeds available space {available}"
            ),
        }
    }
}

impl std::error::Error for HeaderError {}
