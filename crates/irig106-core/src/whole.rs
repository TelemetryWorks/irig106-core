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
const QUANTUM: usize = 4096;
/// Complete-frame routing and admission evidence; never a payload interpretation.
#[derive(Debug)]
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
}
/// Ordered whole-packet reader with one shared assembly lease and direct body reads.
/// Source starts at offset zero. No fallback to capture/drain/chunk delivery.
/// Errors/tails are terminal. Wait deadlines do not interrupt arbitrary blocking Read.
pub struct WholePacketReader<R> {
    framing: PacketReader<R>,
    policy: WholePolicy,
    pool: BlockPool,
    source: Option<SourceOps<R>>,
    _workspace: Workspace,
}
impl<R: Read> WholePacketReader<R> {
    /// Non-seekable input. Oversoft packets require explicit policy stream opt-in.
    pub fn stream(input: R, policy: WholePolicy, pool: BlockPool) -> Result<Self, Error> {
        let workspace = pool
            .workspace(mem::size_of::<Self>() + QUANTUM + 512)
            .map_err(|source| Error {
                reason: ErrorKind::Pool { source },
                position: 0,
                recovery: None,
            })?;
        Ok(Self {
            framing: PacketReader::direct(input, policy.recovery),
            policy,
            pool,
            source: None,
            _workspace: workspace,
        })
    }
    /// Return the exclusive source; no body read-ahead is retained.
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
        self.source
            .as_ref()
            .map(|ops| (ops.stamp)(self.framing.source()))
            .transpose()
            .map_err(Into::into)
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
        let (offset, header, ranges) = match self.framing.find_header()? {
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
        let mut received = 24;
        while buf.blocks.len() < count {
            let mut block = assembly.next_block_until(deadline)?;
            let start = buf.blocks.len() * BLOCK_BYTES;
            let valid = (length - start).min(BLOCK_BYTES);
            let mut filled = if start == 0 {
                block.storage_mut()[..24].copy_from_slice(header.as_bytes());
                24
            } else {
                0
            };
            while filled < valid {
                let wanted = (valid - filled).min(QUANTUM);
                let n = self
                    .framing
                    .read_counted(&mut block.storage_mut()[filled..filled + wanted])?;
                filled += n;
                received += n;
                if n < wanted {
                    self.unchanged(before)?;
                    return self.tail(offset);
                }
            }
            // Capacity was fallibly reserved/charged at PacketBuf construction.
            buf.blocks.push(block);
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
        reader.source = Some(SourceOps {
            stamp: R::source_stamp,
            probe: probe::<R>,
        });
        Ok(reader)
    }
}
