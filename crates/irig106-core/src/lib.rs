//! Structural parsing for IRIG 106 Chapter 10 recordings.
//!
//! Primary framing and bounded streaming capture using the 106-24R1 layout.
//! Includes typed completion/errors, optional recovery limits, secondary checksum
//! evidence, trailer ranges, bounded header inventory and per-channel sequence
//! comparisons. No payload meaning or
//! full recording validation.
//!
//! A payload consumer can request a bounded prefix and check completeness before
//! decoding. The following independent frame declares four DATA bytes:
//! ```
//! use irig106_core::{Event, PacketReader, ReaderLimits};
//! let bytes = [0x25,0xEB,0,0,28,0,0,0,4,0,0,0,0,0,0,9,0,0,0,0,0,0,0x45,0xF4,1,2,3,4];
//! let mut reader = PacketReader::new(&bytes[..], ReaderLimits::new().with_capture_bytes(4))?;
//! match reader.next_packet(|_| 4)? {
//!     Some(Event::Packet(packet)) if packet.body_prefix.len() == packet.header.data_length() as usize => {
//!         assert_eq!(packet.body_prefix, [1,2,3,4]); // Complete DATA, still uninterpreted.
//!     }
//!     _ => panic!("fixture must deliver complete DATA"),
//! }
//! # Ok::<(), irig106_core::Error>(())
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod admission;
mod error;
mod header;
mod inspection;
mod integrity;
mod pool;
mod reader;
mod sequence;
mod whole;

pub use error::{Error, ErrorKind, Recovery, Tail};
pub use header::{Header, HeaderError, PRIMARY_HEADER_LEN, primary_checksum};
pub use inspection::{
    AdmissionCounts, ChannelStats, Counts, Inspection, InspectionError, InspectionLimits, Presence,
    PresenceFacts, ScanStatus,
};
pub use integrity::{DataChecksumState, PacketRanges, SecondaryIntegrity};
pub use irig106_types::Rtc;
pub use reader::{Event, Packet, PacketReader, ReaderLimits, TailKind};
pub use sequence::{SequenceKind, SequenceObservation};

pub use pool::{
    BLOCK_BYTES, BlockAssembly, BlockPool, DEFAULT_POOL_BUDGET, PoolError, PoolStats, PooledBlock,
};

pub use admission::{
    AdmissionError, AdmissionEvidence, Authorization, DEFAULT_HARD_LIMIT, DEFAULT_READ_AHEAD_BYTES,
    DEFAULT_SMALL_BODY_CUTOFF, EditionPolicy, SourceStamp, StableSource, UnconfirmedReason,
    WholePolicy,
};
pub use whole::{
    CoalescedPacket, CoalescingEvidence, DrainDisposition, DrainError, DrainEvent, DrainReason,
    DrainReport, PacketBuf, PacketInfo, WholeEvent, WholePacketReader,
};
