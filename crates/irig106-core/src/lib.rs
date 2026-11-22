//! Structural parsing for IRIG 106 Chapter 10 recordings.
//!
//! Primary framing and bounded streaming capture using the 106-24R1 layout.
//! Includes typed completion/errors and optional recovery limits. Secondary and
//! trailer validation remain subsequent slices; no full recording validation.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;
mod header;
mod reader;

pub use error::{Error, ErrorKind, Recovery, Tail};
pub use header::{Header, HeaderError, PRIMARY_HEADER_LEN, primary_checksum};
pub use irig106_types::Rtc;
pub use reader::{Event, Packet, PacketReader, ReaderLimits, TailKind};
