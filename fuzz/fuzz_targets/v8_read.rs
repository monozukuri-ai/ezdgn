#![no_main]

use ezdgn_core::{read_v8, V8ReadOptions, V8ScanOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let options = V8ScanOptions {
        read: V8ReadOptions {
            max_file_size: 16 * 1024 * 1024,
            max_cfb_entries: 4_096,
            max_stream_size: 8 * 1024 * 1024,
            max_total_stream_bytes: 32 * 1024 * 1024,
        },
        max_pages: 1_024,
        max_objects: 100_000,
        max_object_size: 256 * 1024,
        max_inflated_stream_size: 8 * 1024 * 1024,
        max_total_inflated_bytes: 32 * 1024 * 1024,
        max_models: 256,
        max_string_bytes: 256 * 1024,
        max_vertices: 100_000,
        max_hierarchy_depth: 128,
    };
    let _ = read_v8(data, options);
});
