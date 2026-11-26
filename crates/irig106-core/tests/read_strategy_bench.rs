//! Decision-only small-body read benchmark; production strategy is unchanged.
use irig106_core::*;
use std::{
    io::{self, Read},
    time::{Duration, Instant},
};
#[path = "support/watchdog.rs"]
mod support;
use support::watchdog;

fn frame(p: u32, channel: u16, kind: u8, flags: u8) -> Vec<u8> {
    let mut b = vec![0x5a; p as usize];
    b[..24].fill(0);
    b[..2].copy_from_slice(&[0x25, 0xeb]);
    b[2..4].copy_from_slice(&channel.to_le_bytes());
    b[4..8].copy_from_slice(&p.to_le_bytes());
    b[8..12].copy_from_slice(&4u32.to_le_bytes());
    b[14] = flags;
    b[15] = kind;
    let sum = b[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    b[22..24].copy_from_slice(&sum.to_le_bytes());
    b
}
struct Counted<R> {
    input: R,
    calls: usize,
}
impl<R: Read> Read for Counted<R> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        self.input.read(b)
    }
}

#[test]
#[ignore = "owner decision evidence only; release-mode matrix runs explicitly in CI"]
fn small_body_read_strategy_benchmark() {
    watchdog(
        Duration::from_secs(120),
        small_body_read_strategy_benchmark_case,
    );
}
fn small_body_read_strategy_benchmark_case() {
    use std::{
        fs::{File, OpenOptions},
        io::{BufReader, BufWriter, Write},
        time::SystemTime,
    };
    const BUFFER: usize = 65_536;
    // Exactly one counter read. Linux accounts that read after producing its
    // snapshot; subtract its contribution from the following snapshot delta.
    fn kernel_reads() -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            let mut raw = [0; 512];
            let n = File::open("/proc/thread-self/io")
                .ok()?
                .read(&mut raw)
                .ok()?;
            std::str::from_utf8(&raw[..n])
                .ok()?
                .lines()
                .find_map(|line| line.strip_prefix("syscr: "))?
                .trim()
                .parse()
                .ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    struct Sample {
        seconds: f64,
        calls: usize,
        kernel: Option<u64>,
    }
    fn measure<R: Read>(
        input: R,
        count: impl FnOnce(R) -> usize,
        pool: &BlockPool,
        buf: &mut PacketBuf,
        packets: usize,
        p: usize,
    ) -> Sample {
        let mut reader =
            WholePacketReader::stream(input, WholePolicy::new(), pool.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let before = kernel_reads();
        let start = Instant::now();
        let mut seen = 0usize;
        let mut digest = 0usize;
        while let Some(event) = reader.next_packet_into(buf, deadline).unwrap() {
            assert!(matches!(event, WholeEvent::Packet));
            let bytes = std::hint::black_box(buf.contiguous().unwrap());
            digest = digest.wrapping_add(usize::from(bytes[0]) + usize::from(bytes[p - 1]));
            seen += 1;
        }
        let seconds = start.elapsed().as_secs_f64();
        let after = kernel_reads();
        let kernel = before.zip(after).map(|(a, b)| b - a - 1);
        assert_eq!(seen, packets);
        assert_eq!(digest, packets * (0x25 + 0x5a));
        let calls = count(reader.into_inner());
        #[cfg(target_os = "linux")]
        assert_eq!(
            kernel,
            Some(calls as u64),
            "kernel counter calibration / unexpected read syscalls"
        );
        Sample {
            seconds,
            calls,
            kernel,
        }
    }
    let target = std::env::var("IRIG106_SMALL_BODY_BENCH_BYTES")
        .ok()
        .map(|v| v.parse::<usize>().unwrap())
        .unwrap_or(16 * 1024 * 1024);
    assert!((524_288..=32 * 1024 * 1024).contains(&target));
    println!(
        "small_body benchmark: fixed 64 KiB buffered-copy prototype, median of 3, alternating order; generation/construction/first block initialization excluded; warm cache; EOF read included"
    );
    println!(
        "small_body,size,mode,packets,bytes,source_reads,kernel_read_syscalls,seconds,packets_per_s,MB_per_s"
    );
    for (case, p) in [28usize, 400, 4096, 65_536, 524_288]
        .into_iter()
        .enumerate()
    {
        let packets = target / p;
        let total = packets * p;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!(
            "irig106-core-small-{}-{stamp}-{p}.bench",
            std::process::id()
        );
        let path = Temp(std::env::temp_dir().join(name));
        let mut output = BufWriter::with_capacity(
            BUFFER,
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path.0)
                .unwrap(),
        );
        let mut bytes = frame(p as u32, 0, 9, 0);
        bytes[8..12].copy_from_slice(&((p - 24) as u32).to_le_bytes());
        let checksum = bytes[..22].chunks_exact(2).fold(0u16, |sum, w| {
            sum.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
        });
        bytes[22..24].copy_from_slice(&checksum.to_le_bytes());
        for _ in 0..packets {
            output.write_all(&bytes).unwrap();
        }
        output.flush().unwrap();
        drop(output);
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut warm = pool.try_begin(1).unwrap();
        drop(warm.try_next_block().unwrap());
        drop(warm);
        let mut samples: [Vec<Sample>; 2] = std::array::from_fn(|_| Vec::with_capacity(3));
        for repetition in 0..3 {
            for turn in 0..2 {
                let mode = (case + repetition + turn) % 2;
                let file = Counted {
                    input: File::open(&path.0).unwrap(),
                    calls: 0,
                };
                let sample = if mode == 0 {
                    measure(file, |source| source.calls, &pool, &mut buf, packets, p)
                } else {
                    // Test-only prototype: same production reader; an external
                    // fixed BufReader copies small bodies, bypasses large reads.
                    measure(
                        BufReader::with_capacity(BUFFER, file),
                        |source| source.into_inner().calls,
                        &pool,
                        &mut buf,
                        packets,
                        p,
                    )
                };
                samples[mode].push(sample);
            }
        }
        for (index, mut group) in samples.into_iter().enumerate() {
            group.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
            let median = &group[1];
            let mode = if index == 0 {
                "direct"
            } else {
                "buffered_copy_64KiB"
            };
            let kernel = median
                .kernel
                .map_or_else(|| "unsupported".to_owned(), |n| n.to_string());
            println!(
                "small_body,{p},{mode},{packets},{total},{},{kernel},{:.6},{:.1},{:.1}",
                median.calls,
                median.seconds,
                packets as f64 / median.seconds,
                total as f64 / median.seconds / 1e6
            );
        }
    }
}
