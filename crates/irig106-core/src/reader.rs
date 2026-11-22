//! Promoted time S16b transport; partial L3-IO-002/CAP-002/REC-003/END-004.
//! No payload meaning. Typed completion/cap refinements are slice 4.
use crate::{Header, HeaderError};
use std::io::{self, BufRead, BufReader, Read};

/// Caller-owned transport bounds; no allocation follows Packet Length.
#[derive(Debug, Clone, Copy)]
pub struct ReaderLimits {
    /// Fixed buffered-input capacity; must be nonzero.
    pub buffer_bytes: usize,
    /// Maximum DATA prefix retained per returned packet.
    pub capture_bytes: usize,
}
impl Default for ReaderLimits {
    fn default() -> Self {
        Self {
            buffer_bytes: 8192,
            capture_bytes: 65536,
        }
    }
}
/// Captured prefix of DATA ONLY. Compare its length to `header.data_length()`
/// before passing it to any decoder requiring a complete body. Filler/checksum
/// bytes are drained, never exposed as data. Secondary bytes remain raw.
#[derive(Debug, PartialEq, Eq)]
pub struct Packet {
    /// Packet start offset in the input stream.
    pub offset: u64,
    /// Primary header checked by the shared framing parser.
    pub header: Header,
    /// Uninterpreted secondary header, if bit 7 is set.
    pub secondary: Option<[u8; 12]>,
    /// Caller-selected DATA prefix; not necessarily the complete DATA.
    pub body_prefix: Vec<u8>,
    /// Rejected byte positions skipped before this packet.
    pub recovered_bytes: u64,
}
/// Coordinator EOF policy: cut final packets/headers are warnings; unsynced
/// padding after an established stream is information. The caller grades them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailKind {
    /// Fewer than 24 primary bytes at EOF.
    PartialHeader,
    /// Declared secondary/body/trailer crosses EOF.
    TruncatedPacket,
    /// No further valid header before EOF after an established packet.
    Unsynchronized,
}
/// One packet or a terminal tail; no diagnostic history is retained.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// A completely consumed frame, with optional bounded DATA capture.
    Packet(Packet),
    /// Terminal bytes that did not produce a complete frame.
    Tail {
        /// Start of the tail.
        offset: u64,
        /// Number of consumed tail bytes.
        bytes: u64,
        /// Reason this tail was emitted.
        kind: TailKind,
    },
}
/// Constant transport space: buffer + 24-byte window + caller-bounded capture.
/// Returned packets belong to the caller; retaining them is its responsibility.
/// Errors are terminal. A truncated final packet is not rescanned: its consumed
/// bytes (possibly including later syncs) are reported in the tail count.
#[derive(Debug)]
pub struct PacketReader<R> {
    input: BufReader<R>,
    limits: ReaderLimits,
    position: u64,
    established: bool,
    done: bool,
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
impl<R: Read> PacketReader<R> {
    /// Construct a reader with bounded buffer/capture and uncapped sync search.
    pub fn new(input: R, limits: ReaderLimits) -> io::Result<Self> {
        if limits.buffer_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "zero reader buffer",
            ));
        }
        Ok(Self {
            input: BufReader::with_capacity(limits.buffer_bytes, input),
            limits,
            position: 0,
            established: false,
            done: false,
        })
    }
    /// Consumed byte offset, excluding buffered read-ahead.
    pub fn position(&self) -> u64 {
        self.position
    }
    /// Returns the underlying reader, discarding any buffered read-ahead.
    pub fn into_inner(self) -> R {
        self.input.into_inner()
    }
    fn advance(&mut self, n: usize) -> io::Result<()> {
        self.position = self
            .position
            .checked_add(n as u64)
            .ok_or_else(|| invalid("file offset overflow"))?;
        Ok(())
    }
    fn read_counted(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut total = 0;
        while total < out.len() {
            match self.input.read(&mut out[total..]) {
                Ok(0) => break,
                Ok(n) => {
                    self.advance(n)?;
                    total += n;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(total)
    }
    fn drain(&mut self, mut n: u64) -> io::Result<bool> {
        while n > 0 {
            let available = match self.input.fill_buf() {
                Ok(b) => b.len(),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
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
    fn tail(&mut self, offset: u64, kind: TailKind) -> Event {
        self.done = true;
        Event::Tail {
            offset,
            bytes: self.position - offset,
            kind,
        }
    }
    /// Capture at most this many DATA bytes, selected after header validation.
    /// Requests are clamped to Data Length, then checked against `capture_bytes`.
    /// Invalid framing/no established sync is InvalidData; exceeding the caller's
    /// capture cap is InvalidInput. Read errors retain their original kind.
    pub fn next_packet(
        &mut self,
        capture: impl FnOnce(&Header) -> usize,
    ) -> io::Result<Option<Event>> {
        if self.done {
            return Ok(None);
        }
        let result = self.next_inner(capture);
        if result.is_err() {
            self.done = true;
        }
        result
    }
    fn next_inner(&mut self, capture: impl FnOnce(&Header) -> usize) -> io::Result<Option<Event>> {
        let start = self.position;
        let mut b = [0; 24];
        match self.read_counted(&mut b)? {
            0 => {
                self.done = true;
                return Ok(None);
            }
            24 => (),
            _ => return Ok(Some(self.tail(start, TailKind::PartialHeader))),
        }
        // Reuse the primary parser: only sync/checksum failures search forward.
        let header = loop {
            match Header::parse(&b) {
                Ok(header) => break header,
                Err(HeaderError::InvalidSync { .. } | HeaderError::ChecksumMismatch { .. }) => (),
                Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
            }
            b.copy_within(1.., 0);
            if self.read_counted(&mut b[23..])? == 0 {
                return if self.established {
                    Ok(Some(self.tail(start, TailKind::Unsynchronized)))
                } else {
                    Err(invalid("no valid Chapter 10 sync/header"))
                };
            }
        };
        let offset = self.position - 24;
        let wanted = capture(&header).min(header.data_length() as usize);
        if wanted > self.limits.capture_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "data capture exceeds caller cap",
            ));
        }
        let secondary = if header.has_secondary_header() {
            let mut raw = [0; 12];
            if self.read_counted(&mut raw)? != 12 {
                return Ok(Some(self.tail(offset, TailKind::TruncatedPacket)));
            }
            Some(raw)
        } else {
            None
        };
        let mut body_prefix = Vec::new();
        body_prefix
            .try_reserve_exact(wanted)
            .map_err(|_| io::Error::other("data capture allocation failed"))?;
        body_prefix.resize(wanted, 0);
        if self.read_counted(&mut body_prefix)? != wanted
            || !self
                .drain(u64::from(header.packet_length() - header.header_length()) - wanted as u64)?
        {
            return Ok(Some(self.tail(offset, TailKind::TruncatedPacket)));
        }
        self.established = true;
        Ok(Some(Event::Packet(Packet {
            offset,
            header,
            secondary,
            body_prefix,
            recovered_bytes: offset - start,
        })))
    }
}
