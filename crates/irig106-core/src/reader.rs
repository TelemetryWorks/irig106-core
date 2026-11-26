//! Promoted time S16b transport; partial L3-IO-002/CAP-002/REC-003/END-004.
//! No payload meaning; completion and recovery evidence are structure only.
use crate::{
    Error, ErrorKind, Header, HeaderError, PacketRanges, PoolError, Recovery, SecondaryIntegrity,
    Tail,
};
use std::io::{self, BufRead, BufReader, Read};

/// Caller-owned transport bounds; no allocation follows Packet Length.
/// ```compile_fail,E0639
/// use irig106_core::ReaderLimits;
/// fn rebuild(value: ReaderLimits) { let _ = ReaderLimits { ..value }; }
/// ```
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ReaderLimits {
    /// Fixed buffered-input capacity; must be nonzero.
    pub buffer_bytes: usize,
    /// Maximum DATA prefix retained per returned packet.
    pub capture_bytes: usize,
    /// Optional per-incident candidate-displacement cap; None searches to EOF.
    pub recovery_bytes: Option<u64>,
}
impl ReaderLimits {
    /// Default limits: 8192 buffered bytes, 65536 capture bytes, uncapped recovery.
    pub const fn new() -> Self {
        Self {
            buffer_bytes: 8192,
            capture_bytes: 65536,
            recovery_bytes: None,
        }
    }
    /// Set input-buffer capacity; zero is rejected by PacketReader::new.
    #[must_use]
    pub const fn with_buffer_bytes(mut self, bytes: usize) -> Self {
        self.buffer_bytes = bytes;
        self
    }
    /// Set the maximum retained DATA prefix.
    #[must_use]
    pub const fn with_capture_bytes(mut self, bytes: usize) -> Self {
        self.capture_bytes = bytes;
        self
    }
    /// Set or remove the per-incident search cap; None continues to EOF.
    #[must_use]
    pub const fn with_recovery_bytes(mut self, bytes: Option<u64>) -> Self {
        self.recovery_bytes = bytes;
        self
    }
}
impl Default for ReaderLimits {
    fn default() -> Self {
        Self::new()
    }
}
/// Captured prefix of DATA ONLY. Compare its length to `header.data_length()`
/// before passing it to any decoder requiring a complete body. Filler/checksum
/// bytes are drained, never exposed as data. Secondary bytes remain raw.
/// ```compile_fail,E0639
/// use irig106_core::Packet;
/// fn rebuild(value: Packet) { let _ = Packet { ..value }; }
/// ```
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Packet {
    /// Packet start offset in the input stream.
    pub offset: u64,
    /// Primary header checked by the shared framing parser.
    pub header: Header,
    /// Uninterpreted secondary header, if bit 7 is set.
    pub secondary: Option<[u8; 12]>,
    /// Raw secondary integrity evidence; mismatch does not discard this packet.
    pub secondary_integrity: Option<SecondaryIntegrity>,
    /// Checked relative DATA/filler/checksum ranges and checksum status.
    pub ranges: PacketRanges,
    /// Caller-selected DATA prefix; not necessarily the complete DATA.
    pub body_prefix: Vec<u8>,
    /// Rejected byte positions skipped before this packet.
    pub recovered_bytes: u64,
}
/// Coordinator EOF policy: cut final packets/headers are warnings; unsynced
/// padding after an established stream is information. The caller grades them.
/// ```compile_fail,E0004
/// use irig106_core::TailKind;
/// fn closed(value: TailKind) { match value { TailKind::PartialHeader => (), TailKind::TruncatedPacket => (), TailKind::Unsynchronized => () } }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TailKind {
    /// Fewer than 24 primary bytes at EOF.
    PartialHeader,
    /// Declared secondary/body/trailer crosses EOF.
    TruncatedPacket,
    /// No further valid header before EOF after an established packet.
    Unsynchronized,
}
/// One packet or a terminal tail; no diagnostic history is retained.
/// ```compile_fail,E0004
/// use irig106_core::Event;
/// fn closed(value: Event) { match value { Event::Packet(_) => (), Event::Tail { .. } => () } }
/// ```
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A completely consumed frame, with optional bounded DATA capture.
    Packet(Packet),
    /// Terminal bytes that did not produce a complete frame.
    /// ```compile_fail,E0638
    /// use irig106_core::Event;
    /// fn closed(event: Event) {
    ///     if let Event::Tail { offset: _, bytes: _, kind: _, recovery: _ } = event {}
    /// }
    /// ```
    #[non_exhaustive]
    Tail {
        /// Start of the tail.
        offset: u64,
        /// Number of consumed tail bytes.
        bytes: u64,
        /// Reason this tail was emitted.
        kind: TailKind,
        /// Recovery displacement, separate from consumed tail bytes.
        recovery: Option<Recovery>,
    },
}
#[derive(Debug)]
struct ReadAhead {
    bytes: Vec<u8>,
    used: usize,
    available: usize,
}

/// Constant transport space: buffer + 24-byte window + caller-bounded capture.
/// Returned packets belong to the caller; retaining them is its responsibility.
/// Errors are terminal. A truncated final packet is not rescanned: its consumed
/// bytes (possibly including later syncs) are reported in the tail count.
#[derive(Debug)]
pub struct PacketReader<R> {
    input: BufReader<R>,
    scratch: Option<ReadAhead>,
    limits: ReaderLimits,
    pub(crate) position: u64,
    pub(crate) established: bool,
    pub(crate) done: bool,
    pub(crate) recovery: Option<Recovery>,
}
impl<R: Read> PacketReader<R> {
    /// Construct a reader with bounded buffer/capture and optional explicit sync-search cap.
    pub fn new(input: R, limits: ReaderLimits) -> Result<Self, Error> {
        if limits.buffer_bytes == 0 {
            return Err(Error {
                reason: ErrorKind::InvalidBuffer,
                position: 0,
                recovery: None,
            });
        }
        Ok(Self {
            input: BufReader::with_capacity(limits.buffer_bytes, input),
            scratch: None,
            limits,
            position: 0,
            established: false,
            done: false,
            recovery: None,
        })
    }
    pub(crate) fn whole(
        input: R,
        recovery_bytes: Option<u64>,
        read_ahead: usize,
    ) -> Result<Self, PoolError> {
        if !(24..=isize::MAX as usize).contains(&read_ahead) {
            return Err(PoolError::InvalidRequest);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(read_ahead)?;
        if bytes.capacity() != read_ahead {
            return Err(PoolError::UnexpectedCapacity);
        }
        bytes.resize(read_ahead, 0);
        Ok(Self {
            input: BufReader::with_capacity(0, input),
            scratch: Some(ReadAhead {
                bytes,
                used: 0,
                available: 0,
            }),
            limits: ReaderLimits::new().with_recovery_bytes(recovery_bytes),
            position: 0,
            established: false,
            done: false,
            recovery: None,
        })
    }
    pub(crate) fn buffered(&self) -> usize {
        self.scratch.as_ref().map_or(0, |b| b.available - b.used)
    }
    fn refill(&mut self) -> Result<usize, ErrorKind> {
        let scratch = self.scratch.as_mut().unwrap();
        loop {
            match self.input.read(&mut scratch.bytes) {
                Ok(n) => {
                    scratch.used = 0;
                    scratch.available = n;
                    return Ok(n);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
    pub(crate) fn source(&mut self) -> &mut R {
        self.input.get_mut()
    }
    /// Consumed byte offset, excluding buffered read-ahead.
    pub fn position(&self) -> u64 {
        self.position
    }
    /// Returns the underlying reader, discarding any buffered read-ahead.
    pub fn into_inner(self) -> R {
        self.input.into_inner()
    }
    fn advance(&mut self, n: usize) -> Result<(), ErrorKind> {
        self.position = self
            .position
            .checked_add(n as u64)
            .ok_or(ErrorKind::OffsetOverflow)?;
        Ok(())
    }
    pub(crate) fn read_counted(&mut self, out: &mut [u8]) -> Result<usize, ErrorKind> {
        self.read_with(out, true)
    }
    pub(crate) fn read_body_counted(
        &mut self,
        out: &mut [u8],
        copy_body: bool,
    ) -> Result<usize, ErrorKind> {
        self.read_with(out, copy_body)
    }
    fn read_with(&mut self, out: &mut [u8], prefetch: bool) -> Result<usize, ErrorKind> {
        let mut total = 0;
        while total < out.len() {
            let buffered = if let Some(scratch) = &mut self.scratch {
                let n = (scratch.available - scratch.used).min(out.len() - total);
                out[total..total + n]
                    .copy_from_slice(&scratch.bytes[scratch.used..scratch.used + n]);
                scratch.used += n;
                n
            } else {
                0
            };
            if buffered > 0 {
                self.advance(buffered)?;
                total += buffered;
                continue;
            }
            if prefetch && self.scratch.is_some() {
                if self.refill()? == 0 {
                    break;
                }
                continue;
            }
            match self.input.read(&mut out[total..]) {
                Ok(0) => break,
                Ok(n) => {
                    self.advance(n)?;
                    total += n;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(total)
    }
    fn drain(&mut self, mut n: u64) -> Result<bool, ErrorKind> {
        while n > 0 {
            let available = match self.input.fill_buf() {
                Ok(b) => b.len(),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            if available == 0 {
                return Ok(false);
            }
            let take = available.min(n.try_into().unwrap_or(usize::MAX));
            self.input.consume(take);
            self.advance(take)?;
            n -= take as u64;
        }
        Ok(true)
    }
    pub(crate) fn tail(&mut self, offset: u64, kind: TailKind) -> Result<Option<Event>, ErrorKind> {
        self.done = true;
        let bytes = self.position - offset;
        if !self.established {
            return Err(ErrorKind::NoCompletePacket {
                tail: Some(Tail {
                    offset,
                    bytes,
                    kind,
                }),
            });
        }
        Ok(Some(Event::Tail {
            offset,
            bytes,
            kind,
            recovery: self.recovery,
        }))
    }
    /// Capture at most this many DATA bytes, selected after header validation.
    /// Requests are clamped to Data Length, then checked against `capture_bytes`.
    /// EOF without a complete packet yields NoCompletePacket. Other failures
    /// preserve typed causes; every error/tail is terminal, followed by None.
    pub fn next_packet(
        &mut self,
        capture: impl FnOnce(&Header) -> usize,
    ) -> Result<Option<Event>, Error> {
        if self.done {
            return Ok(None);
        }
        self.recovery = None;
        self.next_inner(capture).map_err(|reason| {
            self.done = true;
            Error {
                reason,
                position: self.position,
                recovery: self.recovery,
            }
        })
    }
    pub(crate) fn find_header(&mut self) -> Result<FrameStart, ErrorKind> {
        let start = self.position;
        let mut b = [0; 24];
        match self.read_counted(&mut b)? {
            0 => {
                self.done = true;
                return if self.established {
                    Ok(FrameStart::End(None))
                } else {
                    Err(ErrorKind::NoCompletePacket { tail: None })
                };
            }
            24 => (),
            _ => {
                return self
                    .tail(start, TailKind::PartialHeader)
                    .map(FrameStart::End);
            }
        }
        // Reuse the primary parser: only sync/checksum failures search forward.
        let header = loop {
            match Header::parse(&b) {
                Ok(header) => break header,
                Err(HeaderError::InvalidSync { .. } | HeaderError::ChecksumMismatch { .. }) => (),
                Err(error) => return Err(ErrorKind::Header { source: error }),
            }
            let recovery = self.recovery.get_or_insert(Recovery {
                start_offset: start,
                scanned_bytes: 0,
            });
            if let Some(limit) = self.limits.recovery_bytes {
                if recovery.scanned_bytes >= limit {
                    return Err(ErrorKind::RecoveryLimit { limit });
                }
            }
            b.copy_within(1.., 0);
            if self.read_counted(&mut b[23..])? == 0 {
                return self
                    .tail(start, TailKind::Unsynchronized)
                    .map(FrameStart::End);
            }
            if let Some(recovery) = &mut self.recovery {
                recovery.scanned_bytes += 1;
            }
        };
        let offset = self.position - 24;
        let ranges =
            PacketRanges::from_header(&header).map_err(|source| ErrorKind::Header { source })?;
        Ok(FrameStart::Header {
            offset,
            header,
            ranges,
        })
    }
    fn next_inner(
        &mut self,
        capture: impl FnOnce(&Header) -> usize,
    ) -> Result<Option<Event>, ErrorKind> {
        let (offset, header, ranges) = match self.find_header()? {
            FrameStart::Header {
                offset,
                header,
                ranges,
            } => (offset, header, ranges),
            FrameStart::End(event) => return Ok(event),
        };
        let start = self.recovery.map_or(offset, |r| r.start_offset);
        let wanted = capture(&header).min(header.data_length() as usize);
        if wanted > self.limits.capture_bytes {
            return Err(ErrorKind::CaptureLimit {
                requested: wanted,
                limit: self.limits.capture_bytes,
            });
        }
        let secondary = if header.has_secondary_header() {
            let mut raw = [0; 12];
            if self.read_counted(&mut raw)? != 12 {
                return self.tail(offset, TailKind::TruncatedPacket);
            }
            Some(raw)
        } else {
            None
        };
        let mut body_prefix = Vec::new();
        body_prefix
            .try_reserve_exact(wanted)
            .map_err(|source| ErrorKind::Allocation { source })?;
        body_prefix.resize(wanted, 0);
        if self.read_counted(&mut body_prefix)? != wanted
            || !self
                .drain(u64::from(header.packet_length() - header.header_length()) - wanted as u64)?
        {
            return self.tail(offset, TailKind::TruncatedPacket);
        }
        self.established = true;
        Ok(Some(Event::Packet(Packet {
            offset,
            header,
            secondary_integrity: secondary.as_ref().map(SecondaryIntegrity::inspect),
            secondary,
            ranges,
            body_prefix,
            recovered_bytes: offset - start,
        })))
    }
}

impl std::fmt::Display for TailKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::PartialHeader => "partial header",
            Self::TruncatedPacket => "truncated packet",
            Self::Unsynchronized => "unsynchronized data",
        })
    }
}

pub(crate) enum FrameStart {
    Header {
        offset: u64,
        header: Header,
        ranges: PacketRanges,
    },
    End(Option<Event>),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_read_ahead_minimum_is_one_complete_primary_header() {
        assert!(matches!(
            PacketReader::whole(&[][..], None, 23),
            Err(PoolError::InvalidRequest)
        ));
        assert!(PacketReader::whole(&[][..], None, 24).is_ok());
    }
    #[test]
    fn offset_overflow_is_typed_without_wrapping() {
        let mut reader = PacketReader::new(&[][..], ReaderLimits::default()).unwrap();
        reader.position = u64::MAX;
        assert!(matches!(reader.advance(1), Err(ErrorKind::OffsetOverflow)));
        assert_eq!(reader.position(), u64::MAX);
    }
}
