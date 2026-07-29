use std::io::Read;
use std::sync::Arc;

use crate::{DgnError, DEFAULT_MAX_FILE_SIZE_BYTES};

use super::container::{
    inspect_compound, open_v8_compound, portable_cfb_path, V8CfbEntryKind, V8Compound,
};
use super::{V8ContainerInfo, DEFAULT_MAX_CFB_ENTRIES};

/// Default maximum size of one extracted CFB stream (256 MiB).
pub const DEFAULT_MAX_CFB_STREAM_SIZE_BYTES: usize = 256 * 1024 * 1024;
/// Default maximum combined size of all extracted streams (1 GiB).
pub const DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES: usize = 1024 * 1024 * 1024;

/// Resource limits for bounded V8 container and stream access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V8ReadOptions {
    pub max_file_size: usize,
    pub max_cfb_entries: usize,
    pub max_stream_size: usize,
    pub max_total_stream_bytes: usize,
}

impl Default for V8ReadOptions {
    fn default() -> Self {
        Self {
            max_file_size: DEFAULT_MAX_FILE_SIZE_BYTES,
            max_cfb_entries: DEFAULT_MAX_CFB_ENTRIES,
            max_stream_size: DEFAULT_MAX_CFB_STREAM_SIZE_BYTES,
            max_total_stream_bytes: DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES,
        }
    }
}

/// One logical CFB stream extracted from a DGN V8 candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8Stream {
    pub path: String,
    pub bytes: Arc<[u8]>,
}

impl V8Stream {
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A bounded snapshot of every stream in one confirmed DGN V8 container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8StreamSet {
    pub container: V8ContainerInfo,
    pub streams: Vec<V8Stream>,
}

impl V8StreamSet {
    /// Returns a stream by its portable absolute CFB path.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&V8Stream> {
        self.streams.iter().find(|stream| stream.path == path)
    }

    /// Combined logical byte length of all extracted streams.
    #[must_use]
    pub fn total_size(&self) -> usize {
        self.streams.iter().map(V8Stream::len).sum()
    }
}

/// Extracts one named stream after validating the V8 markers and configured limits.
pub fn read_v8_stream(
    input: &[u8],
    path: &str,
    options: V8ReadOptions,
) -> Result<V8Stream, DgnError> {
    validate_file_size(input, options)?;
    let mut compound = open_v8_compound(input)?;
    let info = inspect_compound(&compound, options.max_cfb_entries)?;
    require_v8_markers(&info)?;

    let entry = compound
        .entry(path)
        .map_err(|error| DgnError::InvalidV8Stream {
            path: path.to_owned(),
            context: error.to_string(),
        })?;
    if !entry.is_stream() {
        return Err(DgnError::InvalidV8Stream {
            path: portable_cfb_path(entry.path()),
            context: "CFB entry is a storage, not a stream".to_owned(),
        });
    }
    let canonical_path = portable_cfb_path(entry.path());
    let declared_size = entry.len();
    validate_stream_size(&canonical_path, declared_size, options.max_stream_size)?;
    validate_total_size(declared_size, options.max_total_stream_bytes)?;
    read_stream(&mut compound, canonical_path, declared_size)
}

/// Extracts every stream after validating the V8 markers and configured limits.
pub fn read_v8_streams(input: &[u8], options: V8ReadOptions) -> Result<V8StreamSet, DgnError> {
    validate_file_size(input, options)?;
    let mut compound = open_v8_compound(input)?;
    let info = inspect_compound(&compound, options.max_cfb_entries)?;
    require_v8_markers(&info)?;

    let entries = info
        .entries
        .iter()
        .filter(|entry| entry.kind == V8CfbEntryKind::Stream)
        .map(|entry| (entry.path.clone(), entry.size_bytes.unwrap_or(0)))
        .collect::<Vec<_>>();

    let mut total_size = 0_u64;
    for (path, size) in &entries {
        validate_stream_size(path, *size, options.max_stream_size)?;
        total_size =
            total_size
                .checked_add(*size)
                .ok_or(DgnError::CfbTotalStreamSizeLimitExceeded {
                    size: u64::MAX,
                    limit: options.max_total_stream_bytes,
                })?;
        validate_total_size(total_size, options.max_total_stream_bytes)?;
    }

    let mut streams = Vec::with_capacity(entries.len());
    for (path, declared_size) in entries {
        streams.push(read_stream(&mut compound, path, declared_size)?);
    }

    Ok(V8StreamSet {
        container: info,
        streams,
    })
}

fn validate_file_size(input: &[u8], options: V8ReadOptions) -> Result<(), DgnError> {
    if input.len() > options.max_file_size {
        return Err(DgnError::FileSizeLimitExceeded {
            actual: input.len(),
            limit: options.max_file_size,
        });
    }
    Ok(())
}

fn require_v8_markers(info: &V8ContainerInfo) -> Result<(), DgnError> {
    if info.has_dgn_v8_markers {
        Ok(())
    } else {
        Err(DgnError::MissingV8Markers {
            missing: info.missing_markers.clone(),
        })
    }
}

fn validate_stream_size(path: &str, size: u64, limit: usize) -> Result<(), DgnError> {
    if size > limit as u64 {
        return Err(DgnError::CfbStreamSizeLimitExceeded {
            path: path.to_owned(),
            size,
            limit,
        });
    }
    Ok(())
}

fn validate_total_size(size: u64, limit: usize) -> Result<(), DgnError> {
    if size > limit as u64 {
        return Err(DgnError::CfbTotalStreamSizeLimitExceeded { size, limit });
    }
    Ok(())
}

fn read_stream(
    compound: &mut V8Compound<'_>,
    path: String,
    declared_size: u64,
) -> Result<V8Stream, DgnError> {
    let capacity = usize::try_from(declared_size).map_err(|_| DgnError::InvalidV8Stream {
        path: path.clone(),
        context: format!("declared size {declared_size} does not fit usize"),
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    compound
        .open_stream(&path)
        .and_then(|mut stream| stream.read_to_end(&mut bytes))
        .map_err(|error| DgnError::InvalidV8Stream {
            path: path.clone(),
            context: error.to_string(),
        })?;
    if bytes.len() as u64 != declared_size {
        return Err(DgnError::InvalidV8Stream {
            path,
            context: format!(
                "directory declares {declared_size} bytes, but the stream yielded {}",
                bytes.len()
            ),
        });
    }
    Ok(V8Stream {
        path,
        bytes: Arc::from(bytes),
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    fn dgn_cfb() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut compound =
            cfb::CompoundFile::create_with_version(cfb::Version::V3, cursor).unwrap();
        compound.create_storage("/Dgn-Md").unwrap();
        compound.create_storage("/Dgn-Md/#000000").unwrap();
        for (path, bytes) in [
            ("/Dgn~H", b"header".as_slice()),
            ("/Dgn~S", b"summary".as_slice()),
            ("/Dgn-Md/#000000/data", b"geometry".as_slice()),
        ] {
            let mut stream = compound.create_stream(path).unwrap();
            stream.write_all(bytes).unwrap();
        }
        compound.into_inner().into_inner()
    }

    #[test]
    fn extracts_one_or_all_streams_with_portable_paths() {
        let bytes = dgn_cfb();
        let one = read_v8_stream(&bytes, "/Dgn~H", V8ReadOptions::default()).unwrap();
        assert_eq!(one.path, "/Dgn~H");
        assert_eq!(one.as_bytes(), b"header");

        let all = read_v8_streams(&bytes, V8ReadOptions::default()).unwrap();
        assert_eq!(all.container.cfb_version, 3);
        assert!(all.container.has_dgn_v8_markers);
        assert_eq!(all.streams.len(), 3);
        assert_eq!(all.total_size(), 21);
        assert_eq!(all.get("/Dgn~S").unwrap().as_bytes(), b"summary");
    }

    #[test]
    fn rejects_missing_markers_and_non_stream_paths() {
        let cursor = Cursor::new(Vec::new());
        let mut generic = cfb::CompoundFile::create(cursor).unwrap();
        generic.create_storage("/documents").unwrap();
        let generic = generic.into_inner().into_inner();
        assert!(matches!(
            read_v8_streams(&generic, V8ReadOptions::default()),
            Err(DgnError::MissingV8Markers { .. })
        ));

        let bytes = dgn_cfb();
        assert!(matches!(
            read_v8_stream(&bytes, "/Dgn-Md", V8ReadOptions::default()),
            Err(DgnError::InvalidV8Stream { .. })
        ));
        assert!(matches!(
            read_v8_stream(&bytes, "/missing", V8ReadOptions::default()),
            Err(DgnError::InvalidV8Stream { .. })
        ));
    }

    #[test]
    fn enforces_file_stream_total_and_entry_limits_before_extraction() {
        let bytes = dgn_cfb();

        let options = V8ReadOptions {
            max_file_size: bytes.len() - 1,
            ..V8ReadOptions::default()
        };
        assert!(matches!(
            read_v8_streams(&bytes, options),
            Err(DgnError::FileSizeLimitExceeded { .. })
        ));

        let options = V8ReadOptions {
            max_stream_size: 7,
            ..V8ReadOptions::default()
        };
        assert!(matches!(
            read_v8_streams(&bytes, options),
            Err(DgnError::CfbStreamSizeLimitExceeded { .. })
        ));

        let options = V8ReadOptions {
            max_total_stream_bytes: 20,
            ..V8ReadOptions::default()
        };
        assert!(matches!(
            read_v8_streams(&bytes, options),
            Err(DgnError::CfbTotalStreamSizeLimitExceeded { .. })
        ));

        let options = V8ReadOptions {
            max_cfb_entries: 1,
            ..V8ReadOptions::default()
        };
        assert!(matches!(
            read_v8_streams(&bytes, options),
            Err(DgnError::CfbEntryLimitExceeded { limit: 1 })
        ));
    }
}
