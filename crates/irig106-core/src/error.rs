//! Typed reader failures and bounded terminal evidence.
use crate::{AdmissionError, HeaderError, PoolError, TailKind};
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
    /// Whole-packet size/stability admission failure.
    #[non_exhaustive]
    Admission {
        /// Typed policy evidence.
        source: AdmissionError,
    },
    /// Shared whole-packet storage failure.
    #[non_exhaustive]
    Pool {
        /// Typed budget/allocation/wait cause.
        source: PoolError,
    },
    /// Original operational read failure.
    /// ```compile_fail,E0638
    /// use irig106_core::ErrorKind;
    /// fn closed(reason: ErrorKind) { if let ErrorKind::Io { source: _ } = reason {} }
    /// ```
    #[non_exhaustive]
    Io {
        /// Original typed cause.
        source: io::Error,
    },
    /// Checksum-qualified impossible primary framing.
    /// ```compile_fail,E0638
    /// use irig106_core::ErrorKind;
    /// fn closed(reason: ErrorKind) { if let ErrorKind::Header { source: _ } = reason {} }
    /// ```
    #[non_exhaustive]
    Header {
        /// Original typed cause.
        source: HeaderError,
    },
    /// EOF without a single completely consumed frame.
    /// ```compile_fail,E0638
    /// use irig106_core::ErrorKind;
    /// fn closed(reason: ErrorKind) { if let ErrorKind::NoCompletePacket { tail: _ } = reason {} }
    /// ```
    #[non_exhaustive]
    NoCompletePacket {
        /// Available tail evidence, absent for empty input.
        tail: Option<Tail>,
    },
    /// Caller's inclusive candidate-displacement budget was exhausted.
    /// ```compile_fail,E0638
    /// use irig106_core::ErrorKind;
    /// fn closed(reason: ErrorKind) { if let ErrorKind::RecoveryLimit { limit: _ } = reason {} }
    /// ```
    #[non_exhaustive]
    RecoveryLimit {
        /// Explicit caller cap; never a default limit.
        limit: u64,
    },
    /// Zero buffered-input capacity.
    InvalidBuffer,
    /// Requested DATA prefix exceeds the caller's capture bound.
    /// ```compile_fail,E0638
    /// use irig106_core::ErrorKind;
    /// fn closed(reason: ErrorKind) { if let ErrorKind::CaptureLimit { requested: _, limit: _ } = reason {} }
    /// ```
    #[non_exhaustive]
    CaptureLimit {
        /// Requested size after clamping to Data Length.
        requested: usize,
        /// Caller capture bound.
        limit: usize,
    },
    /// Bounded DATA capture allocation failed.
    /// ```compile_fail,E0638
    /// use irig106_core::ErrorKind;
    /// fn closed(reason: ErrorKind) { if let ErrorKind::Allocation { source: _ } = reason {} }
    /// ```
    #[non_exhaustive]
    Allocation {
        /// Original typed cause.
        source: TryReserveError,
    },
    /// Advancing the consumed byte offset would exceed u64.
    OffsetOverflow,
}

impl From<io::Error> for ErrorKind {
    fn from(error: io::Error) -> Self {
        Self::Io { source: error }
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
            ErrorKind::Io { source: error } => error.kind(),
            ErrorKind::Pool { source } => match source {
                PoolError::DeadlineExceeded => io::ErrorKind::TimedOut,
                PoolError::WouldBlock | PoolError::BudgetUnavailable => io::ErrorKind::WouldBlock,
                PoolError::Cancelled => io::ErrorKind::Interrupted,
                PoolError::BudgetTooSmall { .. } | PoolError::InvalidRequest => {
                    io::ErrorKind::InvalidInput
                }
                _ => io::ErrorKind::Other,
            },
            ErrorKind::InvalidBuffer | ErrorKind::CaptureLimit { .. } => {
                io::ErrorKind::InvalidInput
            }
            ErrorKind::Allocation { source: _ } => io::ErrorKind::Other,
            _ => io::ErrorKind::InvalidData,
        }
    }
    /// Underlying typed header, I/O or allocation error, when present.
    pub fn get_ref(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        match &self.reason {
            ErrorKind::Admission { source } => Some(source),
            ErrorKind::Pool { source } => Some(source),
            ErrorKind::Io { source: error } => Some(error),
            ErrorKind::Header { source: error } => Some(error),
            ErrorKind::Allocation { source: error } => Some(error),
            _ => None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "packet reader at {}: {}", self.position, self.reason)?;
        if let Some(r) = self.recovery {
            write!(
                f,
                "; recovery started at {}, scanned {} bytes",
                r.start_offset, r.scanned_bytes
            )?;
        }
        Ok(())
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
            Self::Admission { source } => write!(f, "packet admission failed: {source}"),
            Self::Pool { source } => write!(f, "packet storage failed: {source}"),
            Self::Io { source: error } => write!(f, "input read failed: {error}"),
            Self::Header { source: error } => write!(f, "invalid packet framing: {error}"),
            Self::NoCompletePacket { tail } => {
                f.write_str("no complete Chapter 10 packet found")?;
                if let Some(t) = tail {
                    write!(f, ": {} at {} ({} bytes)", t.kind, t.offset, t.bytes)?;
                }
                Ok(())
            }
            Self::RecoveryLimit { limit } => {
                write!(f, "recovery search exhausted caller limit of {limit} bytes")
            }
            Self::InvalidBuffer => f.write_str("reader buffer must contain at least one byte"),
            Self::CaptureLimit { requested, limit } => write!(
                f,
                "requested DATA prefix {requested} bytes exceeds capture limit {limit} bytes"
            ),
            Self::Allocation { source: error } => {
                write!(f, "could not allocate DATA prefix: {error}")
            }
            Self::OffsetOverflow => f.write_str("input byte offset exceeds u64 range"),
        }
    }
}

impl From<AdmissionError> for ErrorKind {
    fn from(source: AdmissionError) -> Self {
        Self::Admission { source }
    }
}
impl From<PoolError> for ErrorKind {
    fn from(source: PoolError) -> Self {
        Self::Pool { source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_pool_failures_do_not_request_io_retry() {
        for (source, expected) in [
            (PoolError::Cancelled, io::ErrorKind::Other),
            (PoolError::BudgetUnavailable, io::ErrorKind::Other),
            (PoolError::WouldBlock, io::ErrorKind::WouldBlock),
            (PoolError::DeadlineExceeded, io::ErrorKind::TimedOut),
        ] {
            let error = Error {
                reason: ErrorKind::Pool { source },
                position: 24,
                recovery: None,
            };
            assert_eq!(error.kind(), expected);
        }
    }
    #[test]
    fn every_error_reason_has_human_display() {
        for (reason, text) in [
            (
                ErrorKind::Io {
                    source: io::Error::new(io::ErrorKind::PermissionDenied, "reader blocked"),
                },
                "input read failed: reader blocked",
            ),
            (
                ErrorKind::Header {
                    source: HeaderError::UnalignedPacket { length: 30 },
                },
                "invalid packet framing: packet length 30 is not divisible by four",
            ),
            (
                ErrorKind::NoCompletePacket { tail: None },
                "no complete Chapter 10 packet found",
            ),
            (
                ErrorKind::RecoveryLimit { limit: 7 },
                "recovery search exhausted caller limit of 7 bytes",
            ),
            (
                ErrorKind::InvalidBuffer,
                "reader buffer must contain at least one byte",
            ),
            (
                ErrorKind::CaptureLimit {
                    requested: 5,
                    limit: 4,
                },
                "requested DATA prefix 5 bytes exceeds capture limit 4 bytes",
            ),
            (
                ErrorKind::OffsetOverflow,
                "input byte offset exceeds u64 range",
            ),
        ] {
            assert_eq!(reason.to_string(), text);
        }
        let allocation = Vec::<u8>::new().try_reserve(usize::MAX).unwrap_err(); // No allocation attempted.
        assert!(
            ErrorKind::Allocation { source: allocation }
                .to_string()
                .starts_with("could not allocate DATA prefix: ")
        );
    }
}
