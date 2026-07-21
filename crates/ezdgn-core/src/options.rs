/// Largest record permitted by the V7 16-bit `words_to_follow` field.
pub const MAX_V7_RECORD_SIZE_BYTES: usize = 2 * (u16::MAX as usize + 2);

/// Default complete-file safety limit (1 GiB).
pub const DEFAULT_MAX_FILE_SIZE_BYTES: usize = 1024 * 1024 * 1024;

/// Default maximum number of records in one scan.
pub const DEFAULT_MAX_RECORDS: usize = 1_000_000;

/// Resource limits applied by the raw V7 record scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanOptions {
    pub max_file_size: usize,
    pub max_records: usize,
    pub max_record_size: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_file_size: DEFAULT_MAX_FILE_SIZE_BYTES,
            max_records: DEFAULT_MAX_RECORDS,
            max_record_size: MAX_V7_RECORD_SIZE_BYTES,
        }
    }
}
