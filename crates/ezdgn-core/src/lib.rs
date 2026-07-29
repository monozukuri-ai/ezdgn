//! Pure Rust core for the `ezdgn` package.
//!
//! The semantic reader targets the sequential record stream used by V7/ISFF
//! DGN files. V8 compound files use an independently implemented bounded
//! container, object-page, and semantic reader.

#![forbid(unsafe_code)]

mod common;
mod entities;
mod error;
mod format;
mod io;
mod linkage;
mod numbers;
mod options;
mod record;
mod settings;
mod v8;
mod writer;

pub use common::{
    decode_common_header, element_type_has_common_header, CommonElementHeader, ElementProperties,
    ElementRange, ElementSymbology, MasterElementRange,
};
pub use entities::{
    read_v7_2d, Arc2D, BSplineCurve2D, BSplineKnot2D, BSplinePole2D, BSplineSurface2D,
    BSplineSurfaceBoundary2D, BSplineWeight2D, CellHeader2D, ColorTable, ComplexHeader2D, Curve2D,
    Element2D, ElementData2D, Ellipse2D, Line2D, LineString2D, Point2, Shape2D, Text2D, TextNode2D,
    V7Document2D,
};
pub use error::DgnError;
pub use format::{detect_format, DgnFormat, V7Dimension};
pub use linkage::{decode_attribute_linkages, AttributeLinkage, LinkageData, PrecisionDelta};
pub use numbers::{
    decode_middle_endian_i32, decode_middle_endian_u32, decode_offset_binary_i32, decode_vax_d_f64,
    encode_vax_d_f64,
};
pub use options::{
    ScanOptions, DEFAULT_MAX_FILE_SIZE_BYTES, DEFAULT_MAX_RECORDS, MAX_V7_RECORD_SIZE_BYTES,
};
pub use record::{
    scan_records, RawElementHeader, RawElementRef, RecordScan, RecordStreamEnd, V7RecordIter,
};
pub use settings::{decode_design_settings, DesignSettings, MasterPoint, RawPoint};
pub use v8::{
    decode_v8_linkages, inspect_v8_container, read_v8, read_v8_stream, read_v8_streams,
    scan_v8_objects, V8AuxiliaryPage, V8AuxiliaryRecord, V8CfbEntry, V8CfbEntryKind,
    V8CommonHeader, V8ContainerInfo, V8Dimension, V8Document, V8Element, V8ElementData, V8Linkage,
    V8Model, V8ModelIndexEntry, V8ModelMetadata, V8ObjectFamily, V8ObjectPage, V8ObjectRole,
    V8PageHeader, V8Point, V8Point3, V8PointOrientation, V8Range3, V8Range3I64, V8RawDocument,
    V8RawModel, V8RawObject, V8ReadOptions, V8ScanOptions, V8Stream, V8StreamSet, V8TextEncoding,
    DEFAULT_MAX_CFB_ENTRIES, DEFAULT_MAX_CFB_STREAM_SIZE_BYTES, DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES,
    DEFAULT_MAX_V8_HIERARCHY_DEPTH, DEFAULT_MAX_V8_INFLATED_STREAM_BYTES, DEFAULT_MAX_V8_MODELS,
    DEFAULT_MAX_V8_OBJECTS, DEFAULT_MAX_V8_OBJECT_SIZE_BYTES, DEFAULT_MAX_V8_PAGES,
    DEFAULT_MAX_V8_STRING_BYTES, DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES, DEFAULT_MAX_V8_VERTICES,
    V8_PROPERTY_LINKAGE_KIND,
};
pub use writer::{write_v7_2d, V7ElementStyle, V7WriteOptions, WritableElement2D};

/// Version of the Rust core bundled with the Python package.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns the version of the Rust core.
#[must_use]
pub const fn version() -> &'static str {
    VERSION
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_package_version() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
    }
}
