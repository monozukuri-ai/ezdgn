# V8 fuzz target

Install `cargo-fuzz` and run the bounded native V8 reader target from the
repository root:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
cargo +nightly fuzz run v8_read -- -max_len=16777216
```

The harness applies lower file, stream, inflated-output, object, vertex, string,
and hierarchy limits than the public defaults so malformed inputs cannot turn a
fuzz worker into an unbounded decompression or allocation job.

For a finite local smoke run, append `-runs=1000`. `cargo check
--manifest-path fuzz/Cargo.toml` verifies the harness on the workspace's stable
toolchain; sanitizer-backed execution requires nightly Rust.
