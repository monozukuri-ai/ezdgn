//! Pure Rust core for the `ezdgn` package.
//!
//! The initial reader targets the sequential record stream used by V7/ISFF
//! DGN files. V8 compound files are detected explicitly but are not parsed by
//! this crate.

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
    inspect_v8_container, V8CfbEntry, V8CfbEntryKind, V8ContainerInfo, DEFAULT_MAX_CFB_ENTRIES,
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
