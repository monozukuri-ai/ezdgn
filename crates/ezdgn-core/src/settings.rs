use crate::numbers::{decode_middle_endian_u32, decode_vax_d_f64};
use crate::{DgnError, DgnFormat, RecordScan, V7Dimension};

const TCB_ELEMENT_TYPE: u8 = 9;
const TCB_REQUIRED_SIZE: usize = 1264;
const SUBUNITS_PER_MASTER_OFFSET: usize = 1112;
const UOR_PER_SUBUNIT_OFFSET: usize = 1116;
const MASTER_UNIT_LABEL_OFFSET: usize = 1120;
const SUB_UNIT_LABEL_OFFSET: usize = 1122;
const DIMENSION_FLAGS_OFFSET: usize = 1214;
const GLOBAL_ORIGIN_X_OFFSET: usize = 1240;
const GLOBAL_ORIGIN_Y_OFFSET: usize = 1248;
const GLOBAL_ORIGIN_Z_OFFSET: usize = 1256;

/// Design-wide coordinate and unit settings decoded from the leading TCB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesignSettings {
    pub dimension: V7Dimension,
    pub subunits_per_master: u32,
    pub uor_per_subunit: u32,
    pub master_unit_label: [u8; 2],
    pub sub_unit_label: [u8; 2],
    /// Global origin in raw units of resolution, before unit scaling.
    pub global_origin_uor: [f64; 3],
}

impl DesignSettings {
    /// Number of units of resolution in one master unit.
    #[must_use]
    pub const fn uor_per_master(self) -> u64 {
        self.subunits_per_master as u64 * self.uor_per_subunit as u64
    }

    /// Scale from UOR coordinates to master units, or `None` for a zero unit
    /// denominator in a malformed/nonstandard TCB.
    #[must_use]
    pub fn scale(self) -> Option<f64> {
        let denominator = self.uor_per_master();
        (denominator != 0).then(|| 1.0 / denominator as f64)
    }

    /// Global origin expressed in master units.
    #[must_use]
    pub fn global_origin_master(self) -> Option<[f64; 3]> {
        let scale = self.scale()?;
        Some(self.global_origin_uor.map(|coordinate| coordinate * scale))
    }

    /// Converts raw element coordinates into master coordinates.
    #[must_use]
    pub fn transform_point(self, point: RawPoint) -> Option<MasterPoint> {
        let scale = self.scale()?;
        let origin = self.global_origin_master()?;
        Some(MasterPoint {
            x: f64::from(point.x) * scale - origin[0],
            y: f64::from(point.y) * scale - origin[1],
            z: point
                .z
                .map(|coordinate| f64::from(coordinate) * scale - origin[2]),
        })
    }

    /// Converts a possibly fractional 2D UOR point into master units.
    #[must_use]
    pub fn transform_xy(self, point: [f64; 2]) -> Option<[f64; 2]> {
        let scale = self.scale()?;
        let origin = self.global_origin_master()?;
        Some([point[0] * scale - origin[0], point[1] * scale - origin[1]])
    }

    /// Scales a distance in UOR without applying the global-origin offset.
    #[must_use]
    pub fn transform_distance(self, distance: f64) -> Option<f64> {
        Some(distance * self.scale()?)
    }
}

/// A two- or three-dimensional signed UOR point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawPoint {
    pub x: i32,
    pub y: i32,
    pub z: Option<i32>,
}

/// A two- or three-dimensional point in master units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasterPoint {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
}

/// Decodes the first TCB and verifies it against the V7 signature.
pub fn decode_design_settings(scan: &RecordScan<'_>) -> Result<DesignSettings, DgnError> {
    let record = scan
        .records
        .first()
        .ok_or(DgnError::MissingDesignSettings)?;
    if record.header.element_type != TCB_ELEMENT_TYPE {
        return Err(DgnError::UnexpectedElementType {
            offset: record.offset,
            expected: TCB_ELEMENT_TYPE,
            actual: record.header.element_type,
            context: "design settings TCB",
        });
    }
    if record.bytes.len() < TCB_REQUIRED_SIZE {
        return Err(DgnError::ElementTooShort {
            offset: record.offset,
            element_type: record.header.element_type,
            needed: TCB_REQUIRED_SIZE,
            actual: record.bytes.len(),
            context: "design settings TCB",
        });
    }

    let tcb_dimension = if record.bytes[DIMENSION_FLAGS_OFFSET] & 0x40 != 0 {
        V7Dimension::Three
    } else {
        V7Dimension::Two
    };
    let DgnFormat::V7(signature_dimension) = scan.format else {
        return Err(DgnError::UnsupportedFormat {
            format: scan.format,
        });
    };
    if signature_dimension != tcb_dimension {
        return Err(DgnError::DimensionMismatch {
            signature: signature_dimension,
            tcb: tcb_dimension,
        });
    }

    Ok(DesignSettings {
        dimension: tcb_dimension,
        subunits_per_master: decode_middle_endian_u32(read_four(
            record.bytes,
            SUBUNITS_PER_MASTER_OFFSET,
        )),
        uor_per_subunit: decode_middle_endian_u32(read_four(record.bytes, UOR_PER_SUBUNIT_OFFSET)),
        master_unit_label: [
            record.bytes[MASTER_UNIT_LABEL_OFFSET],
            record.bytes[MASTER_UNIT_LABEL_OFFSET + 1],
        ],
        sub_unit_label: [
            record.bytes[SUB_UNIT_LABEL_OFFSET],
            record.bytes[SUB_UNIT_LABEL_OFFSET + 1],
        ],
        global_origin_uor: [
            decode_vax_d_f64(read_eight(record.bytes, GLOBAL_ORIGIN_X_OFFSET)),
            decode_vax_d_f64(read_eight(record.bytes, GLOBAL_ORIGIN_Y_OFFSET)),
            decode_vax_d_f64(read_eight(record.bytes, GLOBAL_ORIGIN_Z_OFFSET)),
        ],
    })
}

fn read_four(bytes: &[u8], offset: usize) -> [u8; 4] {
    [
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]
}

fn read_eight(bytes: &[u8], offset: usize) -> [u8; 8] {
    [
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ]
}
