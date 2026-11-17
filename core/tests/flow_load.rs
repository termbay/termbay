//! Load test for the pty flow control (task F2, section 6.5 of
//! `docs/FASE1-PLANO.md`). Ignored by default because it runs for several
//! seconds; the CI runs it on Linux and macOS with
//! `cargo test -p termbay-core --test flow_load -- --ignored --nocapture`.
//!
//! A shell runs `yes` in the pty while a consumer thread plays the UI: it takes each chunk, waits
//! as long as a renderer of a given speed would, and acknowledges it. Each
//! scenario reports throughput and the largest number of bytes in flight.
//!
//! What fails the test (a "large regression"), independent of machine speed:
//! - bytes in flight above the high water mark plus one read (64 KiB);
//! - with a consumer that never waits, the gated throughput below half of
//!   the ungated throughput measured in the same run;
//! - with a slow consumer, throughput not following the consumer.
//!
//! `TERMBAY_FLOW_SECONDS` sets the length of each scenario (default 3 s).
//! When `GITHUB_STEP_SUMMARY` is set, a table is appended to it.
#![cfg(unix)]

use std::io::Write;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use termbay_core::pty::{
    FLOW_HIGH_WATER, FLOW_LOW_WATER, FlowControl, PtyEvent, PtySession, ReadGate, ShellSpec,
    SpawnOptions,
};

/// Largest single read of the session reader.
const READ_CHUNK: u64 = 64 * 1024;

struct Outcome {
    label: String,
    mb_per_s: f64,
    max_in_flight: u64,
}

fn seconds() -> Duration {
    let secs = std::env::var("TERMBAY_FLOW_SECONDS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(3.0);
    Duration::from_secs_f64(secs)
}

/// Floods the pty for `duration`. `high` = `None` reads without a gate.
/// `consumer_mb_s` = `None` acknowledges immediately.
fn run(label: &str, high: Option<u64>, consumer_mb_s: Option<f64>, duration: Duration) -> Outcome {
    let gate = high.map(|_| Arc::new(ReadGate::new()));
    let flow = Arc::new(Mutex::new(FlowControl::new(
        high.unwrap_or(u64::MAX),
        FLOW_LOW_WATER.min(high.unwrap_or(u64::MAX) / 4),
    )));
    let max_in_flight = Arc::new(Mutex::new(0u64));
    let (tx, rx) = mpsc::channel::<usize>();

    let options = SpawnOptions {
        shell: Some(ShellSpec::new("/bin/sh")),
        env: vec![
            ("ENV".into(), "/dev/null".into()),
            ("PS1".into(), "$ ".into()),
        ],
        read_gate: gate.clone(),
        ..SpawnOptions::default()
    };
    let session = {
        let gate = gate.clone();
        let flow = Arc::clone(&flow);
        let max_in_flight = Arc::clone(&max_in_flight);
        PtySession::spawn(options, move |event| {
            if let PtyEvent::Output(bytes) = event {
                if let Some(gate) = &gate {
                    let mut flow = flow.lock().unwrap();
                    if flow.sent(bytes.len() as u64) {
                        gate.pause();
                    }
                    let mut max = max_in_flight.lock().unwrap();
                    *max = (*max).max(flow.in_flight());
                }
                let _ = tx.send(bytes.len());
            }
        })
        .expect("spawn /bin/sh")
    };
    session
        .write(b"yes yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy\r")
        .unwrap();

    // Let the loop start, then measure.
    let warmup = Instant::now() + Duration::from_millis(300);
    let mut total = 0u64;
    let mut started: Option<Instant> = None;
    loop {
        let now = Instant::now();
        if let Some(start) = started
            && now.duration_since(start) >= duration
        {
            break;
        }
        let Ok(n) = rx.recv_timeout(Duration::from_millis(50)) else {
            continue;
        };
        if let Some(rate) = consumer_mb_s {
            thread::sleep(Duration::from_secs_f64(n as f64 / (rate * 1e6)));
        }
        if let Some(gate) = &gate
            && flow.lock().unwrap().acked(n as u64)
        {
            gate.resume();
        }
        if started.is_none() && Instant::now() >= warmup {
            started = Some(Instant::now());
            *max_in_flight.lock().unwrap() = flow.lock().unwrap().in_flight();
        } else if started.is_some() {
            total += n as u64;
        }
    }
    let elapsed = started.unwrap().elapsed().as_secs_f64();
    let _ = session.write(b"\x03");
    session.kill().unwrap();
    let max = *max_in_flight.lock().unwrap();
    Outcome {
        label: label.to_owned(),
        mb_per_s: total as f64 / elapsed / 1e6,
        max_in_flight: max,
    }
}

fn report(outcomes: &[Outcome]) {
    let mut table = String::from(
        "### Flow control load (F2)\n\n| scenario | MB/s | max in flight (KiB) |\n|---|---:|---:|\n",
    );
    for o in outcomes {
        table.push_str(&format!(
            "| {} | {:.1} | {} |\n",
            o.label,
            o.mb_per_s,
            o.max_in_flight / 1024
        ));
    }
    println!("{table}");
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY")
        && let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path)
    {
        let _ = writeln!(file, "{table}");
    }
}

#[test]
#[ignore = "load test, several seconds; run with --ignored"]
fn flow_control_keeps_throughput_and_bounds_the_backlog() {
    let d = seconds();
    let one_mib = 1024 * 1024;
    let outcomes = vec![
        run("no gate, instant consumer", None, None, d),
        run("512 KiB, instant consumer", Some(FLOW_HIGH_WATER), None, d),
        run("1 MiB, instant consumer", Some(one_mib), None, d),
        run(
            "512 KiB, consumer at 15 MB/s",
            Some(FLOW_HIGH_WATER),
            Some(15.0),
            d,
        ),
        run("1 MiB, consumer at 15 MB/s", Some(one_mib), Some(15.0), d),
    ];
    report(&outcomes);

    let ungated = outcomes[0].mb_per_s;
    for o in &outcomes[1..] {
        let high = if o.label.starts_with("1 MiB") {
            one_mib
        } else {
            FLOW_HIGH_WATER
        };
        assert!(
            o.max_in_flight <= high + READ_CHUNK,
            "{}: {} bytes in flight, limit {}",
            o.label,
            o.max_in_flight,
            high + READ_CHUNK
        );
    }
    for o in &outcomes[1..3] {
        assert!(
            o.mb_per_s >= ungated * 0.5,
            "{}: {:.1} MB/s, ungated {:.1} MB/s",
            o.label,
            o.mb_per_s,
            ungated
        );
    }
    // A slow renderer: the reader follows it instead of queueing without limit.
    // Only meaningful when the pty itself is faster than the consumer.
    if ungated > 20.0 {
        for o in &outcomes[3..] {
            assert!(
                o.mb_per_s > 15.0 * 0.5 && o.mb_per_s < 15.0 * 1.2,
                "{}: {:.1} MB/s with a 15 MB/s consumer",
                o.label,
                o.mb_per_s
            );
        }
    }
}
