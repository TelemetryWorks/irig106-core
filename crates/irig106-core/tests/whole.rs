//! Hand-packed frame/admission probes; no encoder or capture-reader oracle.
use irig106_core::*;
use std::{io::{self, Cursor, Read, Seek, SeekFrom}, time::{Duration, Instant}};
fn frame(p: u32, channel: u16, kind: u8, flags: u8) -> Vec<u8> {
    let mut b = vec![0x5a; p as usize];
    b[..24].fill(0);
    b[..2].copy_from_slice(&[0x25,0xeb]);
    b[2..4].copy_from_slice(&channel.to_le_bytes());
    b[4..8].copy_from_slice(&p.to_le_bytes());
    b[8..12].copy_from_slice(&4u32.to_le_bytes());
    b[14]=flags; b[15]=kind;
    let sum = b[..22].chunks_exact(2).fold(0u16, |s,w| s.wrapping_add(u16::from_le_bytes([w[0],w[1]])));
    b[22..24].copy_from_slice(&sum.to_le_bytes()); b
}
fn deadline() -> Instant { Instant::now()+Duration::from_secs(2) }
#[derive(Debug)]
struct Stable { input: Cursor<Vec<u8>>, change: bool }
impl Read for Stable {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> { self.input.read(b) }
}
impl Seek for Stable {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> { self.input.seek(pos) }
}
impl StableSource for Stable {
    fn source_stamp(&mut self) -> io::Result<SourceStamp> {
        Ok(SourceStamp::new(self.input.get_ref().len() as u64, 1,
            u128::from(self.change && self.input.position() > 24)))
    }
}
fn stable(b: Vec<u8>) -> Stable { Stable { input: Cursor::new(b), change:false } }
#[test]
fn direct_blocks_admission_eof_and_truncation() {
    let pool=BlockPool::with_defaults().unwrap();
    let mut buf=PacketBuf::new(pool.clone()).unwrap();
    let mut bytes=frame(524_292,7,9,0);
    bytes.extend(frame(28,0,9,0));
    let mut r=WholePacketReader::seek_checked(stable(bytes), WholePolicy::new(), pool.clone()).unwrap();
    assert!(matches!(r.next_packet_into(&mut buf,deadline()).unwrap(),Some(WholeEvent::Packet)));
    assert_eq!(buf.info().unwrap().admission.authorization,Authorization::NextHeader{offset:524_292});
    assert_eq!(buf.frame_spans().count(),2);
    assert_eq!(buf.frame_spans().map(<[u8]>::len).sum::<usize>(),524_292);
    assert_eq!(buf.data_spans().flatten().copied().collect::<Vec<_>>(),[0x5a;4]);
    assert!(matches!(r.next_packet_into(&mut buf,deadline()).unwrap(),Some(WholeEvent::Packet)));
    assert_eq!(buf.contiguous().unwrap().len(),28);
    assert!(r.next_packet_into(&mut buf,deadline()).unwrap().is_none());
    for (cut,changed) in [(0,false),(1,false),(0,true)] {
        let mut b=frame(524_292,7,9,0); b.truncate(b.len()-cut);
        let mut source=stable(b); source.change=changed;
        let mut r=WholePacketReader::seek_checked(source,WholePolicy::new(),pool.clone()).unwrap();
        let result=r.next_packet_into(&mut buf,deadline());
        if changed { assert!(matches!(result.unwrap_err().reason,ErrorKind::Admission{source:AdmissionError::SourceChanged,..})); }
        else if cut==0 {
            assert!(matches!(result.unwrap(),Some(WholeEvent::Packet)));
            assert_eq!(buf.info().unwrap().admission.authorization,Authorization::Unconfirmed(UnconfirmedReason::ExactEof));
        } else {
            assert!(matches!(result.unwrap_err().reason,ErrorKind::NoCompletePacket{tail:Some(Tail{kind:TailKind::TruncatedPacket,bytes:524_291,..}),..}));
            assert!(buf.info().is_none());
        }
    }
}
#[test]
fn refusals_legacy_and_no_speculative_growth() {
    let pool=BlockPool::with_defaults().unwrap(); let mut b=PacketBuf::new(pool.clone()).unwrap();
    assert_eq!(WholePolicy::new().hard_limit(),134_217_728);
    for (policy,accepted) in [(WholePolicy::new(),false),(WholePolicy::new().with_legacy_setup_any_channel(true),true)] {
        let mut r=WholePacketReader::stream(Cursor::new(frame(524_292,4,1,0)),policy,pool.clone()).unwrap();
        let result=r.next_packet_into(&mut b,deadline());
        if accepted { assert!(matches!(result.unwrap(),Some(WholeEvent::Packet))); assert!(b.info().unwrap().admission.legacy_setup_any_channel); }
        else { assert!(matches!(result.unwrap_err().reason,ErrorKind::Admission{source:AdmissionError::Uncorroborated,..})); assert_eq!(r.position(),24); }
    }
    let mut raw=frame(28,0,9,0); raw[4..8].copy_from_slice(&0xffff_fffcu32.to_le_bytes());
    let sum=raw[..22].chunks_exact(2).fold(0u16,|s,w|s.wrapping_add(u16::from_le_bytes([w[0],w[1]]))); raw[22..24].copy_from_slice(&sum.to_le_bytes());
    let mut r=WholePacketReader::stream(Cursor::new(raw),WholePolicy::new(),pool).unwrap();
    assert!(matches!(r.next_packet_into(&mut b,deadline()).unwrap_err().reason,ErrorKind::Admission{source:AdmissionError::HardLimit{..},..}));
    assert!(r.next_packet_into(&mut b,deadline()).unwrap().is_none());
}
