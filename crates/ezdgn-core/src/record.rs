use std::iter::FusedIterator;

use crate::io::ByteCursor;
use crate::{detect_format, DgnError, DgnFormat, ScanOptions};

const RECORD_HEADER_SIZE: usize = 4;
const END_MARKER: [u8; 2] = [0xff, 0xff];

/// Decoded four-byte header shared by all V7 element records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawElementHeader {
    pub level: u8,
    pub element_type: u8,
    pub complex_component: bool,
    pub reserved: bool,
    pub deleted: bool,
    pub words_to_follow: u16,
}

impl RawElementHeader {
    fn decode(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() >= RECORD_HEADER_SIZE);
        Self {
            level: bytes[0] & 0x3f,
            complex_component: bytes[0] & 0x80 != 0,
            reserved: bytes[0] & 0x40 != 0,
            element_type: bytes[1] & 0x7f,
            deleted: bytes[1] & 0x80 != 0,
            words_to_follow: u16::from_le_bytes([bytes[2], bytes[3]]),
        }
    }

    /// Complete record length, including this four-byte header.
    #[must_use]
    pub const fn byte_len(self) -> usize {
        2 * (self.words_to_follow as usize + 2)
    }
}

/// Borrowed raw record yielded by [`V7RecordIter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawElementRef<'a> {
    pub index: usize,
    pub offset: usize,
    pub header: RawElementHeader,
    pub bytes: &'a [u8],
}

impl<'a> RawElementRef<'a> {
    /// Bytes after the common four-byte record header.
    #[must_use]
    pub fn payload(self) -> &'a [u8] {
        &self.bytes[RECORD_HEADER_SIZE..]
    }
}

/// How a valid record stream ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStreamEnd {
    /// The stream ended with the explicit `ff ff` marker. Bytes after it are
    /// padding/stale block contents and are intentionally ignored.
    EndMarker {
        offset: usize,
        trailing_bytes: usize,
    },
    /// The file ended exactly at a record boundary without an explicit marker.
    PhysicalEof { offset: usize },
}

impl RecordStreamEnd {
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::EndMarker { .. } => "end_marker",
            Self::PhysicalEof { .. } => "physical_eof",
        }
    }

    #[must_use]
    pub const fn offset(self) -> usize {
        match self {
            Self::EndMarker { offset, .. } | Self::PhysicalEof { offset } => offset,
        }
    }

    #[must_use]
    pub const fn trailing_bytes(self) -> usize {
        match self {
            Self::EndMarker { trailing_bytes, .. } => trailing_bytes,
            Self::PhysicalEof { .. } => 0,
        }
    }
}

/// Iterator over the bounded V7 record stream.
pub struct V7RecordIter<'a> {
    format: DgnFormat,
    cursor: ByteCursor<'a>,
    options: ScanOptions,
    record_index: usize,
    finished: bool,
    termination: Option<RecordStreamEnd>,
}

impl<'a> V7RecordIter<'a> {
    /// Creates a scanner after validating the input family and file-size limit.
    pub fn new(input: &'a [u8], options: ScanOptions) -> Result<Self, DgnError> {
        if input.len() > options.max_file_size {
            return Err(DgnError::FileSizeLimitExceeded {
                actual: input.len(),
                limit: options.max_file_size,
            });
        }

        let format = detect_format(input)?;
        if !matches!(format, DgnFormat::V7(_)) {
            return Err(DgnError::UnsupportedFormat { format });
        }

        Ok(Self {
            format,
            cursor: ByteCursor::new(input),
            options,
            record_index: 0,
            finished: false,
            termination: None,
        })
    }

    #[must_use]
    pub const fn format(&self) -> DgnFormat {
        self.format
    }

    /// Available after the iterator has returned `None` following a successful
    /// scan. It remains `None` when iteration stopped on an error.
    #[must_use]
    pub const fn termination(&self) -> Option<RecordStreamEnd> {
        self.termination
    }

    fn finish_with_error(
        &mut self,
        error: DgnError,
    ) -> Option<Result<RawElementRef<'a>, DgnError>> {
        self.finished = true;
        Some(Err(error))
    }
}

impl<'a> Iterator for V7RecordIter<'a> {
    type Item = Result<RawElementRef<'a>, DgnError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let offset = self.cursor.position();
        let remaining = self.cursor.remaining();
        if remaining == 0 {
            self.finished = true;
            self.termination = Some(RecordStreamEnd::PhysicalEof { offset });
            return None;
        }

        if remaining >= END_MARKER.len()
            && self.cursor.peek_exact(END_MARKER.len(), "end marker").ok()
                == Some(END_MARKER.as_slice())
        {
            if let Err(error) = self.cursor.skip(END_MARKER.len(), "end marker") {
                return self.finish_with_error(error);
            }
            self.finished = true;
            self.termination = Some(RecordStreamEnd::EndMarker {
                offset,
                trailing_bytes: self.cursor.remaining(),
            });
            return None;
        }

        if self.record_index >= self.options.max_records {
            return self.finish_with_error(DgnError::RecordLimitExceeded {
                offset,
                limit: self.options.max_records,
            });
        }

        let header_bytes = match self.cursor.peek_exact(RECORD_HEADER_SIZE, "record header") {
            Ok(bytes) => bytes,
            Err(error) => return self.finish_with_error(error),
        };
        let header = RawElementHeader::decode(header_bytes);
        let byte_len = header.byte_len();

        if byte_len > self.options.max_record_size {
            return self.finish_with_error(DgnError::RecordSizeLimitExceeded {
                offset,
                element_type: header.element_type,
                declared: byte_len,
                limit: self.options.max_record_size,
            });
        }

        if remaining < byte_len {
            return self.finish_with_error(DgnError::TruncatedRecord {
                offset,
                element_type: header.element_type,
                declared: byte_len,
                remaining,
            });
        }

        let bytes = match self.cursor.read_exact(byte_len, "record body") {
            Ok(bytes) => bytes,
            Err(error) => return self.finish_with_error(error),
        };
        let record = RawElementRef {
            index: self.record_index,
            offset,
            header,
            bytes,
        };
        self.record_index += 1;
        Some(Ok(record))
    }
}

impl FusedIterator for V7RecordIter<'_> {}

/// Fully collected zero-copy view of a V7 record stream.
#[derive(Debug)]
pub struct RecordScan<'a> {
    pub format: DgnFormat,
    pub records: Vec<RawElementRef<'a>>,
    pub termination: RecordStreamEnd,
    pub source_size: usize,
}

/// Scans all V7 records while borrowing raw bytes from `input`.
pub fn scan_records(input: &[u8], options: ScanOptions) -> Result<RecordScan<'_>, DgnError> {
    let mut iterator = V7RecordIter::new(input, options)?;
    let format = iterator.format();
    let mut records = Vec::new();
    for record in iterator.by_ref() {
        records.push(record?);
    }
    let termination = iterator
        .termination()
        .unwrap_or(RecordStreamEnd::PhysicalEof {
            offset: input.len(),
        });
    Ok(RecordScan {
        format,
        records,
        termination,
        source_size: input.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{V7Dimension, MAX_V7_RECORD_SIZE_BYTES};

    fn one_record_with_tail(tail: &[u8]) -> Vec<u8> {
        let mut input = vec![0_u8; 1536];
        input[..4].copy_from_slice(&[0x08, 0x09, 0xfe, 0x02]);
        input.extend_from_slice(tail);
        input
    }

    #[test]
    fn header_decodes_flags_and_size() {
        let header = RawElementHeader::decode(&[0xc5, 0x91, 0x03, 0x00]);
        assert_eq!(header.level, 5);
        assert_eq!(header.element_type, 17);
        assert!(header.complex_component);
        assert!(header.reserved);
        assert!(header.deleted);
        assert_eq!(header.words_to_follow, 3);
        assert_eq!(header.byte_len(), 10);
    }

    #[test]
    fn accepts_marker_and_ignores_every_trailing_byte() {
        let input = one_record_with_tail(&[0xff, 0xff, 1, 2, 3, 0xff]);
        let scan = scan_records(&input, ScanOptions::default()).unwrap();
        assert_eq!(scan.format, DgnFormat::V7(V7Dimension::Two));
        assert_eq!(scan.records.len(), 1);
        assert_eq!(
            scan.termination,
            RecordStreamEnd::EndMarker {
                offset: 1536,
                trailing_bytes: 4
            }
        );
    }

    #[test]
    fn accepts_physical_eof_at_record_boundary() {
        let input = one_record_with_tail(&[]);
        let scan = scan_records(&input, ScanOptions::default()).unwrap();
        assert_eq!(scan.records.len(), 1);
        assert_eq!(
            scan.termination,
            RecordStreamEnd::PhysicalEof { offset: 1536 }
        );
    }

    #[test]
    fn rejects_truncated_record_without_panicking() {
        let input = [0x08, 0x09, 0xfe, 0x02];
        assert!(matches!(
            scan_records(&input, ScanOptions::default()),
            Err(DgnError::TruncatedRecord {
                offset: 0,
                element_type: 9,
                declared: 1536,
                remaining: 4,
            })
        ));
    }

    #[test]
    fn rejects_partial_header_after_valid_record() {
        let input = one_record_with_tail(&[0]);
        assert!(matches!(
            scan_records(&input, ScanOptions::default()),
            Err(DgnError::UnexpectedEof {
                offset: 1536,
                needed: 4,
                remaining: 1,
                context: "record header"
            })
        ));
    }

    #[test]
    fn iterator_is_fused_after_an_error() {
        let input = [0x08, 0x09, 0xfe, 0x02];
        let mut iterator = V7RecordIter::new(&input, ScanOptions::default()).unwrap();
        assert!(iterator.next().unwrap().is_err());
        assert!(iterator.next().is_none());
        assert!(iterator.next().is_none());
        assert_eq!(iterator.termination(), None);
    }

    #[test]
    fn enforces_each_resource_limit() {
        let input = one_record_with_tail(&[]);
        assert!(matches!(
            scan_records(
                &input,
                ScanOptions {
                    max_file_size: input.len() - 1,
                    ..ScanOptions::default()
                }
            ),
            Err(DgnError::FileSizeLimitExceeded { .. })
        ));
        assert!(matches!(
            scan_records(
                &input,
                ScanOptions {
                    max_records: 0,
                    ..ScanOptions::default()
                }
            ),
            Err(DgnError::RecordLimitExceeded { .. })
        ));
        assert!(matches!(
            scan_records(
                &input,
                ScanOptions {
                    max_record_size: 1535,
                    ..ScanOptions::default()
                }
            ),
            Err(DgnError::RecordSizeLimitExceeded { .. })
        ));
        assert_eq!(MAX_V7_RECORD_SIZE_BYTES, 131_074);
    }
}
