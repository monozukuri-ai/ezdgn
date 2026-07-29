use std::hint::black_box;
use std::time::Instant;

use ezdgn_core::{read_v8, V8ScanOptions};

const V8: &[u8] = include_bytes!("../../../tests/data/dgn/v8/test_dgnv8.dgn");

fn main() {
    let iterations = std::env::var("EZDGN_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1_000)
        .max(1);

    let warmup = read_v8(V8, V8ScanOptions::default()).expect("V8 warmup parse failed");
    assert_eq!(warmup.models.len(), 1);
    assert_eq!(warmup.models[0].entities().count(), 34);

    let start = Instant::now();
    for _ in 0..iterations {
        black_box(read_v8(black_box(V8), V8ScanOptions::default()).expect("V8 parse failed"));
    }
    let elapsed = start.elapsed();
    let seconds = elapsed.as_secs_f64();
    let mib = V8.len() as f64 * iterations as f64 / (1024.0 * 1024.0);
    println!("iterations={iterations}");
    println!("elapsed_seconds={seconds:.6}");
    println!("parses_per_second={:.2}", iterations as f64 / seconds);
    println!("input_mib_per_second={:.2}", mib / seconds);
}
