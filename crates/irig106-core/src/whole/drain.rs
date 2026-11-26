//! Explicit omissions; no whole-frame storage, admission or inventory observation.
use super::*;
use std::fmt;

/// Why the caller-requested omission could not be ordinary whole delivery.
/// This does not authorize size or claim a following header was corroborated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DrainReason {
    /// Caller explicitly discards; no whole-packet size authorization attempted.
    CallerRequested,
    /// Absolute whole-delivery hard ceiling would refuse this frame.
    #[non_exhaustive]
    HardLimit {
        /// Declared frame length.
        length: u32,
        /// Configured H.
        limit: u32,
    },
    /// Entire reclaimed pool could not hold whole delivery.
    #[non_exhaustive]
    BudgetTooSmall {
        /// Whole-frame working set including fixed commitments.
        needed: usize,
        /// Configured B.
        limit: usize,
    },
    /// Whole storage cannot represent P on this target; fixed draining can.
    TargetLimit,
}
/// Final omission disposition; none denotes a delivered complete packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DrainDisposition {
    /// Declared bytes consumed; next call may inspect the declared boundary.
    Drained,
    /// EOF before declared end; terminal, no body sync recovery.
    Truncated,
    /// Operational failure/cancellation/deadline/source change; terminal.
    Aborted,
}
/// Every explicit omission, with exact logical bytes including the primary header.
/// Checksums remain Absent/NotChecked; discarded contents are not verified.
/// A report cannot be passed off as a complete inventory observation.
/// ```compile_fail,E0308
/// use irig106_core::{DrainReport, Inspection};
/// fn wrong(a: &mut Inspection, r: DrainReport) { a.observe_packet_info(&r); }
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub struct DrainReport {
    /// Validated frame start, excluding preceding recovery bytes.
    pub offset: u64,
    /// Structurally checked primary, not evidence of payload meaning.
    pub header: Header,
    /// Checked declared DATA/filler/checksum ranges; not captured bytes.
    pub ranges: PacketRanges,
    /// Explicit caller request, or whole-delivery refusal classification.
    pub reason: DrainReason,
    /// Declared total frame bytes (including headers/trailer).
    pub requested_bytes: u64,
    /// Actually consumed frame bytes; never includes preceding garbage.
    pub consumed_bytes: u64,
    /// Completion/early termination of this omission.
    pub disposition: DrainDisposition,
    /// Header-finding recovery, separate from consumed frame bytes.
    pub recovery: Option<Recovery>,
}
/// Drain reports are distinct from packets and terminal framing tails.
#[derive(Debug)]
#[non_exhaustive]
pub enum DrainEvent {
    /// Explicit omission, including a reported truncated drain.
    Report(DrainReport),
    /// Tail encountered while looking for a header after established framing.
    #[non_exhaustive]
    Tail {
        /// Terminal framing evidence, not a packet.
        tail: Tail,
        /// Sync-search evidence.
        recovery: Option<Recovery>,
    },
}
/// Typed terminal failure plus omission progress when a header was available.
#[derive(Debug)]
#[non_exhaustive]
pub struct DrainError {
    /// Original typed cause, position and recovery.
    pub error: Error,
    /// Aborted report; None when no structurally valid header was found.
    pub report: Option<DrainReport>,
}
impl fmt::Display for DrainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "drain failed: {}", self.error)?;
        if let Some(r) = &self.report {
            write!(
                f,
                "; explicit drain at {} consumed {}/{} bytes",
                r.offset, r.consumed_bytes, r.requested_bytes
            )?;
        }
        Ok(())
    }
}
impl std::error::Error for DrainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
// Covered by the reader's existing fixed 512-byte metadata allowance.
const _: () = assert!(mem::size_of::<DrainReport>() + mem::size_of::<DrainError>() <= 512);

#[allow(
    clippy::result_large_err,
    reason = "Inline bounded progress avoids allocation while reporting failures."
)]
impl<R: Read> WholePacketReader<R> {
    /// Explicitly discard the next structurally framed packet, even above H/B.
    /// No packet storage, assembly lease, admission authorization or automatic
    /// fallback. Uses the existing charged fixed 4 KiB scratch; no credit waits.
    /// Calling after a terminal whole-reader refusal cannot resume that reader.
    /// Drained bytes establish framing only, not inventory completeness: callers
    /// must record the omission and stop/mark incomplete unfiltered inspection.
    /// Checks before/after bounded reads and Interrupted retries enforce deadline
    /// and pool cancellation cooperatively, not inside arbitrary blocking Read.
    pub fn drain_next(&mut self, deadline: Instant) -> Result<Option<DrainEvent>, DrainError> {
        self.drain_using(deadline, Instant::now)
    }
    fn drain_using(
        &mut self,
        deadline: Instant,
        mut now: impl FnMut() -> Instant,
    ) -> Result<Option<DrainEvent>, DrainError> {
        if self.framing.done {
            return Ok(None);
        }
        self.framing.recovery = None;
        let pool = self.pool.clone();
        let mut check = || -> Result<(), ErrorKind> {
            pool.live()?;
            if now() >= deadline {
                return Err(PoolError::DeadlineExceeded.into());
            }
            Ok(())
        };
        let mut report = None;
        let result = (|| -> Result<Option<DrainEvent>, ErrorKind> {
            check()?;
            let limit = if self.previous_large { 24 } else { usize::MAX };
            let (offset, header, ranges) =
                match self.framing.find_header_checked(limit, &mut check)? {
                    FrameStart::Header {
                        offset,
                        header,
                        ranges,
                    } => (offset, header, ranges),
                    FrameStart::End(event) => {
                        return Ok(event.map(|e| match e {
                            Event::Tail {
                                offset,
                                bytes,
                                kind,
                                recovery,
                            } => DrainEvent::Tail {
                                tail: Tail {
                                    offset,
                                    bytes,
                                    kind,
                                },
                                recovery,
                            },
                            Event::Packet(_) => unreachable!("header finder cannot emit a packet"),
                        }));
                    }
                };
            let p = header.packet_length();
            report = Some(DrainReport {
                offset,
                header,
                ranges,
                reason: DrainReason::CallerRequested,
                requested_bytes: u64::from(p),
                consumed_bytes: 24,
                disposition: DrainDisposition::Aborted,
                recovery: self.framing.recovery,
            });
            let end = offset
                .checked_add(u64::from(p))
                .ok_or(ErrorKind::OffsetOverflow)?;
            let reason = if p > self.policy.hard {
                DrainReason::HardLimit {
                    length: p,
                    limit: self.policy.hard,
                }
            } else if usize::try_from(p)
                .ok()
                .filter(|&p| p <= isize::MAX as usize)
                .is_none()
            {
                DrainReason::TargetLimit
            } else {
                match self.pool.frame_feasible((p as usize).div_ceil(BLOCK_BYTES)) {
                    Ok(()) => DrainReason::CallerRequested,
                    Err(PoolError::BudgetTooSmall { needed, limit }) => {
                        DrainReason::BudgetTooSmall { needed, limit }
                    }
                    Err(e) => return Err(e.into()),
                }
            };
            report.as_mut().unwrap().reason = reason;
            let before = self.stamp()?;
            let mut scratch = [0; QUANTUM];
            while self.framing.position < end {
                check()?;
                let wanted = (end - self.framing.position).min(QUANTUM as u64) as usize;
                let n = self.framing.read_checked(
                    &mut scratch[..wanted],
                    false,
                    usize::MAX,
                    &mut check,
                )?;
                check()?;
                self.unchanged(before)?;
                if n < wanted {
                    self.framing.done = true;
                    let mut r = report.take().unwrap();
                    r.consumed_bytes = self.framing.position - offset;
                    r.disposition = DrainDisposition::Truncated;
                    return Ok(Some(DrainEvent::Report(r)));
                }
            }
            check()?;
            self.unchanged(before)?;
            self.framing.established = true; // Completely consumed framing, NOT delivered data.
            self.previous_large = u64::from(p) - 24 >= self.policy.small_body as u64;
            let mut r = report.take().unwrap();
            r.consumed_bytes = self.framing.position - offset;
            r.disposition = DrainDisposition::Drained;
            Ok(Some(DrainEvent::Report(r)))
        })();
        result.map_err(|reason| {
            self.framing.done = true;
            if let Some(r) = &mut report {
                r.consumed_bytes = self.framing.position - r.offset;
            }
            DrainError {
                error: Error {
                    reason,
                    position: self.framing.position,
                    recovery: self.framing.recovery,
                },
                report,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Cursor,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };
    #[test]
    fn deadlines_cover_interrupted_header_reads_and_final_body_read() {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            struct Interrupted(Arc<AtomicUsize>);
            impl Read for Interrupted {
                fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                    self.0.fetch_add(1, Ordering::Relaxed);
                    Err(io::ErrorKind::Interrupted.into())
                }
            }
            let count = Arc::new(AtomicUsize::new(0));
            let mut r = WholePacketReader::stream(
                Interrupted(count.clone()),
                WholePolicy::new(),
                BlockPool::with_defaults().unwrap(),
            )
            .unwrap();
            let start = Instant::now();
            let mut ticks = 0;
            let e = r
                .drain_using(start + Duration::from_nanos(8), || {
                    ticks += 1;
                    start + Duration::from_nanos(ticks)
                })
                .unwrap_err();
            assert!(matches!(
                e.error.reason,
                ErrorKind::Pool {
                    source: PoolError::DeadlineExceeded,
                    ..
                }
            ));
            assert!((1..8).contains(&count.load(Ordering::Relaxed)));
            assert!(e.report.is_none());
            assert_eq!(r.position(), 0);
            assert!(r.drain_next(start).unwrap().is_none());
            send.send(()).unwrap();
        });
        receive
            .recv_timeout(Duration::from_secs(2))
            .expect("Interrupted drain exceeded watchdog");
        let bound = Instant::now() + Duration::from_secs(2);
        while !worker.is_finished() {
            assert!(Instant::now() < bound);
            std::thread::yield_now();
        }
        worker.join().unwrap();
        struct Advance {
            bytes: Cursor<Vec<u8>>,
            expired: Arc<AtomicBool>,
        }
        impl Read for Advance {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let n = self.bytes.read(out)?;
                if self.bytes.position() > 24 {
                    self.expired.store(true, Ordering::Relaxed);
                }
                Ok(n)
            }
        }
        let mut bytes = vec![0; 48];
        bytes[..2].copy_from_slice(&[0x25, 0xeb]);
        bytes[4] = 48;
        let sum = primary_checksum(bytes[..24].try_into().unwrap());
        bytes[22..24].copy_from_slice(&sum.to_le_bytes());
        let expired = Arc::new(AtomicBool::new(false));
        let input = Advance {
            bytes: Cursor::new(bytes),
            expired: expired.clone(),
        };
        let mut r = WholePacketReader::stream(
            input,
            WholePolicy::new().with_read_ahead_bytes(24),
            BlockPool::with_defaults().unwrap(),
        )
        .unwrap();
        let start = Instant::now();
        let e = r
            .drain_using(start + Duration::from_secs(1), || {
                if expired.load(Ordering::Relaxed) {
                    start + Duration::from_secs(2)
                } else {
                    start
                }
            })
            .unwrap_err();
        assert!(matches!(
            e.error.reason,
            ErrorKind::Pool {
                source: PoolError::DeadlineExceeded,
                ..
            }
        ));
        let d = e.report.unwrap();
        assert_eq!(
            (d.consumed_bytes, d.disposition),
            (48, DrainDisposition::Aborted)
        );
    }
}
