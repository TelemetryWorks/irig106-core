//! Typed reader failures and bounded terminal evidence.
use crate::{HeaderError, TailKind};
use std::{collections::TryReserveError, error::Error as StdError, fmt, io};

/// One recovery incident; the 24-byte validation window is not charged.
/// ```compile_fail,E0639
/// use irig106_core::Recovery;
/// fn rebuild(value: Recovery) { let _ = Recovery { ..value }; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Recovery {
    /// Expected packet offset where sync/checksum search began.
    pub start_offset: u64,
    /// Candidate displacement reached, equal to skipped bytes on success.
    pub scanned_bytes: u64,
}

/// Terminal bytes preserved when EOF occurs before any complete packet.
/// ```compile_fail,E0639
/// use irig106_core::Tail;
/// fn rebuild(value: Tail) { let _ = Tail { ..value }; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tail {
    /// Partial header/packet or unsynchronized region start.
    pub offset: u64,
    /// Consumed bytes from that start, independent of recovery displacement.
    pub bytes: u64,
    /// How input ended.
    pub kind: TailKind,
}

/// Typed failure reason; EOF completion does not mask I/O or structural errors.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Original operational read failure.
    Io(io::Error),
    /// Checksum-qualified impossible primary framing.
    Header(HeaderError),
    /// EOF without a single completely consumed frame.
    NoCompletePacket {
        /// Available tail evidence, absent for empty input.
        tail: Option<Tail>,
    },
    /// Caller's inclusive candidate-displacement budget was exhausted.
    RecoveryLimit {
        /// Explicit caller cap; never a default limit.
        limit: u64,
    },
    /// Zero buffered-input capacity.
    InvalidBuffer,
    /// Requested DATA prefix exceeds the caller's capture bound.
    CaptureLimit {
        /// Requested size after clamping to Data Length.
        requested: usize,
        /// Caller capture bound.
        limit: usize,
    },
    /// Bounded DATA capture allocation failed.
    Allocation(TryReserveError),
    /// Advancing the consumed byte offset would exceed u64.
    OffsetOverflow,
}

impl From<io::Error> for ErrorKind {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Terminal reader failure with exact consumed position and recovery context.
/// ```compile_fail,E0639
/// use irig106_core::Error;
/// fn rebuild(value: Error) { let _ = Error { ..value }; }
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub struct Error {
    /// Typed cause; no error-message matching is required.
    pub reason: ErrorKind,
    /// Consumed byte position, excluding buffered read-ahead.
    pub position: u64,
    /// Current recovery incident, including on I/O/structural failure or EOF.
    pub recovery: Option<Recovery>,
}
impl Error {
    /// Broad I/O classification retained for callers of the promoted seed.
    pub fn kind(&self) -> io::ErrorKind {
        match &self.reason {
            ErrorKind::Io(error) => error.kind(),
            ErrorKind::InvalidBuffer | ErrorKind::CaptureLimit { .. } => {
                io::ErrorKind::InvalidInput
            }
            ErrorKind::Allocation(_) => io::ErrorKind::Other,
            _ => io::ErrorKind::InvalidData,
        }
    }
    /// Underlying typed header, I/O or allocation error, when present.
    pub fn get_ref(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        match &self.reason {
            ErrorKind::Io(error) => Some(error),
            ErrorKind::Header(error) => Some(error),
            ErrorKind::Allocation(error) => Some(error),
            _ => None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "packet reader at {}: {}", self.position, self.reason)
    }
}
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.get_ref().map(|error| error as _)
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "input read failed: {error}"),
            Self::Header(error) => write!(f, "invalid packet framing: {error}"),
            Self::NoCompletePacket { .. } => f.write_str("no complete Chapter 10 packet found"),
            Self::RecoveryLimit { limit } => {
                write!(f, "recovery search exhausted caller limit of {limit} bytes")
            }
            Self::InvalidBuffer => f.write_str("reader buffer must contain at least one byte"),
            Self::CaptureLimit { requested, limit } => write!(
                f,
                "requested DATA prefix {requested} bytes exceeds capture limit {limit} bytes"
            ),
            Self::Allocation(error) => write!(f, "could not allocate DATA prefix: {error}"),
            Self::OffsetOverflow => f.write_str("input byte offset exceeds u64 range"),
        }
    }
}
