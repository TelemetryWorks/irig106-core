//! Structural parsing for IRIG 106 Chapter 10 recordings.
//!
//! Primary framing and bounded streaming capture using the 106-24R1 layout.
//! Typed input completion/recovery limits and secondary/trailer validation
//! remain subsequent slices; this is not complete recording validation.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod header;
mod reader;

pub use header::{Header, HeaderError, PRIMARY_HEADER_LEN, primary_checksum};
pub use irig106_types::Rtc;
pub use reader::{Event, Packet, PacketReader, ReaderLimits, TailKind};
