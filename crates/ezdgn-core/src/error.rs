use thiserror::Error;

use crate::{DgnFormat, V7Dimension};

/// Errors produced while identifying or scanning a DGN file.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DgnError {
    /// The input does not contain enough bytes to identify its format.
    #[error(
        "input is too short to identify DGN format: need at least {needed} bytes, got {actual}"
    )]
    InputTooShort { needed: usize, actual: usize },

    /// The input does not start with a recognized V7 or V8/CFB signature.
    #[error("unrecognized DGN signature: {signature}")]
    UnrecognizedFormat { signature: String },

    /// The format was recognized but the requested reader does not support it.
    #[error(
        "{format} is not supported by the native V7 reader; V8 semantic access requires an explicitly licensed external converter"
    )]
    UnsupportedFormat { format: DgnFormat },

    /// V8 container inspection was requested for another recognized family.
    #[error("expected a V8/CFB candidate, got {format}")]
    ExpectedV8Container { format: DgnFormat },

    /// The CFB signature was present, but the container structure is invalid.
    #[error("invalid V8/CFB container: {context}")]
    InvalidV8Container { context: String },

    /// The CFB directory contains more entries than the configured limit.
    #[error("CFB entry count exceeds configured limit {limit}")]
    CfbEntryLimitExceeded { limit: usize },

    /// The format is V7, but this semantic reader currently handles only 2D.
    #[error("V7 {dimension} geometry is not supported by the 2D entity reader")]
    UnsupportedDimension { dimension: V7Dimension },

    /// The complete input exceeds the configured safety limit.
    #[error("input size {actual} bytes exceeds configured limit {limit} bytes")]
    FileSizeLimitExceeded { actual: usize, limit: usize },

    /// A further record exists after the configured record-count limit.
    #[error("record count exceeds configured limit {limit} at byte offset {offset}")]
    RecordLimitExceeded { offset: usize, limit: usize },

    /// A record's declared size exceeds the configured per-record limit.
    #[error(
        "record type {element_type} at byte offset {offset} declares {declared} bytes, exceeding configured limit {limit} bytes"
    )]
    RecordSizeLimitExceeded {
        offset: usize,
        element_type: u8,
        declared: usize,
        limit: usize,
    },

    /// A complete header declared a record extending beyond physical EOF.
    #[error(
        "record type {element_type} at byte offset {offset} declares {declared} bytes, but only {remaining} remain"
    )]
    TruncatedRecord {
        offset: usize,
        element_type: u8,
        declared: usize,
        remaining: usize,
    },

    /// A bounded read would pass the physical end of the input.
    #[error(
        "unexpected end of input while reading {context} at byte offset {offset}: need {needed} bytes, {remaining} remain"
    )]
    UnexpectedEof {
        offset: usize,
        needed: usize,
        remaining: usize,
        context: &'static str,
    },

    /// The first record required for design-wide settings is absent.
    #[error("V7 record stream does not contain a leading TCB element")]
    MissingDesignSettings,

    /// A semantic decoder was given a different element type.
    #[error(
        "expected element type {expected} for {context} at byte offset {offset}, got type {actual}"
    )]
    UnexpectedElementType {
        offset: usize,
        expected: u8,
        actual: u8,
        context: &'static str,
    },

    /// A structurally complete record is too short for a semantic field.
    #[error(
        "element type {element_type} at byte offset {offset} is too short for {context}: need {needed} bytes, got {actual}"
    )]
    ElementTooShort {
        offset: usize,
        element_type: u8,
        needed: usize,
        actual: usize,
        context: &'static str,
    },

    /// The leading signature and TCB disagree about dimensionality.
    #[error(
        "V7 dimension mismatch: file signature says {signature}, but the leading TCB says {tcb}"
    )]
    DimensionMismatch {
        signature: V7Dimension,
        tcb: V7Dimension,
    },

    /// The display-header attribute pointer does not identify payload bytes.
    #[error(
        "element type {element_type} at byte offset {offset} has invalid attribute offset {attribute_offset} for a {record_size}-byte record"
    )]
    InvalidAttributeOffset {
        offset: usize,
        element_type: u8,
        attribute_offset: usize,
        record_size: usize,
    },

    /// A variable-size primitive declared an invalid number of vertices.
    #[error(
        "element type {element_type} at byte offset {offset} declares {count} vertices; at least {minimum} are required"
    )]
    InvalidVertexCount {
        offset: usize,
        element_type: u8,
        count: usize,
        minimum: usize,
    },

    /// A complex description length is negative, shorter than its header, or
    /// extends beyond the bounded record stream.
    #[error(
        "element type {element_type} at byte offset {offset} has invalid description end {declared_end} (record end {record_end}, stream end {stream_end})"
    )]
    InvalidDescriptionRange {
        offset: usize,
        element_type: u8,
        declared_end: i64,
        record_end: usize,
        stream_end: usize,
    },

    /// A declared complex description ends between record boundaries.
    #[error(
        "element type {element_type} at byte offset {offset} declares a description ending at byte {declared_end}, which is not a record boundary"
    )]
    InvalidDescriptionBoundary {
        offset: usize,
        element_type: u8,
        declared_end: usize,
    },

    /// A complex header's stored number of direct components is inconsistent.
    #[error(
        "element type {element_type} at byte offset {offset} declares {declared} direct components, but {actual} records were found"
    )]
    ComplexElementCountMismatch {
        offset: usize,
        element_type: u8,
        declared: usize,
        actual: usize,
    },

    /// A record inside a declared complex description lacks the component bit.
    #[error(
        "record type {component_type} at byte offset {component_offset} is inside type {parent_type} at byte offset {parent_offset}, but its complex-component bit is clear"
    )]
    MissingComplexComponentFlag {
        parent_offset: usize,
        parent_type: u8,
        component_offset: usize,
        component_type: u8,
    },

    /// A complex-component bit was set without any enclosing description.
    #[error(
        "record type {element_type} at byte offset {offset} has the complex-component bit set without an enclosing complex header"
    )]
    OrphanComplexComponent { offset: usize, element_type: u8 },

    /// B-spline component records do not match the header flags/counts/order.
    #[error(
        "B-spline type {element_type} at byte offset {offset} has invalid components: {context}"
    )]
    InvalidBSplineComponents {
        offset: usize,
        element_type: u8,
        context: &'static str,
    },

    /// A variable-length scalar array does not occupy complete 32-bit values.
    #[error(
        "element type {element_type} at byte offset {offset} has {data_bytes} bytes of {context}; expected a non-empty multiple of 4"
    )]
    InvalidScalarArrayLength {
        offset: usize,
        element_type: u8,
        data_bytes: usize,
        context: &'static str,
    },

    /// A seed file does not contain the control records required to create a
    /// standalone V7 design file.
    #[error("invalid V7 writer seed: {context}")]
    InvalidWriterSeed { context: &'static str },

    /// A writer input cannot be represented by the selected V7 element.
    #[error("cannot write {entity}: {context}")]
    InvalidWriterEntity {
        entity: &'static str,
        context: &'static str,
    },

    /// A master-unit coordinate cannot be represented in the seed design
    /// plane without clipping.
    #[error("cannot write {entity}: {axis} coordinate is outside the V7 design plane")]
    WriterCoordinateOutOfRange {
        entity: &'static str,
        axis: &'static str,
    },

    /// A generated record is too large for the V7 words-to-follow field.
    #[error("cannot write {entity}: generated record is {size} bytes, exceeding {limit} bytes")]
    WriterRecordTooLarge {
        entity: &'static str,
        size: usize,
        limit: usize,
    },
}

impl DgnError {
    /// Returns true when the error represents a configured resource limit.
    #[must_use]
    pub const fn is_limit_error(&self) -> bool {
        matches!(
            self,
            Self::FileSizeLimitExceeded { .. }
                | Self::RecordLimitExceeded { .. }
                | Self::RecordSizeLimitExceeded { .. }
                | Self::CfbEntryLimitExceeded { .. }
        )
    }

    /// Returns true when a recognized family or dimension is outside the
    /// requested reader's scope.
    #[must_use]
    pub const fn is_unsupported_error(&self) -> bool {
        matches!(
            self,
            Self::UnsupportedFormat { .. }
                | Self::ExpectedV8Container { .. }
                | Self::UnsupportedDimension { .. }
        )
    }
}
