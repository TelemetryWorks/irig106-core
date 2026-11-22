//! Structural parsing for IRIG 106 Chapter 10 recordings.
//!
//! Primary framing and bounded streaming capture using the 106-24R1 layout.
//! Includes typed completion/errors, optional recovery limits, secondary checksum
//! evidence and trailer ranges. No payload meaning or full recording validation.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;
mod header;
mod integrity;
mod reader;

pub use error::{Error, ErrorKind, Recovery, Tail};
pub use header::{Header, HeaderError, PRIMARY_HEADER_LEN, primary_checksum};
pub use integrity::{DataChecksumState, PacketRanges, SecondaryIntegrity};
pub use irig106_types::Rtc;
pub use reader::{Event, Packet, PacketReader, ReaderLimits, TailKind};
