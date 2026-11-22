//! Structural parsing for IRIG 106 Chapter 10 recordings.
//!
//! Currently exposes primary-header parsing only, using the 106-24R1 Chapter 11
//! layout. Parsing a header does not establish a complete packet or recording.
//! Streaming, recovery and secondary/trailer checks are subsequent slices.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod header;

pub use header::{Header, HeaderError, PRIMARY_HEADER_LEN, primary_checksum};
pub use irig106_types::Rtc;
