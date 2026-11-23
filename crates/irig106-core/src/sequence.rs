//! Per-channel comparisons of accepted headers; no packet-loss inference.

/// Relationship to the last accepted packet on the same channel, across types.
/// ```compile_fail,E0004
/// use irig106_core::SequenceKind;
/// fn closed(k: SequenceKind) { match k {
///     SequenceKind::First | SequenceKind::Contiguous | SequenceKind::Duplicate |
///     SequenceKind::Discontinuity => ()
/// } }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SequenceKind {
    /// No earlier packet for this channel; its initial number may be arbitrary.
    First,
    /// Increment by one, including 255 to 0.
    Contiguous,
    /// Same number as the last accepted packet; not proof of identical content.
    Duplicate,
    /// Neither the previous number nor its successor; not proof of packet loss.
    Discontinuity,
}

/// One comparison returned to the caller, never retained as a diagnostic history.
/// Recovery does not reset the comparison; consumers choose finding severities.
/// ```compile_fail,E0639
/// use irig106_core::SequenceObservation;
/// fn closed(s: SequenceObservation) { let _ = SequenceObservation { ..s }; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SequenceObservation {
    /// Header channel ID, shared across all data types on that channel.
    pub channel: u16,
    /// Current accepted packet's byte offset.
    pub offset: u64,
    /// Last accepted sequence on this channel; None for the first packet.
    pub previous: Option<u8>,
    /// Current header's raw sequence byte.
    pub observed: u8,
    /// Bytes skipped before this packet; zero does not prove continuity of input.
    pub recovered_bytes: u64,
}
impl SequenceObservation {
    /// Expected successor modulo 256; None before a channel's first packet.
    pub fn expected(&self) -> Option<u8> {
        self.previous.map(|p| p.wrapping_add(1))
    }
    /// Classify the observed bytes without estimating missing-packet counts.
    pub fn kind(&self) -> SequenceKind {
        match self.previous {
            None => SequenceKind::First,
            Some(p) if self.observed == p.wrapping_add(1) => SequenceKind::Contiguous,
            Some(p) if self.observed == p => SequenceKind::Duplicate,
            Some(_) => SequenceKind::Discontinuity,
        }
    }
}
