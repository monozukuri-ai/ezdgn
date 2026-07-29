use std::fs;
use std::path::Path;

use ezdgn_core::{read_v8_streams, V8ReadOptions, V8StreamSet};
use sha2::{Digest, Sha256};

pub fn load_streams(path: &Path) -> Result<V8StreamSet, String> {
    let input =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    read_v8_streams(&input, V8ReadOptions::default())
        .map_err(|error| format!("failed to extract {}: {error}", path.display()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
