//! Reusable complete-frame storage and direct-read integration.
use crate::{
    admission::{probe, seek_retry},
    pool::Workspace,
    reader::FrameStart,
    *,
};
use std::{
    io::{self, Read},
    mem,
    time::Instant,
};
mod drain;
pub use drain::{DrainDisposition, DrainError, DrainEvent, DrainReason, DrainReport};
const QUANTUM: usize = 4096;
/// Complete-frame routing and admission evidence; never a payload interpretation.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PacketInfo {
    /// Absolute frame start.
    pub offset: u64,
    /// Checked primary header.
    pub header: Header,
    /// Checked relative DATA/filler/checksum ranges.
    pub ranges: PacketRanges,
    /// Raw secondary integrity, if present.
    pub secondary_integrity: Option<SecondaryIntegrity>,
    /// Bytes skipped before this frame.
    pub recovered_bytes: u64,
    /// Explicit size authorization.
    pub admission: AdmissionEvidence,
}
/// Explicit successful coalescing evidence, never a new transport observation.
/// ```compile_fail,E0639
/// use irig106_core::CoalescingEvidence;
/// fn closed(e: CoalescingEvidence) { let _ = CoalescingEvidence { ..e }; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CoalescingEvidence {
    /// Original absolute packet offset.
    pub offset: u64,
    /// Complete frame bytes explicitly copied, including headers/filler/trailer.
    pub copied_bytes: usize,
    /// Input blocks still charged simultaneously during the copy.
    pub source_blocks: usize,
    /// Retained payload capacity plus metadata charged to B.
    pub charged_bytes: usize,
}
/// Independently owned contiguous copy, explicitly created under the same B.
/// Spans remain the allocation-free default. Drop frees storage before credits.
/// ```compile_fail,E0505
/// use irig106_core::CoalescedPacket;
/// fn borrowed(p: CoalescedPacket) { let data = p.data(); drop(p); println!("{:?}", data); }
/// ```
#[derive(Debug)]
pub struct CoalescedPacket {
    bytes: Vec<u8>, // Field order is significant: free before _lease Drop.
    info: PacketInfo,
    evidence: CoalescingEvidence,
    _lease: crate::pool::CoalescedLease,
}
impl CoalescedPacket {
    /// Borrow the entire original frame, without another copy.
    pub fn frame(&self) -> &[u8] {
        &self.bytes
    }
    /// Borrow DATA only; excludes secondary/filler/checksum bytes.
    pub fn data(&self) -> &[u8] {
        &self.bytes[self.info.ranges.data.start as usize..self.info.ranges.data.end as usize]
    }
    /// Preserved admission/integrity/routing evidence; do not inspect twice.
    pub fn info(&self) -> &PacketInfo {
        &self.info
    }
    /// Report every explicit copy and its accounted cost.
    pub fn evidence(&self) -> CoalescingEvidence {
        self.evidence
    }
}
/// Caller-reusable owned frame storage. Move it to workers; borrow its slices.
/// Descriptor capacity is reserved/charged from B at construction, never from P.
/// ```compile_fail,E0502
/// use irig106_core::{BlockPool, PacketBuf};
/// let mut buf = PacketBuf::new(BlockPool::with_defaults().unwrap()).unwrap();
/// let mut spans = buf.frame_spans();
/// buf.clear();
/// println!("{:?}", spans.next());
/// ```
#[derive(Debug)]
pub struct PacketBuf {
    blocks: Vec<PooledBlock>,
    info: Option<PacketInfo>,
    pool: BlockPool,
    _workspace: Workspace,
}
impl PacketBuf {
    /// Reserve bounded segment descriptors, with zero payload blocks.
    pub fn new(pool: BlockPool) -> Result<Self, PoolError> {
        let slots = pool.slots();
        let bytes = slots
            .checked_mul(mem::size_of::<PooledBlock>())
            .and_then(|n| n.checked_add(mem::size_of::<Self>()))
            .ok_or(PoolError::InvalidRequest)?;
        let workspace = pool.workspace(bytes)?;
        let mut blocks = Vec::new();
        blocks.try_reserve_exact(slots)?;
        if blocks.capacity() != slots {
            return Err(PoolError::UnexpectedCapacity);
        }
        Ok(Self {
            blocks,
            info: None,
            pool,
            _workspace: workspace,
        })
    }
    /// Complete-frame metadata, absent on empty/tail/error outcomes.
    pub fn info(&self) -> Option<&PacketInfo> {
        self.info.as_ref()
    }
    /// Borrow the single complete contiguous frame, if it occupies one block.
    pub fn contiguous(&self) -> Option<&[u8]> {
        let length = self.info.as_ref()?.header.packet_length() as usize;
        (self.blocks.len() == 1).then(|| &self.blocks[0].storage()[..length])
    }
    /// Borrow complete frame spans; empty before completion. No copying/allocation.
    pub fn frame_spans(&self) -> impl Iterator<Item = &[u8]> {
        let length = self
            .info
            .as_ref()
            .map_or(0, |i| i.header.packet_length() as usize);
        self.blocks
            .iter()
            .enumerate()
            .filter_map(move |(index, b)| {
                let start = index * BLOCK_BYTES;
                (start < length).then(|| &b.storage()[..(length - start).min(BLOCK_BYTES)])
            })
    }
    /// Borrow DATA only, omitting secondary/filler/checksum bytes.
    pub fn data_spans(&self) -> impl Iterator<Item = &[u8]> {
        let range = self.info.as_ref().map_or(0..0, |i| {
            i.ranges.data.start as usize..i.ranges.data.end as usize
        });
        self.blocks
            .iter()
            .enumerate()
            .filter_map(move |(index, b)| {
                let offset = index * BLOCK_BYTES;
                let start = range.start.saturating_sub(offset).min(BLOCK_BYTES);
                let end = range.end.saturating_sub(offset).min(BLOCK_BYTES);
                (start < end).then(|| &b.storage()[start..end])
            })
    }
    /// Explicitly copy a COMPLETE frame into independently owned contiguous storage.
    /// Check contiguous() first; coalesce_until always copies, even for one block.
    /// Old+new capacity is charged, including concurrent reader assembly.
    /// Unlike pool begin_until/next_block_until, this never waits for credits:
    /// Block-credit waiters have priority; queued assembly permits do not.
    /// WouldBlock preserves idle storage.
    /// _until bounds only the copy; credit shortage returns WouldBlock immediately.
    /// There is no try_ variant because credit acquisition is already non-waiting.
    /// Deadline/cancellation
    /// are checked between bounded block copies, not inside the OS allocator.
    /// Source remains unchanged on success or failure; one-block callers can
    /// instead borrow contiguous() without copying. No automatic fallback.
    pub fn coalesce_until(&self, deadline: Instant) -> Result<CoalescedPacket, PoolError> {
        self.coalesce_using(deadline, Instant::now)
    }
    fn coalesce_using(
        &self,
        deadline: Instant,
        mut now: impl FnMut() -> Instant,
    ) -> Result<CoalescedPacket, PoolError> {
        let check = |clock: &mut dyn FnMut() -> Instant| {
            self.pool.live()?;
            if clock() >= deadline {
                Err(PoolError::DeadlineExceeded)
            } else {
                Ok(())
            }
        };
        check(&mut now)?;
        let info = self.info.as_ref().ok_or(PoolError::InvalidRequest)?;
        let length =
            usize::try_from(info.header.packet_length()).map_err(|_| PoolError::InvalidRequest)?;
        let charged_bytes = length
            .checked_add(mem::size_of::<CoalescedPacket>())
            .ok_or(PoolError::InvalidRequest)?;
        let lease = self.pool.coalescing(charged_bytes, self.blocks.len())?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length)?;
        if bytes.capacity() != length {
            return Err(PoolError::UnexpectedCapacity);
        }
        for span in self.frame_spans() {
            check(&mut now)?;
            // The complete source proves summed spans <= reserved capacity.
            bytes.extend_from_slice(span);
        }
        check(&mut now)?;
        Ok(CoalescedPacket {
            bytes,
            info: info.clone(),
            evidence: CoalescingEvidence {
                offset: info.offset,
                copied_bytes: length,
                source_blocks: self.blocks.len(),
                charged_bytes,
            },
            _lease: lease,
        })
    }
    /// Return payload storage, retaining charged descriptors for reuse.
    pub fn clear(&mut self) {
        self.info = None;
        let count = self.blocks.len();
        self.blocks.clear();
        // Poison/cancellation cannot make Drop panic; subsequent acquisition is typed.
        let _ = self.pool.trim_after_return(count);
    }
}
impl Drop for PacketBuf {
    fn drop(&mut self) {
        self.clear();
    }
}
/// Outcome of filling caller-owned PacketBuf; no payload copies or partial dispatch.
#[derive(Debug)]
#[non_exhaustive]
pub enum WholeEvent {
    /// PacketBuf holds exactly one complete frame and its evidence.
    Packet,
    /// Terminal non-packet bytes, after an established stream.
    #[non_exhaustive]
    Tail {
        /// Terminal evidence.
        tail: Tail,
        /// Recovery incident, separate from tail bytes.
        recovery: Option<Recovery>,
    },
}
struct SourceOps<R> {
    stamp: fn(&mut R) -> io::Result<SourceStamp>,
    probe: fn(&mut R, u64) -> io::Result<bool>,
    origin: SourceStamp,
}
/// Ordered whole-packet reader with budgeted hybrid read-ahead and pooled bodies.
/// Source starts at offset zero. No fallback to capture/drain/chunk delivery.
/// Errors/tails are terminal. Wait deadlines do not interrupt arbitrary blocking Read.
pub struct WholePacketReader<R> {
    framing: PacketReader<R>,
    policy: WholePolicy,
    previous_large: bool,
    pool: BlockPool,
    source: Option<SourceOps<R>>,
    _workspace: Workspace,
}
impl<R: Read> WholePacketReader<R> {
    /// Non-seekable input. Oversoft packets require explicit policy stream opt-in.
    pub fn stream(input: R, policy: WholePolicy, pool: BlockPool) -> Result<Self, Error> {
        if !(24..=isize::MAX as usize).contains(&policy.read_ahead) {
            return Err(Error {
                reason: ErrorKind::Pool {
                    source: PoolError::InvalidRequest,
                },
                position: 0,
                recovery: None,
            });
        }
        let bytes = policy
            .read_ahead
            .checked_add(mem::size_of::<Self>() + QUANTUM + 512)
            .ok_or(Error {
                reason: ErrorKind::Pool {
                    source: PoolError::InvalidRequest,
                },
                position: 0,
                recovery: None,
            })?;
        let workspace = pool.workspace(bytes).map_err(|source| Error {
            reason: ErrorKind::Pool { source },
            position: 0,
            recovery: None,
        })?;
        Ok(Self {
            framing: PacketReader::whole(input, policy.recovery, policy.read_ahead).map_err(
                |source| Error {
                    reason: ErrorKind::Pool { source },
                    position: 0,
                    recovery: None,
                },
            )?,
            policy,
            previous_large: false,
            pool,
            source: None,
            _workspace: workspace,
        })
    }
    /// Return the exclusive source, discarding pending read-ahead.
    /// Its physical cursor may be ahead of position() by the configured buffer size.
    pub fn into_inner(self) -> R {
        self.framing.into_inner()
    }
    /// Consumed logical position, excluding restored lookahead probes.
    pub fn position(&self) -> u64 {
        self.framing.position
    }
    /// Consume one frame into reusable storage, using one absolute budget deadline.
    /// Holding prior frames on the same thread requires a deadline or releasing them.
    pub fn next_packet_into(
        &mut self,
        buf: &mut PacketBuf,
        deadline: Instant,
    ) -> Result<Option<WholeEvent>, Error> {
        buf.clear();
        if self.framing.done {
            return Ok(None);
        }
        self.framing.recovery = None;
        let result = self.next_inner(buf, deadline);
        if !matches!(result, Ok(Some(WholeEvent::Packet))) {
            buf.clear();
        }
        result.map_err(|reason| {
            self.framing.done = true;
            Error {
                reason,
                position: self.framing.position,
                recovery: self.framing.recovery,
            }
        })
    }
    fn tail(&mut self, offset: u64) -> Result<Option<WholeEvent>, ErrorKind> {
        self.framing
            .tail(offset, TailKind::TruncatedPacket)
            .map(Self::end)
    }
    fn end(event: Option<Event>) -> Option<WholeEvent> {
        match event {
            Some(Event::Tail {
                offset,
                bytes,
                kind,
                recovery,
            }) => Some(WholeEvent::Tail {
                tail: Tail {
                    offset,
                    bytes,
                    kind,
                },
                recovery,
            }),
            _ => None,
        }
    }
    fn stamp(&mut self) -> Result<Option<SourceStamp>, ErrorKind> {
        if let Some(ops) = &self.source {
            let stamp = (ops.stamp)(self.framing.source())?;
            if stamp != ops.origin {
                return Err(AdmissionError::SourceChanged.into());
            }
            Ok(Some(stamp))
        } else {
            Ok(None)
        }
    }
    fn unchanged(&mut self, before: Option<SourceStamp>) -> Result<(), ErrorKind> {
        if self.stamp()? != before || before.is_some_and(|stamp| self.framing.position > stamp.size)
        {
            return Err(AdmissionError::SourceChanged.into());
        }
        Ok(())
    }
    fn next_inner(
        &mut self,
        buf: &mut PacketBuf,
        deadline: Instant,
    ) -> Result<Option<WholeEvent>, ErrorKind> {
        if !self.pool.same_pool(&buf.pool) {
            return Err(PoolError::InvalidRequest.into());
        }
        // Predict only header-refill size, never body delivery/copy policy.
        // No previous frame (and a previous small body) keeps full amortisation.
        let header_limit = if self.previous_large { 24 } else { usize::MAX };
        let (offset, header, ranges) = match self.framing.find_header_limited(header_limit)? {
            FrameStart::Header {
                offset,
                header,
                ranges,
            } => (offset, header, ranges),
            FrameStart::End(event) => return Ok(Self::end(event)),
        };
        let p = header.packet_length();
        if p > self.policy.hard {
            return Err(AdmissionError::HardLimit {
                length: p,
                limit: self.policy.hard,
            }
            .into());
        }
        let length = usize::try_from(p)
            .ok()
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or(AdmissionError::TargetLimit)?;
        let end = offset
            .checked_add(u64::from(p))
            .ok_or(AdmissionError::TargetLimit)?;
        let count = length.div_ceil(BLOCK_BYTES);
        // Refusal/known-short paths do not wait for an unrelated active build.
        self.pool.frame_feasible(count)?;
        let mut evidence = self.policy.evidence(&header);
        let before = self.stamp()?;
        if let Some(stamp) = before {
            if end > stamp.size {
                let mut scratch = [0; QUANTUM];
                let mut remaining = u64::from(p) - 24;
                while remaining > 0 {
                    let wanted = remaining.min(QUANTUM as u64) as usize;
                    let n = self.framing.read_counted(&mut scratch[..wanted])?;
                    remaining -= n as u64;
                    if n < wanted {
                        break;
                    }
                }
                self.unchanged(before)?;
                return self.tail(offset);
            }
        }
        if evidence.over_soft {
            if let Some(stamp) = before {
                evidence.authorization = if end == stamp.size {
                    self.unchanged(before)?;
                    Authorization::Unconfirmed(UnconfirmedReason::ExactEof)
                } else if (self.source.as_ref().unwrap().probe)(self.framing.source(), end)? {
                    Authorization::NextHeader { offset: end }
                } else {
                    return Err(AdmissionError::Uncorroborated.into());
                };
            } else if self.policy.stream {
                evidence.authorization = Authorization::StreamOptIn;
            } else {
                return Err(AdmissionError::Uncorroborated.into());
            }
        }
        // Rechecks feasibility after any intervening constructor; one build per B.
        let mut assembly = self.pool.begin_until(count, deadline)?;
        // Choose once for the entire body, not a short read or final fragment.
        let body = length - 24;
        let copy_body = body < self.policy.small_body || self.framing.buffered() >= body;
        let mut received = 24;
        while buf.blocks.len() < count {
            let block = assembly.next_block_until(deadline)?;
            let start = buf.blocks.len() * BLOCK_BYTES;
            // Retain the provisional descriptor too: error/EOF cleanup must
            // return and trim the complete actual working set, including it.
            buf.blocks.push(block);
            let block = buf.blocks.last_mut().unwrap();
            let valid = (length - start).min(BLOCK_BYTES);
            let mut filled = if start == 0 {
                block.storage_mut()[..24].copy_from_slice(header.as_bytes());
                24
            } else {
                0
            };
            while filled < valid {
                let wanted = valid - filled;
                let n = self.framing.read_body_counted(
                    &mut block.storage_mut()[filled..filled + wanted],
                    copy_body,
                )?;
                filled += n;
                received += n;
                if n < wanted {
                    self.unchanged(before)?;
                    return self.tail(offset);
                }
            }
        }
        debug_assert_eq!(received, length);
        self.unchanged(before)?;
        let secondary_integrity = if header.has_secondary_header() {
            let raw: &[u8; 12] = buf.blocks[0].storage()[24..36].try_into().unwrap();
            Some(SecondaryIntegrity::inspect(raw))
        } else {
            None
        };
        self.framing.established = true;
        self.previous_large = body >= self.policy.small_body;
        buf.info = Some(PacketInfo {
            offset,
            header,
            ranges,
            secondary_integrity,
            recovered_bytes: self.framing.recovery.map_or(0, |r| r.scanned_bytes),
            admission: evidence,
        });
        Ok(Some(WholeEvent::Packet))
    }
}
impl<R: StableSource> WholePacketReader<R> {
    /// Enable lookahead/exact-EOF corroboration on a caller-guaranteed stable source.
    pub fn seek_checked(mut input: R, policy: WholePolicy, pool: BlockPool) -> Result<Self, Error> {
        let position =
            seek_retry(&mut input, std::io::SeekFrom::Current(0)).map_err(|source| Error {
                reason: ErrorKind::Io { source },
                position: 0,
                recovery: None,
            })?;
        if position != 0 {
            return Err(Error {
                reason: ErrorKind::Pool {
                    source: PoolError::InvalidRequest,
                },
                position: 0,
                recovery: None,
            });
        }
        let mut reader = Self::stream(input, policy, pool)?;
        let origin = reader
            .framing
            .source()
            .source_stamp()
            .map_err(|source| Error {
                reason: ErrorKind::Io { source },
                position: 0,
                recovery: None,
            })?;
        reader.source = Some(SourceOps {
            stamp: R::source_stamp,
            probe: probe::<R>,
            origin,
        });
        Ok(reader)
    }
}

#[cfg(test)]
mod coalescing_tests {
    use super::*;
    #[test]
    fn cooperative_copy_expiry_and_cancel_roll_back() {
        for cancel in [false, true] {
            let pool = BlockPool::with_defaults().unwrap();
            let p = BLOCK_BYTES + 28;
            let mut bytes = vec![0u8; p];
            bytes[..2].copy_from_slice(&[0x25, 0xeb]);
            bytes[4..8].copy_from_slice(&(p as u32).to_le_bytes());
            bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
            bytes[15] = 1;
            let sum = primary_checksum(bytes[..24].try_into().unwrap());
            bytes[22..24].copy_from_slice(&sum.to_le_bytes());
            let mut source = PacketBuf::new(pool.clone()).unwrap();
            let mut reader =
                WholePacketReader::stream(io::Cursor::new(bytes), WholePolicy::new(), pool.clone())
                    .unwrap();
            let before_time = Instant::now();
            let end = before_time + std::time::Duration::from_secs(5);
            reader.next_packet_into(&mut source, end).unwrap();
            let mut calls = 0;
            let result = source.coalesce_using(end, || {
                calls += 1;
                if calls == 3 {
                    // First block copied; expire/cancel before completion.
                    if cancel {
                        pool.cancel().unwrap();
                    } else {
                        return end;
                    }
                }
                before_time
            });
            assert!(if cancel {
                matches!(result, Err(PoolError::Cancelled))
            } else {
                matches!(result, Err(PoolError::DeadlineExceeded))
            });
            assert_eq!(pool.stats().unwrap().coalesced_bytes, 0);
            assert!(source.info().is_some());
            assert_eq!(source.frame_spans().map(|s| s.len()).sum::<usize>(), p);
        }
    }
}
