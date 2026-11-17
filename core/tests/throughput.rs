//! Cost of the shell integration scanner on the pty output (AC8,
//! `docs/AUTOCOMPLETE.md`, section 6: "loss below 1% on a `cat` of 50 MB").
//!
//! These are benchmarks, so they are `#[ignore]`d and meant for a release
//! build:
//!
//! ```text
//! cargo test --release -p termbay-core --test throughput -- --ignored --nocapture --test-threads=1
//! ```
//!
//! - `pty_cat_with_and_without_scanner` (the goal) runs `cat` of a 50 MB
//!   file through a real pty and the real batcher, alternating "off" (output
//!   straight to the batcher, as in `PtyBridge::open` without a tap) and
//!   "on" (scanner + `ShellLink` in front of the batcher, as `Tap::output`
//!   does). It prints the medians and how busy the reader thread was, and
//!   asserts the median loss is below 1%.
//! - `scanner_cpu_cost` (information) runs the [`OscScanner`] over 50 MB in
//!   memory, in chunks the size of a pty read, next to the copy the bridge
//!   already does without integration, and prints the extra CPU time as a
//!   share of the pty time. It shows how much headroom is left on systems
//!   with a faster pty than the one measured.
//!
//! Without integration there is nothing to measure: `ShellSupport::prepare`
//! returns `None`, no scanner exists and the reader hands the bytes to the
//! batcher exactly as before (see `app/src-tauri/src/bridge.rs` and the test
//! `disabled_bridge_passes_private_sequences_through` in the app).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use termbay_core::pty::{
    BatchLimits, PtyEvent, PtySession, PtySize, ShellSpec, SpawnOptions, spawn_batcher,
};
use termbay_core::shell_integration::{Nonce, OscScanner, ShellKind, ShellLink, ShellMark};

const SIZE: usize = 50 * 1024 * 1024;
const NONCE: &str = "0123456789abcdef0123456789abcdef";
const ROUNDS: usize = 7;
const PTY_ROUNDS: usize = 12;

/// Colored log lines: an ESC every ~20 bytes, which sends the scanner
/// through its `ESC` path far more often than plain text.
fn ansi_log(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size + 256);
    let mut i: u64 = 0;
    while out.len() < size {
        i += 1;
        let level = [
            "\x1b[32mINFO\x1b[0m ",
            "\x1b[33mWARN\x1b[0m ",
            "\x1b[31mERR \x1b[0m ",
        ][(i % 3) as usize];
        out.extend_from_slice(
            format!(
                "\x1b[2m2026-11-16T12:{:02}:{:02}\x1b[0m {level}\x1b[36mtermbay::pty\x1b[0m read {} bytes from session {} \x1b]133;D;0\x07\x1b]7;file://host/tmp\x07\n",
                (i / 60) % 60,
                i % 60,
                i * 7 % 65536,
                i % 97
            )
            .as_bytes(),
        );
    }
    out.truncate(size);
    out
}

/// Plain ASCII text, no escape sequences.
fn plain_text(size: usize) -> Vec<u8> {
    let line =
        b"The quick brown fox jumps over the lazy dog 0123456789 abcdefghijklmnopqrstuvwxyz\n";
    line.iter().copied().cycle().take(size).collect()
}

struct TempFile(PathBuf);

impl TempFile {
    fn new(tag: &str, content: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "termbay-bench-{tag}-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, content).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

fn mb_per_s(bytes: usize, time: Duration) -> f64 {
    bytes as f64 / 1e6 / time.as_secs_f64()
}

/// Result of one `cat` through the pty.
struct CatRun {
    time: Duration,
    bytes: usize,
    reads: usize,
    /// Bytes read from the pty, before the scanner.
    raw: usize,
    /// Time the reader thread spent in the output callback.
    busy: Duration,
}

/// `cat <file>` in a pty, with the output going through the batcher, as in
/// the bridge. With `scanner`, the output first goes through the scanner and
/// the link, exactly like `Tap::output` in `app/src-tauri/src/complete.rs`.
fn cat_through_pty(file: &Path, scanner: bool) -> CatRun {
    let shown = Arc::new(AtomicUsize::new(0));
    let reads = Arc::new(AtomicUsize::new(0));
    let raw = Arc::new(AtomicUsize::new(0));
    let raw_count = Arc::clone(&raw);
    let (done_tx, done_rx) = mpsc::channel();
    let counter = Arc::clone(&shown);
    let batcher = spawn_batcher(
        BatchLimits::default(),
        move |chunk: Vec<u8>| {
            counter.fetch_add(chunk.len(), Ordering::Relaxed);
        },
        move |()| {
            let _ = done_tx.send(());
        },
    )
    .unwrap();

    let mut tap = scanner.then(|| {
        (
            OscScanner::new(&Nonce::new(NONCE)),
            Arc::new(ShellLink::new(ShellKind::Zsh)),
            Vec::<u8>::new(),
            Vec::<ShellMark>::new(),
        )
    });
    let options = SpawnOptions {
        shell: Some(ShellSpec::new("/bin/cat").arg(file)),
        size: PtySize {
            rows: 50,
            cols: 200,
        },
        ..SpawnOptions::default()
    };
    let read_count = Arc::clone(&reads);
    let busy = Arc::new(AtomicUsize::new(0));
    let busy_ns = Arc::clone(&busy);
    let start = Instant::now();
    let session = PtySession::spawn(options, move |event| match event {
        PtyEvent::Output(bytes) => {
            let begin = Instant::now();
            read_count.fetch_add(1, Ordering::Relaxed);
            raw_count.fetch_add(bytes.len(), Ordering::Relaxed);
            match tap.as_mut() {
                None => batcher.output(bytes),
                Some((scanner, link, out, marks)) => {
                    out.clear();
                    scanner.feed(bytes, out, marks);
                    if !out.is_empty() {
                        batcher.output(out);
                    }
                    if !marks.is_empty() {
                        link.apply(marks.drain(..));
                    }
                }
            }
            let spent = usize::try_from(begin.elapsed().as_nanos()).unwrap_or(usize::MAX);
            busy_ns.fetch_add(spent, Ordering::Relaxed);
        }
        PtyEvent::Exit(_) => batcher.end(()),
    })
    .unwrap();
    done_rx
        .recv_timeout(Duration::from_secs(300))
        .expect("cat did not finish");
    let time = start.elapsed();
    drop(session);
    CatRun {
        time,
        bytes: shown.load(Ordering::Relaxed),
        reads: reads.load(Ordering::Relaxed),
        raw: raw.load(Ordering::Relaxed),
        busy: Duration::from_nanos(busy.load(Ordering::Relaxed) as u64),
    }
}

/// Average size of a pty read on this system, measured with a short `cat`.
fn typical_read_size() -> usize {
    let file = TempFile::new("probe", &plain_text(4 * 1024 * 1024));
    let run = cat_through_pty(&file.0, false);
    (run.bytes / run.reads.max(1)).max(1)
}

/// CPU time of the integrated path minus the plain path, over `data` cut
/// into `chunk`-byte reads. Best of several runs, to drop scheduler noise.
fn scanner_extra_cpu(data: &[u8], chunk: usize) -> (Duration, Duration) {
    let mut best_plain = Duration::MAX;
    let mut best_scan = Duration::MAX;
    for _ in 0..ROUNDS {
        // Plain path: the batcher copies every read into a Vec.
        let start = Instant::now();
        let mut total = 0usize;
        for piece in data.chunks(chunk) {
            let copy = piece.to_vec();
            total += std::hint::black_box(copy).len();
        }
        assert_eq!(total, data.len());
        best_plain = best_plain.min(start.elapsed());

        // Integrated path: scanner into a reused buffer, then the same copy.
        let mut scanner = OscScanner::new(&Nonce::new(NONCE));
        let mut out = Vec::new();
        let mut marks = Vec::new();
        let start = Instant::now();
        let mut total = 0usize;
        for piece in data.chunks(chunk) {
            out.clear();
            scanner.feed(piece, &mut out, &mut marks);
            let copy = out.to_vec();
            total += std::hint::black_box(copy).len();
        }
        best_scan = best_scan.min(start.elapsed());
        assert_eq!(total, data.len(), "no private sequence in the data");
        assert!(marks.is_empty());
    }
    (best_plain, best_scan)
}

#[test]
#[ignore = "benchmark: run with --release -- --ignored --nocapture"]
fn scanner_cpu_cost() {
    let read = typical_read_size();
    let file = TempFile::new("cpu", &ansi_log(SIZE));
    let mut pty_times: Vec<f64> = (0..3)
        .map(|_| cat_through_pty(&file.0, false).time.as_secs_f64())
        .collect();
    let pty_time = median(&mut pty_times);

    println!(
        "pty read size ~{read} B; cat of 50 MB through the pty (off): {:.3} s ({:.0} MB/s)",
        pty_time,
        SIZE as f64 / 1e6 / pty_time
    );
    for (name, data) in [
        ("ansi log", ansi_log(SIZE)),
        ("plain text", plain_text(SIZE)),
    ] {
        let (plain, scan) = scanner_extra_cpu(&data, read);
        let extra = scan.saturating_sub(plain).as_secs_f64();
        // Reference: a SIMD search for the whole prefix, which does not stop
        // at every ESC of a colored output.
        let finder = memchr::memmem::Finder::new(b"\x1b]6973;");
        let mut best_memmem = Duration::MAX;
        for _ in 0..ROUNDS {
            let start = Instant::now();
            let found: usize = data
                .chunks(read)
                .map(|piece| finder.find_iter(std::hint::black_box(piece)).count())
                .sum();
            best_memmem = best_memmem.min(start.elapsed());
            assert_eq!(found, 0);
        }
        println!(
            "{name}: copy only {:.1} ms ({:.0} MB/s), scanner + copy {:.1} ms ({:.0} MB/s); extra {:.1} ms = {:.2}% of the pty time; memmem of the prefix alone {:.1} ms",
            plain.as_secs_f64() * 1e3,
            mb_per_s(SIZE, plain),
            scan.as_secs_f64() * 1e3,
            mb_per_s(SIZE, scan),
            extra * 1e3,
            extra / pty_time * 100.0,
            best_memmem.as_secs_f64() * 1e3
        );
    }
}

#[test]
#[ignore = "benchmark: run with --release -- --ignored --nocapture"]
fn pty_cat_with_and_without_scanner() {
    for (name, data) in [
        ("ansi log", ansi_log(SIZE)),
        ("plain text", plain_text(SIZE)),
    ] {
        let file = TempFile::new("pty", &data);
        drop(data);
        // Warm up the page cache and the pty.
        cat_through_pty(&file.0, false);
        cat_through_pty(&file.0, true);
        let mut off = Vec::new();
        let mut on = Vec::new();
        let mut busy_off = Vec::new();
        let mut busy_on = Vec::new();
        // Alternate which one goes first, so neither always gets the
        // warmer cache.
        for round in 0..PTY_ROUNDS {
            for scanner in [round % 2 == 0, round % 2 != 0] {
                let run = cat_through_pty(&file.0, scanner);
                assert!(run.bytes >= SIZE, "output lost: {} bytes", run.bytes);
                let share = run.busy.as_secs_f64() / run.time.as_secs_f64();
                if scanner {
                    on.push(run.time.as_secs_f64());
                    busy_on.push(share);
                } else {
                    off.push(run.time.as_secs_f64());
                    busy_off.push(share);
                }
            }
        }
        let off = median(&mut off);
        let on = median(&mut on);
        let loss = (on - off) / off;
        println!(
            "{name}: off {:.3} s ({:.0} MB/s), on {:.3} s ({:.0} MB/s), difference {:+.2}% (median of {PTY_ROUNDS}); reader thread busy {:.1}% off, {:.1}% on",
            off,
            SIZE as f64 / 1e6 / off,
            on,
            SIZE as f64 / 1e6 / on,
            loss * 100.0,
            median(&mut busy_off) * 100.0,
            median(&mut busy_on) * 100.0
        );
        assert!(
            loss < 0.01,
            "{name}: the scanner costs {:.2}% of the throughput (goal: below 1%)",
            loss * 100.0
        );
    }
}

/// Not a benchmark: makes sure the generated data has no private sequence
/// (the CPU benchmark asserts every byte comes out) and that the pty path
/// used by the benchmarks delivers every byte, scanner on and off.
#[test]
fn benchmark_paths_deliver_every_byte() {
    let data = ansi_log(256 * 1024);
    assert!(!data.windows(7).any(|w| w == b"\x1b]6973;"));
    let file = TempFile::new("check", &data);
    for scanner in [false, true] {
        let run = cat_through_pty(&file.0, scanner);
        // Every byte read reaches the batcher. (The pty turns each LF into
        // CR LF; Linux sometimes adds a CR LF at the end, so the count read
        // is compared with the count shown, not with the file.)
        let lines = data.iter().filter(|&&b| b == b'\n').count();
        assert!(run.raw >= data.len() + lines, "scanner: {scanner}");
        assert_eq!(run.bytes, run.raw, "scanner: {scanner}");
    }
}
