//! Actual hybrid implementation versus its configured direct-read baseline.
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
    watchdog(Duration::from_secs(120), || read_benchmark_case(false));
}
#[test]
#[ignore = "checksum cost evidence only; release-mode CI, no timing gate"]
fn data_checksum_cost_benchmark() {
    watchdog(Duration::from_secs(120), || read_benchmark_case(true));
}
fn read_benchmark_case(checksums: bool) {
    use std::{
        fs::{File, OpenOptions},
        io::{BufWriter, Write},
        time::SystemTime,
    };
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
        policy: WholePolicy,
    ) -> Sample {
        let mut reader = WholePacketReader::stream(input, policy, pool.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let before = kernel_reads();
        let start = Instant::now();
        let mut seen = 0usize;
        let mut digest = 0usize;
        while let Some(event) = reader.next_packet_into(buf, deadline).unwrap() {
            assert!(matches!(event, WholeEvent::Packet));
            let bytes = std::hint::black_box(buf.contiguous().unwrap());
            digest = digest.wrapping_add(usize::from(bytes[0]));
            std::hint::black_box(buf.info().unwrap().ranges.data_checksum);
            seen += 1;
        }
        let seconds = start.elapsed().as_secs_f64();
        let after = kernel_reads();
        let kernel = before.zip(after).map(|(a, b)| b - a - 1);
        assert_eq!(seen, packets);
        assert_eq!(digest, packets * 0x25);
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
    if !checksums {
        println!(
            "small_body benchmark: actual adaptive hybrid (64 KiB, cutoff 4 KiB) versus direct baseline (24-byte header buffer, cutoff 0), median of 3, alternating order; generation/construction/first block initialization excluded; warm cache; EOF read included"
        );
        println!(
            "small_body,size,width_bytes,mode,packets,bytes,source_reads,kernel_read_syscalls,seconds,packets_per_s,MB_per_s"
        );
    }
    let label = if checksums {
        "data_checksum"
    } else {
        "small_body"
    };
    println!(
        "{label}: median of 3 alternating modes; checksum comparison uses same actual hybrid and valid fixtures, generation/construction/warm-up excluded; warm-cache informational throughput"
    );
    let widths: &[usize] = if checksums { &[1, 2, 4] } else { &[0] };
    for (width, case, p) in widths.iter().flat_map(|&width| {
        [28usize, 400, 4096, 65_536, 524_288]
            .into_iter()
            .enumerate()
            .map(move |(case, p)| (width, case, p))
    }) {
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
            65_536,
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path.0)
                .unwrap(),
        );
        let flag = match width {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 3,
        };
        let mut bytes = frame(p as u32, 0, 9, flag);
        bytes[8..12].copy_from_slice(&((p - 24 - width) as u32).to_le_bytes());
        if width > 0 {
            let sum: u64 = bytes[24..p - width]
                .iter()
                .enumerate()
                .map(|(i, &b)| u64::from(b) << (8 * (i % width)))
                .sum();
            bytes[p - width..].copy_from_slice(&(sum as u32).to_le_bytes()[..width]);
        }
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
                let policy = if checksums {
                    WholePolicy::new().with_data_checksum_verification(mode == 1)
                } else if mode == 0 {
                    WholePolicy::new()
                        .with_read_ahead_bytes(24)
                        .with_small_body_cutoff(0)
                } else {
                    WholePolicy::new()
                };
                let sample = measure(
                    file,
                    |source| source.calls,
                    &pool,
                    &mut buf,
                    packets,
                    policy,
                );
                samples[mode].push(sample);
            }
        }
        for (index, mut group) in samples.into_iter().enumerate() {
            group.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
            let median = &group[1];
            let mode = if checksums {
                if index == 0 {
                    "checksum_off"
                } else {
                    "checksum_on"
                }
            } else if index == 0 {
                "direct"
            } else {
                "adaptive_hybrid_64KiB_cutoff4KiB"
            };
            let kernel = median
                .kernel
                .map_or_else(|| "unsupported".to_owned(), |n| n.to_string());
            println!(
                "{label},{p},{width},{mode},{packets},{total},{},{kernel},{:.6},{:.1},{:.1}",
                median.calls,
                median.seconds,
                packets as f64 / median.seconds,
                total as f64 / median.seconds / 1e6
            );
        }
    }
}
