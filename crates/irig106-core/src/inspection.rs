//! Bounded header inventory. No payload bytes, packet history or diagnostics.
use crate::{Event, Header, Packet};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

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
    /// No matching header in a complete scan.
    Absent,
    /// No matching header, but scan is not complete.
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

/// Inventory bounded by caller entry budgets plus 256 fixed data-type counters.
/// Feed every event without filtering; call finish only on clean reader EOF,
/// and mark_incomplete on reader error or early stop. Tails are sticky incomplete.
#[derive(Debug)]
pub struct Inspection {
    limits: InspectionLimits,
    totals: Counts,
    channels: BTreeSet<u16>,
    rows: BTreeMap<(u16, u8), ChannelStats>,
    types: [Counts; 256],
    seen: [bool; 3],
    status: ScanStatus,
}
impl Inspection {
    /// Construct an empty inventory with explicit budgets.
    pub fn new(limits: InspectionLimits) -> Self {
        Self {
            limits,
            totals: Counts::default(),
            channels: BTreeSet::new(),
            rows: BTreeMap::new(),
            types: [Counts::default(); 256],
            seen: [false; 3],
            status: ScanStatus::InProgress,
        }
    }
    /// Accept a complete packet or terminal tail. Errors stop further observation.
    pub fn observe(&mut self, event: &Event) -> Result<(), InspectionError> {
        let result = if self.status != ScanStatus::InProgress {
            Err(InspectionError::Stopped)
        } else {
            match event {
                Event::Packet(p) => self.record(p),
                Event::Tail { .. } => {
                    self.mark_incomplete();
                    Ok(())
                }
            }
        };
        if result.is_err() {
            self.mark_incomplete();
        }
        result
    }
    fn record(&mut self, packet: &Packet) -> Result<(), InspectionError> {
        let h = &packet.header;
        let key = (h.channel(), h.data_type());
        if !self.channels.contains(&key.0) && self.channels.len() >= self.limits.max_channels {
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
        self.channels.insert(key.0);
        self.rows.insert(key, row);
        self.totals = totals;
        self.types[key.1 as usize] = type_counts;
        self.seen[0] |= key == (0, 1);
        self.seen[1] |= key.1 == 3;
        self.seen[2] |= key.1 == 2;
        Ok(())
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
    /// Observed headers survive incomplete scans; unseen records remain unknown.
    pub fn presence(&self) -> PresenceFacts {
        let fact = |seen| {
            if seen {
                Presence::Present
            } else if self.status == ScanStatus::Complete {
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
                let before = (a.totals, a.types[9], a.rows[&(0, 9)]);
                assert_eq!(a.observe(&e), Err(InspectionError::CountOverflow));
                assert_eq!((a.totals, a.types[9], a.rows[&(0, 9)]), before);
                assert_eq!((a.channel_count(), a.rows().count()), (1, 1));
                a.finish();
                assert_eq!(a.status(), ScanStatus::Incomplete);
            }
        }
    }
}
