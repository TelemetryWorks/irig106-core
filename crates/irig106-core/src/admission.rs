//! Explicit size policy and seekable-source corroboration, without payload meaning.
use crate::{Header, PacketRanges};
use std::io::{self, Read, Seek, SeekFrom};
/// Built-in hard ceiling, in complete-frame bytes.
pub const DEFAULT_HARD_LIMIT: u32 = 134_217_728;
/// Explicit edition policy; never inferred from data-type versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditionPolicy {
    /// Baseline 106-24R1 Chapter 11 / Table 11-3.
    Irig106_24R1,
    /// Caller-supplied raw uncertainty; conservative ordinary threshold.
    Unknown(u16),
}
/// Whole-packet policy; aggregate B is selected independently on BlockPool.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct WholePolicy {
    pub(crate) hard: u32,
    pub(crate) edition: EditionPolicy,
    pub(crate) legacy: bool,
    pub(crate) stream: bool,
    pub(crate) recovery: Option<u64>,
}
impl Default for WholePolicy {
    fn default() -> Self {
        Self::new()
    }
}
impl WholePolicy {
    /// H=128 MiB; 24R1; no legacy allowance, stream opt-in or recovery cap.
    pub const fn new() -> Self {
        Self {
            hard: DEFAULT_HARD_LIMIT,
            edition: EditionPolicy::Irig106_24R1,
            legacy: false,
            stream: false,
            recovery: None,
        }
    }
    /// Override absolute total-frame H; soft allowances never override it.
    pub const fn with_hard_limit(mut self, bytes: u32) -> Self {
        self.hard = bytes;
        self
    }
    /// Select edition explicitly.
    pub const fn with_edition(mut self, edition: EditionPolicy) -> Self {
        self.edition = edition;
        self
    }
    /// Explicit legacy distributed-multiplexer setup allowance; default off.
    pub const fn with_legacy_setup_any_channel(mut self, enabled: bool) -> Self {
        self.legacy = enabled;
        self
    }
    /// Explicitly authorize oversoft packets on non-seekable input; default off.
    pub const fn with_stream_opt_in(mut self, enabled: bool) -> Self {
        self.stream = enabled;
        self
    }
    /// Optional recovery candidate-displacement cap; default None.
    pub const fn with_recovery_bytes(mut self, bytes: Option<u64>) -> Self {
        self.recovery = bytes;
        self
    }
    /// Configured absolute frame ceiling.
    pub const fn hard_limit(&self) -> u32 {
        self.hard
    }
    pub(crate) fn evidence(&self, h: &Header) -> AdmissionEvidence {
        let legacy = self.legacy
            && h.data_type() == 1
            && !(self.edition == EditionPolicy::Irig106_24R1 && h.channel() == 0);
        let setup = h.data_type() == 1
            && (self.legacy || (self.edition == EditionPolicy::Irig106_24R1 && h.channel() == 0));
        let soft_limit = if setup { DEFAULT_HARD_LIMIT } else { 524_288 };
        AdmissionEvidence {
            edition: self.edition,
            soft_limit,
            over_soft: h.packet_length() > soft_limit,
            legacy_setup_any_channel: legacy,
            authorization: Authorization::WithinSoft,
        }
    }
}
/// Why a complete frame was allowed; never evidence of payload authenticity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Authorization {
    /// Explicit edition/type threshold permits the size.
    WithinSoft,
    /// A structural primary header was found at this absolute offset.
    #[non_exhaustive]
    NextHeader {
        /// Probe location, not proof of the following body.
        offset: u64,
    },
    /// Final frame admitted without a following header.
    Unconfirmed(UnconfirmedReason),
    /// Caller explicitly authorized oversoft non-seekable input.
    StreamOptIn,
}
/// Accepted but unconfirmed boundary evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnconfirmedReason {
    /// Declared end equals stable known EOF.
    ExactEof,
}
/// Scalar admission evidence attached to a complete frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AdmissionEvidence {
    /// Explicit edition selection, including raw uncertainty.
    pub edition: EditionPolicy,
    /// Complete-frame soft threshold.
    pub soft_limit: u32,
    /// Whether Packet Length exceeds that threshold.
    pub over_soft: bool,
    /// Explicit legacy policy supplied an allowance unavailable under the edition default.
    pub legacy_setup_any_channel: bool,
    /// Size authorization used.
    pub authorization: Authorization,
}
/// Comparable caller-provided stable-source snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceStamp {
    /// Byte length of the source.
    pub size: u64,
    /// Identity of the exclusive source, chosen by its adapter.
    pub identity: u128,
    /// Revision/modification evidence, chosen by its adapter.
    pub revision: u128,
}
impl SourceStamp {
    /// Construct an adapter snapshot. The caller excludes undetectable same-size writes.
    pub const fn new(size: u64, identity: u128, revision: u128) -> Self {
        Self {
            size,
            identity,
            revision,
        }
    }
}
/// Exclusive seekable source with stable identity/size/revision evidence.
/// Read+Seek alone cannot promise stability. Implementors exclude in-place mutation;
/// unchanged stamps cannot prove absence of undetectable same-size concurrent writes.
/// No cloned/shared cursor may be used concurrently. Initial logical position is zero.
pub trait StableSource: Read + Seek {
    /// Snapshot identity/size and available modification evidence without moving the cursor.
    fn source_stamp(&mut self) -> io::Result<SourceStamp>;
}
/// Typed admission refusal; no policy failure initiates sync recovery.
#[derive(Debug)]
#[non_exhaustive]
pub enum AdmissionError {
    /// The absolute ceiling wins even over setup allowances.
    #[non_exhaustive]
    HardLimit {
        /// Declared complete-frame length.
        length: u32,
        /// Caller hard ceiling.
        limit: u32,
    },
    /// Oversoft size lacked acceptable authorization.
    Uncorroborated,
    /// Observed size/identity/revision changed before delivery.
    SourceChanged,
    /// Offset/length cannot be represented safely on this target.
    TargetLimit,
}
impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HardLimit { length, limit } => {
                write!(f, "frame length {length} exceeds hard ceiling {limit}")
            }
            Self::Uncorroborated => f.write_str("oversoft frame is uncorroborated"),
            Self::SourceChanged => f.write_str("stable source changed"),
            Self::TargetLimit => f.write_str("frame exceeds target representation limits"),
        }
    }
}
impl std::error::Error for AdmissionError {}
pub(crate) fn seek_retry<R: Seek>(source: &mut R, position: SeekFrom) -> io::Result<u64> {
    loop {
        match source.seek(position) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}
pub(crate) fn probe<R: StableSource>(source: &mut R, end: u64) -> io::Result<bool> {
    let position = seek_retry(source, SeekFrom::Current(0))?;
    let result = (|| {
        seek_retry(source, SeekFrom::Start(end))?;
        let mut raw = [0; 24];
        match source.read_exact(&mut raw) {
            Ok(()) => Ok(Header::parse(&raw)
                .and_then(|h| PacketRanges::from_header(&h))
                .is_ok()),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
            Err(e) => Err(e),
        }
    })();
    seek_retry(source, SeekFrom::Start(position))?; // Restoration errors fail closed too.
    result
}
