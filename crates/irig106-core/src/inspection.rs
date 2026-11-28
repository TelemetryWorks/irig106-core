//! Bounded header inventory. No payload bytes, packet history or diagnostics.
use crate::{
    AdmissionEvidence, Authorization, DataChecksumState, Event, Header, PacketInfo,
    SequenceObservation, UnconfirmedReason,
};
use std::{collections::BTreeMap, fmt};

/// Explicit entry budgets; zero permits no entries. No preallocation occurs.
/// ```compile_fail,E0639
/// use irig106_core::InspectionLimits;
/// fn closed(value: InspectionLimits) { let _ = InspectionLimits { ..value }; }
/// ```
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct InspectionLimits {
    /// Maximum distinct u16 channels.
    pub max_channels: usize,
    /// Maximum distinct (channel, raw data-type) rows.
    pub max_rows: usize,
}
impl InspectionLimits {
    /// Choose both budgets explicitly; no hidden default row/channel limit.
    pub const fn new(max_channels: usize, max_rows: usize) -> Self {
        Self {
            max_channels,
            max_rows,
        }
    }
}

/// Complete-frame totals; packet bytes exclude recovered junk and EOF tails.
/// ```compile_fail,E0639
/// use irig106_core::Counts;
/// fn closed(value: Counts) { let _ = Counts { ..value }; }
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Counts {
    /// Number of accepted complete frames.
    pub packets: u64,
    /// Sum of declared DATA bytes, independent of captured prefix size.
    pub data_bytes: u64,
    /// Sum of complete Packet Lengths, including headers and trailers.
    pub packet_bytes: u64,
}
impl Counts {
    fn after(self, h: &Header) -> Result<Self, InspectionError> {
        Ok(Self {
            packets: self
                .packets
                .checked_add(1)
                .ok_or(InspectionError::CountOverflow)?,
            data_bytes: self
                .data_bytes
                .checked_add(u64::from(h.data_length()))
                .ok_or(InspectionError::CountOverflow)?,
            packet_bytes: self
                .packet_bytes
                .checked_add(u64::from(h.packet_length()))
                .ok_or(InspectionError::CountOverflow)?,
        })
    }
}
/// Admission evidence from successfully accumulated complete whole packets only.
/// Capture events carry no admission evidence; compare evaluated_packets with
/// totals().packets for coverage. Over-soft and legacy counters overlap the
/// authorization partition; zero never proves absent evidence in omitted input.
/// ```compile_fail,E0639
/// use irig106_core::AdmissionCounts;
/// fn closed(value: AdmissionCounts) { let _ = AdmissionCounts { ..value }; }
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AdmissionCounts {
    /// Complete packets whose admission evidence was observed.
    pub evaluated_packets: u64,
    /// Packet Length exceeded its applicable soft threshold.
    pub over_soft: u64,
    /// Authorization was WithinSoft.
    pub within_soft: u64,
    /// Authorization was NextHeader, not proof of the following body's validity.
    pub next_header: u64,
    /// Authorization was Unconfirmed(ExactEof), not confirmation of content.
    pub unconfirmed_exact_eof: u64,
    /// Caller explicitly authorized oversoft stream input.
    pub stream_opt_in: u64,
    /// The explicit legacy allowance was used, not merely enabled.
    pub legacy_setup_any_channel: u64,
}
impl AdmissionCounts {
    fn after(mut self, evidence: Option<&AdmissionEvidence>) -> Result<Self, InspectionError> {
        let Some(e) = evidence else {
            return Ok(self);
        };
        for (counter, increment) in [
            (&mut self.evaluated_packets, true),
            (&mut self.over_soft, e.over_soft),
            (
                &mut self.within_soft,
                matches!(e.authorization, Authorization::WithinSoft),
            ),
            (
                &mut self.next_header,
                matches!(e.authorization, Authorization::NextHeader { .. }),
            ),
            (
                &mut self.unconfirmed_exact_eof,
                matches!(
                    e.authorization,
                    Authorization::Unconfirmed(UnconfirmedReason::ExactEof)
                ),
            ),
            (
                &mut self.stream_opt_in,
                matches!(e.authorization, Authorization::StreamOptIn),
            ),
            (
                &mut self.legacy_setup_any_channel,
                e.legacy_setup_any_channel,
            ),
        ] {
            *counter = counter
                .checked_add(u64::from(increment))
                .ok_or(InspectionError::CountOverflow)?;
        }
        Ok(self)
    }
}
/// Complete-frame checksum outcomes, including capture/opt-out coverage gaps.
/// These fixed counters partition totals().packets; mismatch is not a refusal.
/// ```compile_fail,E0639
/// use irig106_core::DataChecksumCounts;
/// fn closed(c: DataChecksumCounts) { let _ = DataChecksumCounts { ..c }; }
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataChecksumCounts {
    /// No checksum declared.
    pub absent: u64,
    /// Declared but not verified (capture or explicit opt-out).
    pub not_checked: u64,
    /// Stored and computed values agree.
    pub verified: u64,
    /// Complete packet has differing sums.
    pub mismatch: u64,
}
impl DataChecksumCounts {
    fn after(mut self, state: DataChecksumState) -> Result<Self, InspectionError> {
        let counter = match state {
            DataChecksumState::Absent => &mut self.absent,
            DataChecksumState::NotChecked => &mut self.not_checked,
            DataChecksumState::Verified { .. } => &mut self.verified,
            DataChecksumState::Mismatch { .. } => &mut self.mismatch,
        };
        *counter = counter
            .checked_add(1)
            .ok_or(InspectionError::CountOverflow)?;
        Ok(self)
    }
}
/// One (channel, raw data-type) row; exists only after a successful observation.
/// ```compile_fail,E0639
/// use irig106_core::ChannelStats;
/// fn closed(value: ChannelStats) { let _ = ChannelStats { ..value }; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ChannelStats {
    /// Complete-frame totals for the row.
    pub counts: Counts,
    /// Smallest declared DATA size.
    pub min_data_length: u32,
    /// Largest declared DATA size.
    pub max_data_length: u32,
}
/// Whether the unfiltered input scan reached clean EOF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScanStatus {
    /// EOF has not been declared.
    InProgress,
    /// Caller declared clean EOF after at least one complete frame.
    Complete,
    /// Tail, read failure, stopped scan or inventory failure.
    Incomplete,
}
/// Header observation only, never proof of valid contents or enabled features.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presence {
    /// Matching header was successfully accumulated.
    Present,
    /// No matching header in a complete scan without recovered/skipped bytes.
    Absent,
    /// No matching header, but scan is incomplete or includes recovered bytes.
    Unknown,
}
/// Presence facts derived without decoding setup/index/event payloads.
/// ```compile_fail,E0639
/// use irig106_core::PresenceFacts;
/// fn closed(value: PresenceFacts) { let _ = PresenceFacts { ..value }; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PresenceFacts {
    /// Channel0/type01 setup routing; type01 on other channels does not count.
    pub setup_record: Presence,
    /// Any type03 header, not an assertion that indexing is enabled.
    pub recording_index: Presence,
    /// Any type02 header, not an assertion that events are enabled.
    pub recording_events: Presence,
}
/// Terminal inventory failure; the rejected observation changes no counts/rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InspectionError {
    /// A new channel exceeds the explicit budget.
    #[non_exhaustive]
    ChannelLimit {
        /// Configured maximum.
        limit: usize,
    },
    /// A new row exceeds the explicit budget.
    #[non_exhaustive]
    RowLimit {
        /// Configured maximum.
        limit: usize,
    },
    /// A checked counter addition would exceed u64.
    CountOverflow,
    /// Inventory is already complete or incomplete and no longer accepts events.
    Stopped,
}
impl fmt::Display for InspectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChannelLimit { limit } => {
                write!(f, "inspection channel limit of {limit} exceeded")
            }
            Self::RowLimit { limit } => write!(f, "inspection row limit of {limit} exceeded"),
            Self::CountOverflow => f.write_str("inspection counters exceed u64 range"),
            Self::Stopped => f.write_str("inspection is no longer accepting events"),
        }
    }
}
impl std::error::Error for InspectionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChannelSequence {
    last: u8,
    skipped_checkpoint: u64,
}

/// Inventory bounded by caller entry budgets plus 256 fixed data-type counters.
/// Feed every event without filtering; call finish only on clean reader EOF,
/// and mark_incomplete on reader error or early stop. Tails are sticky incomplete.
/// Recovered bytes keep unseen presence facts Unknown even after clean EOF.
#[derive(Debug)]
pub struct Inspection {
    limits: InspectionLimits,
    totals: Counts,
    admission: AdmissionCounts,
    data_checksums: DataChecksumCounts,
    channels: BTreeMap<u16, ChannelSequence>,
    skipped_bytes: u64,
    rows: BTreeMap<(u16, u8), ChannelStats>,
    types: [Counts; 256],
    seen: [bool; 3],
    saw_recovery: bool,
    status: ScanStatus,
}
impl Inspection {
    /// Construct an empty inventory with explicit budgets.
    pub fn new(limits: InspectionLimits) -> Self {
        Self {
            limits,
            totals: Counts::default(),
            admission: AdmissionCounts::default(),
            data_checksums: DataChecksumCounts::default(),
            channels: BTreeMap::new(),
            skipped_bytes: 0,
            rows: BTreeMap::new(),
            types: [Counts::default(); 256],
            seen: [false; 3],
            saw_recovery: false,
            status: ScanStatus::InProgress,
        }
    }
    /// Accept a complete packet or terminal tail. Errors stop further observation.
    pub fn observe(&mut self, event: &Event) -> Result<(), InspectionError> {
        self.observe_with_sequence(event).map(|_| ())
    }
    /// Accept once, returning sequence evidence for packets and None for tails.
    /// Shares all counts, limits and failure rules with observe; do not feed twice.
    pub fn observe_with_sequence(
        &mut self,
        event: &Event,
    ) -> Result<Option<SequenceObservation>, InspectionError> {
        let result = if self.status != ScanStatus::InProgress {
            Err(InspectionError::Stopped)
        } else {
            match event {
                Event::Packet(p) => self
                    .record(
                        &p.header,
                        p.offset,
                        p.recovered_bytes,
                        None,
                        p.ranges.data_checksum,
                    )
                    .map(Some),
                Event::Tail { .. } => {
                    self.mark_incomplete();
                    Ok(None)
                }
            }
        };
        if result.is_err() {
            self.mark_incomplete();
        }
        result
    }
    /// Accept one complete whole-packet metadata view without copying payloads.
    /// Feed every accepted frame exactly once, on the ordered reader path, before
    /// returning/reusing its PacketBuf. Uses the existing explicit entry budgets.
    /// Reader tails/errors/early stop require mark_incomplete; call finish only
    /// on clean EOF. Do not also observe a capture event for the same frame.
    pub fn observe_packet_info(
        &mut self,
        packet: &PacketInfo,
    ) -> Result<SequenceObservation, InspectionError> {
        let result = if self.status != ScanStatus::InProgress {
            Err(InspectionError::Stopped)
        } else {
            self.record(
                &packet.header,
                packet.offset,
                packet.recovered_bytes,
                Some(&packet.admission),
                packet.ranges.data_checksum,
            )
        };
        if result.is_err() {
            self.mark_incomplete();
        }
        result
    }
    fn record(
        &mut self,
        h: &Header,
        offset: u64,
        recovered_bytes: u64,
        evidence: Option<&AdmissionEvidence>,
        checksum: DataChecksumState,
    ) -> Result<SequenceObservation, InspectionError> {
        let key = (h.channel(), h.data_type());
        if !self.channels.contains_key(&key.0) && self.channels.len() >= self.limits.max_channels {
            return Err(InspectionError::ChannelLimit {
                limit: self.limits.max_channels,
            });
        }
        if !self.rows.contains_key(&key) && self.rows.len() >= self.limits.max_rows {
            return Err(InspectionError::RowLimit {
                limit: self.limits.max_rows,
            });
        }
        // Compute every fallible value before changing any inventory state.
        let admission = self.admission.after(evidence)?;
        let data_checksums = self.data_checksums.after(checksum)?;
        let skipped_bytes = self
            .skipped_bytes
            .checked_add(recovered_bytes)
            .ok_or(InspectionError::CountOverflow)?;
        let totals = self.totals.after(h)?;
        let type_counts = self.types[key.1 as usize].after(h)?;
        let old = self.rows.get(&key).copied();
        let row = ChannelStats {
            counts: old.map_or(Counts::default(), |r| r.counts).after(h)?,
            min_data_length: old
                .map_or(h.data_length(), |r| r.min_data_length.min(h.data_length())),
            max_data_length: old
                .map_or(h.data_length(), |r| r.max_data_length.max(h.data_length())),
        };
        let previous = self.channels.get(&key.0);
        let sequence = SequenceObservation {
            channel: key.0,
            offset,
            previous: previous.map(|s| s.last),
            observed: h.sequence(),
            recovered_bytes,
            skipped_since_previous: previous.map_or(0, |s| skipped_bytes - s.skipped_checkpoint),
        };
        self.channels.insert(
            key.0,
            ChannelSequence {
                last: sequence.observed,
                skipped_checkpoint: skipped_bytes,
            },
        );
        self.skipped_bytes = skipped_bytes;
        self.rows.insert(key, row);
        self.totals = totals;
        self.admission = admission;
        self.data_checksums = data_checksums;
        self.types[key.1 as usize] = type_counts;
        self.seen[0] |= key == (0, 1);
        self.seen[1] |= key.1 == 3;
        self.seen[2] |= key.1 == 2;
        self.saw_recovery |= recovered_bytes > 0;
        Ok(sequence)
    }
    /// Declare clean EOF. Empty inventory and any prior failure remain incomplete.
    pub fn finish(&mut self) {
        if self.status == ScanStatus::InProgress {
            self.status = if self.totals.packets > 0 {
                ScanStatus::Complete
            } else {
                ScanStatus::Incomplete
            };
        }
    }
    /// Record a read error, omitted input or early stop; never reversible by finish.
    pub fn mark_incomplete(&mut self) {
        self.status = ScanStatus::Incomplete;
    }
    /// Current scan-completeness evidence.
    pub fn status(&self) -> ScanStatus {
        self.status
    }
    /// Totals over successful complete-packet observations only.
    pub fn totals(&self) -> Counts {
        self.totals
    }
    /// Admission evidence over accepted whole metadata; capture events do not
    /// fabricate authorization. Retained on tails/errors; read status for scope.
    pub fn admission_counts(&self) -> AdmissionCounts {
        self.admission
    }
    /// Checksum outcomes over all accepted complete observations, including
    /// NotChecked for capture/opt-out. Retained on failure; read status for scope.
    pub fn data_checksum_counts(&self) -> DataChecksumCounts {
        self.data_checksums
    }
    /// Number of retained distinct channels.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
    /// Rows in deterministic (channel,type) order; copies cannot mutate inventory.
    pub fn rows(&self) -> impl Iterator<Item = ((u16, u8), ChannelStats)> + '_ {
        self.rows.iter().map(|(&key, &row)| (key, row))
    }
    /// Nonzero raw type counters, including unknown types, in code order.
    pub fn data_types(&self) -> impl Iterator<Item = (u8, Counts)> + '_ {
        self.types
            .iter()
            .enumerate()
            .filter_map(|(code, &c)| (c.packets > 0).then_some((code as u8, c)))
    }
    /// Observed headers stay Present; incomplete or recovered scans cannot prove absence.
    pub fn presence(&self) -> PresenceFacts {
        let fact = |seen| {
            if seen {
                Presence::Present
            } else if self.status == ScanStatus::Complete && !self.saw_recovery {
                Presence::Absent
            } else {
                Presence::Unknown
            }
        };
        PresenceFacts {
            setup_record: fact(self.seen[0]),
            recording_index: fact(self.seen[1]),
            recording_events: fact(self.seen[2]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PacketReader, ReaderLimits};
    #[test]
    fn every_counter_overflow_preserves_inventory_atomically() {
        // EB25+001C+0004+0900 = F445; one hand-packed complete DATA4 frame.
        let b = [
            0x25, 0xEB, 0, 0, 28, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0x45, 0xF4, 1,
            2, 3, 4,
        ];
        let e = PacketReader::new(&b[..], ReaderLimits::new())
            .unwrap()
            .next_packet(|_| 0)
            .unwrap()
            .unwrap();
        let mut next = b;
        next[13] = 1;
        next[23] = 0xF5; // Sequence1 adds 0100 to the independent F445 checksum.
        let recovered = [&[0xFF][..], &next[..]].concat();
        let rejected = PacketReader::new(&recovered[..], ReaderLimits::new())
            .unwrap()
            .next_packet(|_| 0)
            .unwrap()
            .unwrap();
        for scope in 0..3 {
            for field in 0..3 {
                let mut a = Inspection::new(InspectionLimits::new(1, 1));
                a.observe(&e).unwrap();
                let counts = match scope {
                    0 => &mut a.totals,
                    1 => &mut a.types[9],
                    _ => &mut a.rows.get_mut(&(0, 9)).unwrap().counts,
                };
                match field {
                    0 => counts.packets = u64::MAX,
                    1 => counts.data_bytes = u64::MAX,
                    _ => counts.packet_bytes = u64::MAX,
                };
                let before = (a.totals, a.types[9], a.rows[&(0, 9)], a.skipped_bytes);
                let channels_before = a.channels.clone();
                assert_eq!(a.observe(&rejected), Err(InspectionError::CountOverflow));
                assert_eq!(
                    (a.totals, a.types[9], a.rows[&(0, 9)], a.skipped_bytes),
                    before
                );
                assert_eq!(a.channels, channels_before);
                assert_eq!((a.channel_count(), a.rows().count()), (1, 1));
                a.finish();
                assert_eq!(a.status(), ScanStatus::Incomplete);
            }
        }
    }

    #[test]
    fn checksum_counter_overflow_commits_nothing() {
        let bytes = [
            0x25, 0xeb, 0, 0, 28, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0x45, 0xf4, 1,
            2, 3, 4,
        ];
        for state in [
            DataChecksumState::Absent,
            DataChecksumState::NotChecked,
            DataChecksumState::Verified { width: 4, value: 0 },
            DataChecksumState::Mismatch {
                width: 4,
                stored: 1,
                computed: 0,
            },
        ] {
            let mut event = PacketReader::new(&bytes[..], ReaderLimits::new())
                .unwrap()
                .next_packet(|_| 0)
                .unwrap()
                .unwrap();
            let Event::Packet(ref mut packet) = event else {
                panic!("packet")
            };
            packet.ranges.data_checksum = state;
            let mut a = Inspection::new(InspectionLimits::new(1, 1));
            let c = match state {
                DataChecksumState::Absent => &mut a.data_checksums.absent,
                DataChecksumState::NotChecked => &mut a.data_checksums.not_checked,
                DataChecksumState::Verified { .. } => &mut a.data_checksums.verified,
                DataChecksumState::Mismatch { .. } => &mut a.data_checksums.mismatch,
            };
            *c = u64::MAX - 1;
            a.observe(&event).unwrap();
            let before = (a.data_checksums, a.totals, a.admission, a.skipped_bytes);
            assert_eq!(a.observe(&event), Err(InspectionError::CountOverflow));
            assert_eq!(
                (a.data_checksums, a.totals, a.admission, a.skipped_bytes),
                before
            );
            assert_eq!((a.channel_count(), a.rows().count()), (1, 1));
            assert_eq!(a.status(), ScanStatus::Incomplete);
        }
    }
    #[test]
    fn cumulative_skipped_bytes_accept_exact_max_then_reject_atomically() {
        let mut b = [
            0x25, 0xEB, 0, 0, 28, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0x45, 0xF4, 1,
            2, 3, 4,
        ];
        let e = PacketReader::new(&b[..], ReaderLimits::new())
            .unwrap()
            .next_packet(|_| 0)
            .unwrap()
            .unwrap();
        let mut a = Inspection::new(InspectionLimits::new(1, 1));
        a.observe(&e).unwrap();
        a.skipped_bytes = u64::MAX - 1;
        a.channels.get_mut(&0).unwrap().skipped_checkpoint = u64::MAX - 1;
        b[13] = 1;
        b[23] = 0xF5;
        let input = [&[0xFF][..], &b[..]].concat();
        let e = PacketReader::new(&input[..], ReaderLimits::new())
            .unwrap()
            .next_packet(|_| 0)
            .unwrap()
            .unwrap();
        let s = a.observe_with_sequence(&e).unwrap().unwrap();
        assert_eq!(
            (a.skipped_bytes, s.skipped_since_previous, s.kind()),
            (u64::MAX, 1, crate::SequenceKind::Unverified)
        );
        let before = (a.totals, a.types[9], a.rows[&(0, 9)], a.channels.clone());
        assert_eq!(
            a.observe_with_sequence(&e),
            Err(InspectionError::CountOverflow)
        );
        assert_eq!(
            (a.totals, a.types[9], a.rows[&(0, 9)], a.channels.clone()),
            before
        );
        assert_eq!(a.skipped_bytes, u64::MAX);
        assert_eq!(a.status(), ScanStatus::Incomplete);
    }
    #[test]
    fn admission_overflow_is_atomic_for_every_field_and_other_counts() {
        use crate::{EditionPolicy, PacketRanges};
        let bytes = [
            0x25, 0xeb, 0, 0, 28, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0x45, 0xf4, 1,
            2, 3, 4,
        ];
        for field in 0..8 {
            let header = Header::parse(&bytes).unwrap();
            // Seed arithmetic boundaries directly; supplied evidence is counted,
            // never recomputed from header type/version by the inventory.
            let info = PacketInfo {
                ranges: PacketRanges::from_header(&header).unwrap(),
                header,
                offset: 0,
                recovered_bytes: 1,
                secondary_integrity: None,
                admission: AdmissionEvidence {
                    edition: EditionPolicy::Irig106_24R1,
                    soft_limit: 28,
                    over_soft: true,
                    legacy_setup_any_channel: true,
                    authorization: match field {
                        3 => Authorization::NextHeader { offset: 28 },
                        4 => Authorization::Unconfirmed(UnconfirmedReason::ExactEof),
                        5 => Authorization::StreamOptIn,
                        _ => Authorization::WithinSoft,
                    },
                },
            };
            let mut a = Inspection::new(InspectionLimits::new(1, 1));
            if field < 7 {
                let counters = [
                    &mut a.admission.evaluated_packets,
                    &mut a.admission.over_soft,
                    &mut a.admission.within_soft,
                    &mut a.admission.next_header,
                    &mut a.admission.unconfirmed_exact_eof,
                    &mut a.admission.stream_opt_in,
                    &mut a.admission.legacy_setup_any_channel,
                ];
                *counters[field] = u64::MAX - 1;
                a.observe_packet_info(&info).unwrap(); // Exact MAX still succeeds.
            } else {
                a.totals.packets = u64::MAX;
            }
            let before = (
                a.totals,
                a.admission,
                a.types,
                a.rows.clone(),
                a.channels.clone(),
                a.skipped_bytes,
                a.seen,
                a.saw_recovery,
            );
            assert_eq!(
                a.observe_packet_info(&info),
                Err(InspectionError::CountOverflow)
            );
            assert_eq!(
                (
                    a.totals,
                    a.admission,
                    a.types,
                    a.rows.clone(),
                    a.channels.clone(),
                    a.skipped_bytes,
                    a.seen,
                    a.saw_recovery
                ),
                before
            );
            assert_eq!(a.status(), ScanStatus::Incomplete);
            a.finish();
            assert_eq!(a.admission_counts(), before.1);
            assert_eq!(a.observe_packet_info(&info), Err(InspectionError::Stopped));
        }
    }
}
