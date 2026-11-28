//! Opt-in callback-scoped provisional transfer, with an owned End commit report.
use super::*;
use std::{fmt, ops::Range};
/// Fixed maximum callback span, independently charged at reader construction.
pub const CHUNK_BYTES: usize = QUANTUM;
pub(super) const METADATA_BYTES: usize = 1024;
/// Provisional start after structural and size authorization; not PacketInfo.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChunkStart {
    /// Absolute frame offset.
    pub offset: u64,
    /// Checked primary, without payload meaning.
    pub header: Header,
    /// Declared ranges; checksum contents are still provisional.
    pub ranges: PacketRanges,
    /// Explicit edition/size authorization.
    pub admission: AdmissionEvidence,
    /// Configured total-frame H.
    pub hard_limit: u32,
    /// Configured aggregate B; whole-frame storage is not required in this mode.
    pub budget_bytes: usize,
    /// Maximum callback span, including frame headers/trailer.
    pub chunk_bytes: usize,
    /// Header recovery, separate from frame bytes.
    pub recovery: Option<Recovery>,
}
/// Borrow valid only for this callback; ranges are frame-relative, not file offsets.
#[derive(Debug)]
#[non_exhaustive]
pub struct PacketChunk<'a> {
    /// Monotone, disjoint complete-frame byte range.
    pub range: Range<u64>,
    /// Received frame bytes, including any headers/filler/trailer in this range.
    pub bytes: &'a [u8],
    /// DATA intersection, or an empty range within this chunk.
    pub data_range: Range<u64>,
    /// DATA only; never secondary/filler/trailer.
    pub data: &'a [u8],
}
/// Successful chunk-transfer commit, never a complete-packet inventory observation.
/// ```compile_fail,E0308
/// use irig106_core::{ChunkEnd, Inspection};
/// fn wrong(a:&mut Inspection,e:ChunkEnd) { a.observe_packet_info(&e); }
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChunkEnd {
    /// Absolute frame offset.
    pub offset: u64,
    /// Received frame bytes, including primary.
    pub frame_bytes: u64,
    /// Frame bytes acknowledged by successful chunk callbacks.
    pub delivered_bytes: u64,
    /// DATA bytes acknowledged by successful callbacks, not semantic consumption.
    pub data_bytes: u64,
    /// Final ranges and Absent/NotChecked/Verified/Mismatch evidence.
    pub ranges: PacketRanges,
    /// Optional secondary integrity, not interpreted time.
    pub secondary_integrity: Option<SecondaryIntegrity>,
    /// Final size authorization (including Unconfirmed(ExactEof)).
    pub admission: AdmissionEvidence,
    /// Header-finding recovery.
    pub recovery: Option<Recovery>,
}
/// Why provisional output was invalidated; full typed cause is in ChunkError.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChunkAbortKind {
    /// EOF before the declared end.
    Truncated,
    /// Operational source error (including terminal WouldBlock).
    Input,
    /// Stable-source mutation.
    SourceChanged,
    /// Shared pool cancellation.
    Cancelled,
    /// Absolute deadline expired at a cooperative checkpoint.
    DeadlineExceeded,
    /// Sink rejected Begin or a chunk.
    Sink,
    /// Other structural/storage failure.
    Other,
}
/// Terminal progress; all offered output is provisional and must be invalidated.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChunkAbort {
    /// Absolute frame offset.
    pub offset: u64,
    /// Declared frame bytes.
    pub requested_bytes: u64,
    /// Actual logical source bytes, including header; may exceed offered bytes.
    pub received_bytes: u64,
    /// Frame bytes offered, including a callback that returned Err.
    pub offered_bytes: u64,
    /// Frame bytes acknowledged by successful callbacks; invalidated on abort.
    pub delivered_bytes: u64,
    /// Acknowledged DATA bytes; also invalidated.
    pub delivered_data_bytes: u64,
    /// Abort classification.
    pub kind: ChunkAbortKind,
    /// Header recovery, excluded from byte totals.
    pub recovery: Option<Recovery>,
}
/// Callback records. End is returned separately, so no callback follows commit.
#[derive(Debug)]
#[non_exhaustive]
pub enum ChunkEvent<'a> {
    /// Provisional metadata, before any frame bytes are offered.
    Begin(&'a ChunkStart),
    /// Borrowed provisional bytes; never retain beyond this callback.
    Chunk(PacketChunk<'a>),
    /// Invalidate all provisional output; attempted even after sink failure.
    Abort(&'a ChunkAbort),
}
/// Result with no complete-packet variant; only End commits provisional output.
#[derive(Debug)]
#[non_exhaustive]
pub enum ChunkOutcome {
    /// All declared bytes arrived, checks completed and callbacks acknowledged.
    End(ChunkEnd),
    /// Terminal framing/known-short tail, with no Begin for that attempted frame.
    #[non_exhaustive]
    Tail {
        /// Tail evidence.
        tail: Tail,
        /// Recovery evidence.
        recovery: Option<Recovery>,
    },
}
/// Original failure; sink errors are never boxed or replaced by abort errors.
#[derive(Debug)]
#[non_exhaustive]
pub enum ChunkFailure<E> {
    /// Typed reader failure; NoCompletePacket is preserved before establishment.
    #[non_exhaustive]
    Core {
        /// Reader cause, position and recovery.
        error: Error,
    },
    /// Original callback rejection.
    #[non_exhaustive]
    Sink {
        /// Caller-owned error.
        error: E,
    },
    /// Truncated frame after an earlier completed transfer.
    #[non_exhaustive]
    Truncated {
        /// Expected EOF evidence, distinct from an operational I/O error.
        tail: Tail,
    },
}
/// Failure plus provisional progress and any secondary Abort callback error.
#[derive(Debug)]
#[non_exhaustive]
pub struct ChunkError<E> {
    /// Primary cause.
    pub failure: ChunkFailure<E>,
    /// Present iff Begin was attempted. No successful End exists on failure.
    pub abort: Option<ChunkAbort>,
    /// Abort notification failed too; never masks the primary cause.
    pub abort_sink_error: Option<E>,
}
impl<E: fmt::Display> fmt::Display for ChunkError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.failure {
            ChunkFailure::Core { error } => write!(f, "chunk transfer failed: {error}"),
            ChunkFailure::Sink { error } => write!(f, "chunk sink failed: {error}"),
            ChunkFailure::Truncated { tail } => write!(
                f,
                "truncated chunk transfer at {} ({} bytes)",
                tail.offset, tail.bytes
            ),
        }?;
        if let Some(a) = &self.abort {
            write!(
                f,
                "; received {}/{} bytes, acknowledged {}",
                a.received_bytes, a.requested_bytes, a.delivered_bytes
            )?;
        }
        if let Some(e) = &self.abort_sink_error {
            write!(f, "; abort notification failed: {e}")?;
        }
        Ok(())
    }
}
impl<E: std::error::Error + 'static> std::error::Error for ChunkError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.failure {
            ChunkFailure::Core { error } => Some(error),
            ChunkFailure::Sink { error } => Some(error),
            ChunkFailure::Truncated { .. } => None,
        }
    }
}
struct Transfer {
    start: ChunkStart,
    offered: u64,
    delivered: u64,
    data: u64,
}
enum Failure<E> {
    Core(ErrorKind),
    Sink(E),
    Truncated(Tail),
}
impl<E> From<ErrorKind> for Failure<E> {
    fn from(e: ErrorKind) -> Self {
        Self::Core(e)
    }
}
const _: () = assert!(
    mem::size_of::<Transfer>() * 2
        + mem::size_of::<ChunkEnd>()
        + mem::size_of::<ChunkError<()>>()
        + mem::size_of::<ChunkEvent<'static>>()
        + mem::size_of::<DataChecksum>()
        + 32
        <= METADATA_BYTES
);
fn tail_outcome(event: Option<Event>) -> Option<ChunkOutcome> {
    event.map(|e| match e {
        Event::Tail {
            offset,
            bytes,
            kind,
            recovery,
        } => ChunkOutcome::Tail {
            tail: Tail {
                offset,
                bytes,
                kind,
            },
            recovery,
        },
        Event::Packet(_) => unreachable!("header finder cannot emit a packet"),
    })
}
fn offer<E, F>(t: &mut Transfer, sink: &mut F, at: u64, bytes: &[u8]) -> Result<(), E>
where
    F: for<'a> FnMut(ChunkEvent<'a>) -> Result<(), E>,
{
    let end = at + bytes.len() as u64;
    let lo = u64::from(t.start.ranges.data.start).max(at).min(end);
    let hi = u64::from(t.start.ranges.data.end).min(end).max(lo);
    t.offered += bytes.len() as u64; // Partitioned frame ranges are bounded by u32 P.
    sink(ChunkEvent::Chunk(PacketChunk {
        range: at..end,
        bytes,
        data_range: lo..hi,
        data: &bytes[(lo - at) as usize..(hi - at) as usize],
    }))?;
    t.delivered += bytes.len() as u64;
    t.data += hi - lo;
    Ok(())
}
fn abort_kind<E>(failure: &ChunkFailure<E>) -> ChunkAbortKind {
    match failure {
        ChunkFailure::Sink { .. } => ChunkAbortKind::Sink,
        ChunkFailure::Truncated { .. } => ChunkAbortKind::Truncated,
        ChunkFailure::Core { error } => match &error.reason {
            ErrorKind::NoCompletePacket {
                tail:
                    Some(Tail {
                        kind: TailKind::TruncatedPacket,
                        ..
                    }),
            } => ChunkAbortKind::Truncated,
            ErrorKind::Io { .. } => ChunkAbortKind::Input,
            ErrorKind::Admission {
                source: AdmissionError::SourceChanged,
            } => ChunkAbortKind::SourceChanged,
            ErrorKind::Pool {
                source: PoolError::Cancelled,
            } => ChunkAbortKind::Cancelled,
            ErrorKind::Pool {
                source: PoolError::DeadlineExceeded,
            } => ChunkAbortKind::DeadlineExceeded,
            _ => ChunkAbortKind::Other,
        },
    }
}
#[allow(
    clippy::result_large_err,
    reason = "Fixed inline progress and caller-owned sink errors avoid failure-path allocation."
)]
impl<R: Read> WholePacketReader<R> {
    /// Explicit fixed-memory mode; default whole delivery never falls back here.
    /// Begin/Chunk bytes are provisional until the returned End. Callback borrows
    /// cannot escape, and no PacketInfo is emitted. Whole-frame B/isize allocation
    /// limits do not apply; H/edition soft authorization and fixed B still apply.
    /// Abort is attempted on every failure after Begin, preserving both sink
    /// errors if Abort fails too. Errors/EOF are terminal except positioned size
    /// refusals before Begin. Cancellation/deadlines are cooperative, including
    /// after callbacks; they cannot preempt blocking Read/Seek/sink work. Panic
    /// propagates with the reader terminal. Callback/output state needs its own
    /// bound and rollback; core cannot retract irreversible decoder side effects.
    /// ```compile_fail,E0521
    /// use irig106_core::*;
    /// use std::time::{Duration,Instant};
    /// let mut r=WholePacketReader::stream(&b""[..],WholePolicy::new(),BlockPool::with_defaults().unwrap()).unwrap();
    /// let mut retained=Vec::new();
    /// r.next_chunks(Instant::now()+Duration::from_secs(1),|e| {
    ///     if let ChunkEvent::Chunk(c)=e {retained.push(c.bytes);}
    ///     Ok::<(),()>(())
    /// });
    /// ```
    pub fn next_chunks<E, F>(
        &mut self,
        deadline: Instant,
        mut sink: F,
    ) -> Result<Option<ChunkOutcome>, ChunkError<E>>
    where
        F: for<'a> FnMut(ChunkEvent<'a>) -> Result<(), E>,
    {
        self.chunks_using(deadline, &mut sink, Instant::now)
    }
    fn chunks_using<E, F>(
        &mut self,
        deadline: Instant,
        sink: &mut F,
        mut now: impl FnMut() -> Instant,
    ) -> Result<Option<ChunkOutcome>, ChunkError<E>>
    where
        F: for<'a> FnMut(ChunkEvent<'a>) -> Result<(), E>,
    {
        if self.framing.done {
            return Ok(None);
        }
        self.framing.done = true; // Fail closed on callback/source panic as well.
        self.framing.recovery = self.refused.as_ref().and_then(|p| p.recovery);
        let pool = self.pool.clone();
        let mut check = || -> Result<(), ErrorKind> {
            pool.live()?;
            if now() >= deadline {
                return Err(PoolError::DeadlineExceeded.into());
            }
            Ok(())
        };
        let mut transfer: Option<Transfer> = None;
        let result = (|| -> Result<Option<ChunkOutcome>, Failure<E>> {
            check()?;
            let (offset, header, mut ranges) = match self.next_header_checked(&mut check)? {
                FrameStart::Header {
                    offset,
                    header,
                    ranges,
                } => (offset, header, ranges),
                FrameStart::End(event) => return Ok(tail_outcome(event)),
            };
            let p = u64::from(header.packet_length());
            if header.packet_length() > self.policy.hard {
                return Err(self
                    .size_refusal(
                        offset,
                        &header,
                        &ranges,
                        AdmissionError::HardLimit {
                            length: header.packet_length(),
                            limit: self.policy.hard,
                        }
                        .into(),
                    )
                    .into());
            }
            let Some(end) = offset.checked_add(p) else {
                return Err(self
                    .size_refusal(offset, &header, &ranges, AdmissionError::TargetLimit.into())
                    .into());
            };
            let before = self.stamp()?;
            let mut scratch = [0; CHUNK_BYTES];
            if before.is_some_and(|s| end > s.size) {
                // Preserve the approved known-short truncation-only path: no Begin.
                while self.framing.position < end {
                    let wanted = (end - self.framing.position).min(CHUNK_BYTES as u64) as usize;
                    let n = self.framing.read_checked(
                        &mut scratch[..wanted],
                        false,
                        usize::MAX,
                        &mut check,
                    )?;
                    check()?;
                    self.unchanged(before)?;
                    if n < wanted {
                        break;
                    }
                }
                return Ok(tail_outcome(
                    self.framing.tail(offset, TailKind::TruncatedPacket)?,
                ));
            }
            let mut admission = self.policy.evidence(&header);
            if admission.over_soft {
                admission.authorization = if let Some(stamp) = before {
                    if end == stamp.size {
                        self.unchanged(before)?;
                        Authorization::Unconfirmed(UnconfirmedReason::ExactEof)
                    } else if (self.source.as_ref().unwrap().probe)(self.framing.source(), end)
                        .map_err(ErrorKind::from)?
                    {
                        Authorization::NextHeader { offset: end }
                    } else {
                        return Err(ErrorKind::from(AdmissionError::Uncorroborated).into());
                    }
                } else if self.policy.stream {
                    Authorization::StreamOptIn
                } else {
                    return Err(ErrorKind::from(AdmissionError::Uncorroborated).into());
                };
            }
            let start = ChunkStart {
                offset,
                header,
                ranges: ranges.clone(),
                admission,
                hard_limit: self.policy.hard,
                budget_bytes: self.pool.stats().map_err(ErrorKind::from)?.budget_bytes,
                chunk_bytes: CHUNK_BYTES,
                recovery: self.framing.recovery,
            };
            check()?;
            transfer = Some(Transfer {
                start,
                offered: 0,
                delivered: 0,
                data: 0,
            });
            let t = transfer.as_mut().unwrap();
            sink(ChunkEvent::Begin(&t.start)).map_err(Failure::Sink)?;
            check()?;
            offer(t, sink, 0, header.as_bytes()).map_err(Failure::Sink)?;
            check()?;
            let width = ranges.checksum.len();
            let mut checksum =
                (self.policy.verify_data_checksum && width > 0).then(|| DataChecksum::new(width));
            let mut stored = [0; 4];
            let mut secondary_integrity = None;
            while self.framing.position < end {
                let relative = self.framing.position - offset;
                let wanted = (end - self.framing.position).min(CHUNK_BYTES as u64) as usize;
                let n = self.framing.read_checked(
                    &mut scratch[..wanted],
                    false,
                    usize::MAX,
                    &mut check,
                )?;
                check()?;
                self.unchanged(before)?;
                if n > 0 {
                    if relative == 24 && header.has_secondary_header() && n >= 12 {
                        secondary_integrity = Some(SecondaryIntegrity::inspect(
                            scratch[..12].try_into().unwrap(),
                        ));
                    }
                    if let Some(c) = &mut checksum {
                        let lo = u64::from(header.header_length())
                            .saturating_sub(relative)
                            .min(n as u64) as usize;
                        let hi = u64::from(ranges.checksum.start)
                            .saturating_sub(relative)
                            .min(n as u64) as usize;
                        if lo < hi {
                            c.update(&scratch[lo..hi]);
                        }
                    }
                    if relative + n as u64 == p && width > 0 {
                        stored[..width].copy_from_slice(&scratch[n - width..n]);
                    }
                    offer(t, sink, relative, &scratch[..n]).map_err(Failure::Sink)?;
                    check()?;
                }
                if n < wanted {
                    let tail = Tail {
                        offset,
                        bytes: self.framing.position - offset,
                        kind: TailKind::TruncatedPacket,
                    };
                    self.framing.tail(offset, TailKind::TruncatedPacket)?;
                    return Err(Failure::Truncated(tail));
                }
            }
            self.unchanged(before)?;
            check()?;
            if let Some(c) = checksum {
                ranges.data_checksum = c.finish(&stored[..width]);
            }
            debug_assert_eq!((t.delivered, t.data), (p, u64::from(header.data_length())));
            self.framing.established = true;
            self.previous_large = p - 24 >= self.policy.small_body as u64;
            self.framing.done = false; // Returned End is the sole commit point.
            Ok(Some(ChunkOutcome::End(ChunkEnd {
                offset,
                frame_bytes: p,
                delivered_bytes: t.delivered,
                data_bytes: t.data,
                ranges,
                secondary_integrity,
                admission,
                recovery: self.framing.recovery,
            })))
        })();
        match result {
            Ok(outcome) => Ok(outcome),
            Err(reason) => {
                let failure = match reason {
                    Failure::Core(e) => ChunkFailure::Core {
                        error: self.finish_error(e),
                    },
                    Failure::Sink(error) => ChunkFailure::Sink { error },
                    Failure::Truncated(tail) => ChunkFailure::Truncated { tail },
                };
                let abort = transfer.map(|t| ChunkAbort {
                    offset: t.start.offset,
                    requested_bytes: u64::from(t.start.header.packet_length()),
                    received_bytes: self.framing.position - t.start.offset,
                    offered_bytes: t.offered,
                    delivered_bytes: t.delivered,
                    delivered_data_bytes: t.data,
                    kind: abort_kind(&failure),
                    recovery: t.start.recovery,
                });
                if abort.is_some() {
                    self.framing.done = true;
                    self.refused = None;
                }
                let abort_sink_error = abort
                    .as_ref()
                    .and_then(|a| sink(ChunkEvent::Abort(a)).err());
                Err(ChunkError {
                    failure,
                    abort,
                    abort_sink_error,
                })
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/support/watchdog.rs"]
mod support;
#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, time::Duration};
    #[test]
    fn deadline_after_callback_and_interrupted_retries_never_commit() {
        super::support::watchdog(Duration::from_secs(5), || {
            let raw = [
                0x25, 0xeb, 0, 0, 28, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0x45,
                0xf4, 1, 2, 3, 4,
            ];
            let mut r = WholePacketReader::stream(
                &raw[..],
                WholePolicy::new(),
                BlockPool::with_defaults().unwrap(),
            )
            .unwrap();
            let expired = Cell::new(false);
            let base = Instant::now();
            let mut sink = |e: ChunkEvent<'_>| {
                if matches!(e, ChunkEvent::Chunk(_)) {
                    expired.set(true);
                }
                Ok::<(), ()>(())
            };
            let e = r
                .chunks_using(base + Duration::from_secs(1), &mut sink, || {
                    if expired.get() {
                        base + Duration::from_secs(2)
                    } else {
                        base
                    }
                })
                .unwrap_err();
            assert_eq!(e.abort.unwrap().kind, ChunkAbortKind::DeadlineExceeded);
            assert!(r.framing.done);
            struct Interrupted;
            impl Read for Interrupted {
                fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                    Err(io::ErrorKind::Interrupted.into())
                }
            }
            let mut r = WholePacketReader::stream(
                Interrupted,
                WholePolicy::new(),
                BlockPool::with_defaults().unwrap(),
            )
            .unwrap();
            let mut ticks = 0;
            let mut sink = |_: ChunkEvent<'_>| -> Result<(), ()> { panic!("no begin") };
            let e = r
                .chunks_using(base + Duration::from_nanos(8), &mut sink, || {
                    ticks += 1;
                    base + Duration::from_nanos(ticks)
                })
                .unwrap_err();
            assert!(e.abort.is_none());
            assert!(matches!(
                e.failure,
                ChunkFailure::Core {
                    error: Error {
                        reason: ErrorKind::Pool {
                            source: PoolError::DeadlineExceeded,
                            ..
                        },
                        ..
                    },
                    ..
                }
            ));
        });
    }
}
