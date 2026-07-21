//! Bounded inspection of the CFB container around a possible V8 DGN file.
//!
//! This module deliberately does not decode the proprietary DGN V8 streams.

use std::io::Cursor;
use std::path::{Component, Path};

use crate::{detect_format, DgnError, DgnFormat};

/// Default maximum number of non-root CFB directory entries to inspect.
pub const DEFAULT_MAX_CFB_ENTRIES: usize = 100_000;

const DGN_HEADER_STREAM: &str = "/Dgn~H";
const DGN_SUMMARY_STREAM: &str = "/Dgn~S";
const DGN_MODELS_STORAGE: &str = "/Dgn-Md";

fn portable_cfb_path(path: &Path) -> String {
    let mut portable = String::new();
    for component in path.components() {
        if let Component::Normal(name) = component {
            portable.push('/');
            portable.push_str(name.to_string_lossy().as_ref());
        }
    }
    if portable.is_empty() {
        portable.push('/');
    }
    portable
}

/// Kind of one directory entry in a CFB container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8CfbEntryKind {
    /// Nested CFB storage (directory).
    Storage,
    /// CFB stream (file-like byte sequence).
    Stream,
}

impl V8CfbEntryKind {
    /// Stable short label used by language bindings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "storage",
            Self::Stream => "stream",
        }
    }
}

/// Metadata for one non-root CFB directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8CfbEntry {
    /// Absolute path inside the CFB container, using `/` separators on every platform.
    pub path: String,
    /// Whether this entry is a storage or stream.
    pub kind: V8CfbEntryKind,
    /// Stream length in bytes; storages do not have a byte length.
    pub size_bytes: Option<u64>,
}

/// Structural metadata for a possible DGN V8 CFB container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V8ContainerInfo {
    /// CFB major version (3 uses 512-byte sectors, 4 uses 4096-byte sectors).
    pub cfb_version: u16,
    /// Whether the three DGN-specific root markers inspected here are present.
    ///
    /// This is a marker check, not validation of the proprietary stream schema.
    pub has_dgn_v8_markers: bool,
    /// Required DGN marker paths that were not found with the expected kind.
    pub missing_markers: Vec<String>,
    /// Direct model storage paths found below `/Dgn-Md`.
    pub model_storage_paths: Vec<String>,
    /// All non-root storages and streams in preorder.
    pub entries: Vec<V8CfbEntry>,
}

/// Inspects the CFB directory of a possible V8 DGN without decoding DGN data.
///
/// A successful marker check only establishes that the container has the
/// expected DGN-specific root entries. It does not validate or parse their
/// proprietary contents.
pub fn inspect_v8_container(input: &[u8], max_entries: usize) -> Result<V8ContainerInfo, DgnError> {
    let format = detect_format(input)?;
    if format != DgnFormat::V8Cfb {
        return Err(DgnError::ExpectedV8Container { format });
    }

    let compound = cfb::CompoundFile::open(Cursor::new(input)).map_err(|error| {
        DgnError::InvalidV8Container {
            context: error.to_string(),
        }
    })?;

    let mut entries = Vec::new();
    for entry in compound.walk().skip(1) {
        if entries.len() == max_entries {
            return Err(DgnError::CfbEntryLimitExceeded { limit: max_entries });
        }
        let kind = if entry.is_stream() {
            V8CfbEntryKind::Stream
        } else {
            V8CfbEntryKind::Storage
        };
        entries.push(V8CfbEntry {
            path: portable_cfb_path(entry.path()),
            kind,
            size_bytes: entry.is_stream().then(|| entry.len()),
        });
    }

    let mut missing_markers = Vec::new();
    if !compound.is_stream(DGN_HEADER_STREAM) {
        missing_markers.push(DGN_HEADER_STREAM.to_owned());
    }
    if !compound.is_stream(DGN_SUMMARY_STREAM) {
        missing_markers.push(DGN_SUMMARY_STREAM.to_owned());
    }
    if !compound.is_storage(DGN_MODELS_STORAGE) {
        missing_markers.push(DGN_MODELS_STORAGE.to_owned());
    }

    let model_prefix = format!("{DGN_MODELS_STORAGE}/").to_ascii_lowercase();
    let mut model_storage_paths: Vec<_> = entries
        .iter()
        .filter(|entry| {
            let path = entry.path.to_ascii_lowercase();
            let suffix = path.strip_prefix(&model_prefix);
            entry.kind == V8CfbEntryKind::Storage
                && suffix.is_some_and(|suffix| !suffix.contains('/') && suffix.starts_with('#'))
        })
        .map(|entry| entry.path.clone())
        .collect();
    model_storage_paths.sort();

    Ok(V8ContainerInfo {
        cfb_version: compound.version().number(),
        has_dgn_v8_markers: missing_markers.is_empty(),
        missing_markers,
        model_storage_paths,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    fn generic_cfb() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut compound = cfb::OpenOptions::new().create_with(cursor).unwrap();
        compound.create_storage("/documents").unwrap();
        let mut stream = compound.create_stream("/documents/readme").unwrap();
        stream.write_all(b"not a DGN").unwrap();
        drop(stream);
        compound.into_inner().into_inner()
    }

    #[test]
    fn generic_cfb_is_not_claimed_as_dgn() {
        let bytes = generic_cfb();
        let info = inspect_v8_container(&bytes, DEFAULT_MAX_CFB_ENTRIES).unwrap();
        assert!(!info.has_dgn_v8_markers);
        assert_eq!(
            info.missing_markers,
            [DGN_HEADER_STREAM, DGN_SUMMARY_STREAM, DGN_MODELS_STORAGE]
        );
        assert!(info.model_storage_paths.is_empty());
        assert!(info
            .entries
            .iter()
            .any(|entry| { entry.path == "/documents" && entry.kind == V8CfbEntryKind::Storage }));
        assert!(info.entries.iter().any(|entry| {
            entry.path == "/documents/readme" && entry.kind == V8CfbEntryKind::Stream
        }));
        assert!(info.entries.iter().all(|entry| !entry.path.contains('\\')));
    }

    #[test]
    fn renders_cfb_paths_with_portable_separators() {
        assert_eq!(
            portable_cfb_path(Path::new("/Dgn-Md/#000000")),
            "/Dgn-Md/#000000"
        );
        assert_eq!(portable_cfb_path(Path::new("/")), "/");
    }

    #[test]
    fn bounds_directory_entries() {
        let bytes = generic_cfb();
        assert!(matches!(
            inspect_v8_container(&bytes, 0),
            Err(DgnError::CfbEntryLimitExceeded { limit: 0 })
        ));
    }

    #[test]
    fn rejects_truncated_cfb_and_other_dgn_family() {
        let bytes = generic_cfb();
        let truncated = &bytes[..512];
        assert!(matches!(
            inspect_v8_container(truncated, DEFAULT_MAX_CFB_ENTRIES),
            Err(DgnError::InvalidV8Container { .. })
        ));
        assert!(matches!(
            inspect_v8_container(&[0x08, 0x09, 0xfe, 0x02], DEFAULT_MAX_CFB_ENTRIES),
            Err(DgnError::ExpectedV8Container {
                format: DgnFormat::V7(_)
            })
        ));
    }
}
