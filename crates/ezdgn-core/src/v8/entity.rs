use std::collections::HashMap;
use std::sync::Arc;

use crate::DgnError;

use super::linkage::{decode_v8_linkages, decode_windows_1252, V8Linkage};
use super::metadata::{
    decode_model_metadata, read_f64, read_i64, V8Dimension, V8ModelMetadata, V8Point3, V8Range3,
    V8Range3I64,
};
use super::raw::{read_u16, read_u32, read_u64};
use super::{scan_v8_objects, V8AuxiliaryRecord, V8RawDocument, V8RawObject, V8ScanOptions};

const COMMON_HEADER_BYTES: usize = 0x68;
const ELEMENT_3D_FLAG: u32 = 0x0000_0800;
const TEXT_MULTIPLIER_TO_UOR: f64 = 6.0 / 1000.0;

/// Display and identity fields shared by standard V8 graphical objects.
#[derive(Debug, Clone, PartialEq)]
pub struct V8CommonHeader {
    pub level: u32,
    pub element_id: u64,
    pub model_id: u64,
    pub graphic_group: u32,
    pub properties: u32,
    pub geometry_flags: u32,
    pub line_style: u32,
    pub line_weight: u32,
    pub color_index: u32,
    /// Dimension bit stored on this object's own common header.
    pub stored_dimension: V8Dimension,
    /// Effective entity dimension; complex containers inherit their children.
    pub dimension: V8Dimension,
    pub range_uor: V8Range3I64,
    pub range_master: V8Range3,
    pub attribute_offset: usize,
    pub attribute_length: usize,
}

/// One point retained in both stored UOR and transformed master units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V8Point {
    pub uor: V8Point3,
    pub master: V8Point3,
}

/// Orientation attached to point-string vertices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V8PointOrientation {
    pub values: [f64; 4],
}

/// Native text encoding form observed in a V8 text record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8TextEncoding {
    Utf8,
    Windows1252,
    EscapedWindows1252,
}

impl V8TextEncoding {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Windows1252 => "windows-1252",
            Self::EscapedWindows1252 => "v8-escaped-windows-1252",
        }
    }
}

/// Decoded standard V8 element data. Unknown objects remain represented by
/// their exact `V8RawObject` on `V8Element`.
#[derive(Debug, Clone, PartialEq)]
pub enum V8ElementData {
    Line {
        start: V8Point,
        end: V8Point,
    },
    LineString {
        vertices: Vec<V8Point>,
    },
    Shape {
        vertices: Vec<V8Point>,
    },
    Curve {
        vertices: Vec<V8Point>,
    },
    PointString {
        vertices: Vec<V8Point>,
        orientations: Vec<V8PointOrientation>,
    },
    Text {
        font_id: u32,
        justification: u16,
        width_multiplier_raw: f64,
        height_multiplier_raw: f64,
        width_uor: f64,
        height_uor: f64,
        width_master: f64,
        height_master: f64,
        rotation_radians: Option<f64>,
        orientation: Vec<f64>,
        origin: V8Point,
        editable_fields: u16,
        encoding: V8TextEncoding,
        text_bytes: Arc<[u8]>,
        text: String,
    },
    Ellipse {
        primary_axis_uor: f64,
        secondary_axis_uor: f64,
        primary_axis_master: f64,
        secondary_axis_master: f64,
        rotation_radians: Option<f64>,
        orientation: Vec<f64>,
        center: V8Point,
    },
    Arc {
        start_angle_radians: f64,
        sweep_angle_radians: f64,
        primary_axis_uor: f64,
        secondary_axis_uor: f64,
        primary_axis_master: f64,
        secondary_axis_master: f64,
        rotation_radians: Option<f64>,
        orientation: Vec<f64>,
        center: V8Point,
    },
    TextNode {
        child_count: usize,
        node_number: u32,
        origin: V8Point,
    },
    ComplexChain {
        child_count: usize,
    },
    ComplexShape {
        child_count: usize,
    },
    Cell {
        child_count: usize,
        boundary_count: u32,
        origin: V8Point,
        transform: Vec<f64>,
    },
    SharedCellInstance {
        name: Option<String>,
        origin: V8Point,
        transform: Vec<f64>,
    },
    BSplineCurve {
        child_count: usize,
        properties_raw: u32,
        declared_poles: usize,
    },
    BSplinePole {
        vertices: Vec<V8Point>,
    },
    Dimension {
        anchor: Option<V8Point>,
    },
    UnknownComplex {
        child_count: usize,
    },
    Unknown,
}

impl V8ElementData {
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Line { .. } => "LINE",
            Self::LineString { .. } => "LINE_STRING",
            Self::Shape { .. } => "SHAPE",
            Self::Curve { .. } => "CURVE",
            Self::PointString { .. } => "POINT_STRING",
            Self::Text { .. } => "TEXT",
            Self::Ellipse { .. } => "ELLIPSE",
            Self::Arc { .. } => "ARC",
            Self::TextNode { .. } => "TEXT_NODE",
            Self::ComplexChain { .. } => "COMPLEX_CHAIN",
            Self::ComplexShape { .. } => "COMPLEX_SHAPE",
            Self::Cell { .. } => "CELL",
            Self::SharedCellInstance { .. } => "SHARED_CELL_INSTANCE",
            Self::BSplineCurve { .. } => "BSPLINE_CURVE",
            Self::BSplinePole { .. } => "BSPLINE_POLE",
            Self::Dimension { .. } => "DIMENSION",
            Self::UnknownComplex { .. } => "UNKNOWN_COMPLEX",
            Self::Unknown => "UNKNOWN",
        }
    }

    #[must_use]
    pub const fn declared_child_count(&self) -> Option<usize> {
        match self {
            Self::TextNode { child_count, .. }
            | Self::ComplexChain { child_count }
            | Self::ComplexShape { child_count }
            | Self::Cell { child_count, .. }
            | Self::BSplineCurve { child_count, .. }
            | Self::UnknownComplex { child_count } => Some(*child_count),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_drawable(&self) -> bool {
        !matches!(
            self,
            Self::Unknown
                | Self::UnknownComplex { .. }
                | Self::BSplinePole { .. }
                | Self::TextNode { .. }
        )
    }
}

/// One semantic graphical element paired with all retained raw representations.
#[derive(Debug, Clone)]
pub struct V8Element {
    pub index: usize,
    pub raw: V8RawObject,
    pub common: V8CommonHeader,
    pub data: V8ElementData,
    pub parent_index: Option<usize>,
    pub child_indices: Vec<usize>,
    pub linkages: Vec<V8Linkage>,
    pub auxiliary_records: Vec<V8AuxiliaryRecord>,
}

impl V8Element {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.data.kind_name()
    }

    #[must_use]
    pub fn is_top_level(&self) -> bool {
        self.parent_index.is_none()
    }
}

/// One decoded V8 model.
#[derive(Debug, Clone)]
pub struct V8Model {
    pub metadata: V8ModelMetadata,
    pub elements: Vec<V8Element>,
}

impl V8Model {
    pub fn entities(&self) -> impl Iterator<Item = &V8Element> {
        self.elements.iter().filter(|element| {
            if !element.data.is_drawable() {
                return false;
            }
            if element.is_top_level() {
                return true;
            }
            matches!(element.data, V8ElementData::Text { .. })
                && element.parent_index.is_some_and(|index| {
                    matches!(
                        self.elements.get(index).map(|parent| &parent.data),
                        Some(V8ElementData::TextNode { .. })
                    )
                })
        })
    }

    pub fn all_drawable_elements(&self) -> impl Iterator<Item = &V8Element> {
        self.elements
            .iter()
            .filter(|element| element.data.is_drawable())
    }

    pub fn children<'a>(
        &'a self,
        element: &'a V8Element,
    ) -> impl Iterator<Item = &'a V8Element> + 'a {
        element
            .child_indices
            .iter()
            .filter_map(|index| self.elements.get(*index))
    }
}

/// Complete native V8 document with semantic models and a lossless raw scan.
#[derive(Debug, Clone)]
pub struct V8Document {
    pub raw: V8RawDocument,
    pub models: Vec<V8Model>,
}

/// Scan and semantically decode supported V8 models and graphical elements.
pub fn read_v8(input: &[u8], options: V8ScanOptions) -> Result<V8Document, DgnError> {
    let raw = scan_v8_objects(input, options)?;
    let mut models = Vec::with_capacity(raw.models.len());
    for raw_model in &raw.models {
        let metadata = decode_model_metadata(raw_model, options)?;
        let elements = decode_model_elements(raw_model, &metadata, options)?;
        models.push(V8Model { metadata, elements });
    }
    Ok(V8Document { raw, models })
}

fn decode_model_elements(
    model: &super::V8RawModel,
    metadata: &V8ModelMetadata,
    options: V8ScanOptions,
) -> Result<Vec<V8Element>, DgnError> {
    let mut auxiliary = HashMap::<u64, Vec<V8AuxiliaryRecord>>::new();
    for record in model
        .graphical_auxiliary_pages
        .iter()
        .flat_map(|page| page.records.iter())
    {
        auxiliary
            .entry(record.element_id)
            .or_default()
            .push(record.clone());
    }

    let mut elements = Vec::with_capacity(model.graphical_objects().count());
    for raw in model.graphical_objects() {
        let common = decode_common(raw, metadata)?;
        let linkages = decode_v8_linkages(
            raw.attribute_bytes(),
            options,
            &format!("element {}", common.element_id),
        )?;
        let data = decode_element(raw, &common, metadata, &linkages, options)?;
        elements.push(V8Element {
            index: elements.len(),
            raw: raw.clone(),
            common,
            data,
            parent_index: None,
            child_indices: Vec::new(),
            linkages,
            auxiliary_records: auxiliary
                .remove(&raw.element_id.unwrap_or_default())
                .unwrap_or_default(),
        });
    }
    build_hierarchy(&mut elements, options.max_hierarchy_depth)?;
    resolve_container_dimensions(&mut elements);
    Ok(elements)
}

fn decode_common(
    raw: &V8RawObject,
    metadata: &V8ModelMetadata,
) -> Result<V8CommonHeader, DgnError> {
    let bytes = raw.as_bytes();
    if bytes.len() < COMMON_HEADER_BYTES {
        return Err(geometry_error(
            raw,
            format!(
                "common graphical header needs {COMMON_HEADER_BYTES} bytes, got {}",
                bytes.len()
            ),
        ));
    }
    let range_uor = V8Range3I64 {
        low: [
            read_i64(bytes, 0x38).unwrap_or_default(),
            read_i64(bytes, 0x40).unwrap_or_default(),
            read_i64(bytes, 0x48).unwrap_or_default(),
        ],
        high: [
            read_i64(bytes, 0x50).unwrap_or_default(),
            read_i64(bytes, 0x58).unwrap_or_default(),
            read_i64(bytes, 0x60).unwrap_or_default(),
        ],
    };
    let to_master = |point: [i64; 3]| {
        metadata.to_master(V8Point3::new(
            point[0] as f64,
            point[1] as f64,
            point[2] as f64,
        ))
    };
    let attribute_offset = usize::try_from(raw.attribute_words)
        .unwrap_or(usize::MAX)
        .saturating_mul(2);
    let geometry_flags = read_u32(bytes, 0x28).unwrap_or_default();
    let stored_dimension = if geometry_flags & ELEMENT_3D_FLAG != 0 {
        V8Dimension::Three
    } else {
        V8Dimension::Two
    };
    Ok(V8CommonHeader {
        level: read_u32(bytes, 0x0c).unwrap_or_default(),
        element_id: read_u64(bytes, 0x10).unwrap_or_default(),
        model_id: read_u64(bytes, 0x18).unwrap_or_default(),
        graphic_group: read_u32(bytes, 0x20).unwrap_or_default(),
        properties: read_u32(bytes, 0x24).unwrap_or_default(),
        geometry_flags,
        line_style: read_u32(bytes, 0x2c).unwrap_or_default(),
        line_weight: read_u32(bytes, 0x30).unwrap_or_default(),
        color_index: read_u32(bytes, 0x34).unwrap_or_default(),
        stored_dimension,
        dimension: stored_dimension,
        range_uor,
        range_master: V8Range3 {
            low: to_master(range_uor.low),
            high: to_master(range_uor.high),
        },
        attribute_offset,
        attribute_length: bytes.len().saturating_sub(attribute_offset),
    })
}

fn decode_element(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
    linkages: &[V8Linkage],
    options: V8ScanOptions,
) -> Result<V8ElementData, DgnError> {
    match raw.element_type {
        2 => decode_cell(raw, common, metadata),
        3 => decode_line(raw, common, metadata),
        4 => decode_vertices(raw, common, metadata, options, 2)
            .map(|vertices| V8ElementData::LineString { vertices }),
        6 => decode_vertices(raw, common, metadata, options, 3)
            .map(|vertices| V8ElementData::Shape { vertices }),
        7 => decode_text_node(raw, common, metadata),
        11 => decode_vertices(raw, common, metadata, options, 2)
            .map(|vertices| V8ElementData::Curve { vertices }),
        12 => decode_complex(raw, V8ElementData::ComplexChain { child_count: 0 }),
        14 => decode_complex(raw, V8ElementData::ComplexShape { child_count: 0 }),
        15 => decode_ellipse(raw, common, metadata),
        16 => decode_arc(raw, common, metadata),
        17 => decode_text(raw, common, metadata, options),
        21 => decode_vertices(raw, common, metadata, options, 1)
            .map(|vertices| V8ElementData::BSplinePole { vertices }),
        22 => decode_point_string(raw, common, metadata, options),
        27 => decode_bspline_header(raw),
        35 => decode_shared_cell(raw, common, metadata, linkages),
        36 => decode_dimension(raw, common, metadata),
        _ if raw.role.is_header() => Ok(V8ElementData::UnknownComplex {
            child_count: count_at(raw, raw.primary_bytes(), 0x68)?,
        }),
        _ => Ok(V8ElementData::Unknown),
    }
}

fn decode_line(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let width = point_bytes(common.dimension);
    let required = COMMON_HEADER_BYTES + 2 * width;
    require_primary_size(raw, primary, required)?;
    let start = decode_point(
        primary,
        COMMON_HEADER_BYTES,
        common.dimension,
        metadata,
        raw,
    )?;
    let end = decode_point(
        primary,
        COMMON_HEADER_BYTES + width,
        common.dimension,
        metadata,
        raw,
    )?;
    Ok(V8ElementData::Line { start, end })
}

fn decode_vertices(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
    options: V8ScanOptions,
    minimum: usize,
) -> Result<Vec<V8Point>, DgnError> {
    let primary = raw.primary_bytes();
    require_minimum(raw, primary, COMMON_HEADER_BYTES + 8, "vertex header")?;
    let count = usize::try_from(read_u32(primary, COMMON_HEADER_BYTES).unwrap_or_default())
        .unwrap_or(usize::MAX);
    validate_vertex_count(raw, count, minimum, options.max_vertices)?;
    let width = point_bytes(common.dimension);
    let data_bytes = count
        .checked_mul(width)
        .ok_or_else(|| geometry_error(raw, "vertex byte length overflows"))?;
    let required = COMMON_HEADER_BYTES + 8 + data_bytes;
    require_primary_size(raw, primary, required)?;
    (0..count)
        .map(|index| {
            decode_point(
                primary,
                COMMON_HEADER_BYTES + 8 + index * width,
                common.dimension,
                metadata,
                raw,
            )
        })
        .collect()
}

fn decode_point_string(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
    options: V8ScanOptions,
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    require_minimum(raw, primary, COMMON_HEADER_BYTES + 8, "point-string header")?;
    let count = usize::try_from(read_u32(primary, COMMON_HEADER_BYTES).unwrap_or_default())
        .unwrap_or(usize::MAX);
    validate_vertex_count(raw, count, 1, options.max_vertices)?;
    let width = point_bytes(common.dimension);
    let points_bytes = count
        .checked_mul(width)
        .ok_or_else(|| geometry_error(raw, "point-string byte length overflows"))?;
    let orientation_bytes = count
        .checked_mul(32)
        .ok_or_else(|| geometry_error(raw, "point orientation byte length overflows"))?;
    let point_start = COMMON_HEADER_BYTES + 8;
    let orientation_start = point_start + points_bytes;
    let required = orientation_start + orientation_bytes;
    require_primary_size(raw, primary, required)?;
    let vertices = (0..count)
        .map(|index| {
            decode_point(
                primary,
                point_start + index * width,
                common.dimension,
                metadata,
                raw,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let orientations = (0..count)
        .map(|index| {
            let offset = orientation_start + index * 32;
            read_f64_array::<4>(primary, offset, raw).map(|values| V8PointOrientation { values })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(V8ElementData::PointString {
        vertices,
        orientations,
    })
}

fn decode_text(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
    options: V8ScanOptions,
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let text_length = usize::from(
        read_u16(primary, 0x6e)
            .ok_or_else(|| geometry_error(raw, "text byte-length field is truncated"))?,
    );
    if text_length > options.max_string_bytes {
        return Err(DgnError::V8StringLimitExceeded {
            context: format!("text element {}", common.element_id),
            actual: text_length,
            limit: options.max_string_bytes,
        });
    }
    let padded_text_length = text_length
        .checked_add(text_length % 2)
        .ok_or_else(|| geometry_error(raw, "padded text length overflows"))?;
    let text_start = primary
        .len()
        .checked_sub(padded_text_length)
        .ok_or_else(|| geometry_error(raw, "text length exceeds primary object bytes"))?;
    let editable_offset = text_start
        .checked_sub(2)
        .ok_or_else(|| geometry_error(raw, "text editable-field prefix is missing"))?;
    let expected_editable = match common.dimension {
        V8Dimension::Two => 0xa8,
        V8Dimension::Three => 0xc8,
    };
    if editable_offset != expected_editable {
        return Err(geometry_error(
            raw,
            format!(
                "text payload begins at 0x{text_start:x}; expected editable-field offset 0x{expected_editable:x}"
            ),
        ));
    }
    let raw_text = primary
        .get(text_start..text_start + text_length)
        .ok_or_else(|| geometry_error(raw, "text payload is truncated"))?;
    let (encoding, decoded_bytes) = if let Some(bytes) = raw_text.strip_prefix(&[0xff, 0xfe, 1, 0])
    {
        (V8TextEncoding::EscapedWindows1252, bytes)
    } else if std::str::from_utf8(raw_text).is_ok() {
        (V8TextEncoding::Utf8, raw_text)
    } else {
        (V8TextEncoding::Windows1252, raw_text)
    };
    let text = if encoding == V8TextEncoding::Utf8 {
        std::str::from_utf8(decoded_bytes)
            .unwrap_or_default()
            .to_owned()
    } else {
        decode_windows_1252(decoded_bytes)
    };
    let width_multiplier_raw = finite_f64(primary, 0x70, raw, "text width multiplier")?;
    let height_multiplier_raw = finite_f64(primary, 0x78, raw, "text height multiplier")?;
    let width_uor = width_multiplier_raw * TEXT_MULTIPLIER_TO_UOR;
    let height_uor = height_multiplier_raw * TEXT_MULTIPLIER_TO_UOR;
    let (rotation_radians, orientation, origin_offset) = match common.dimension {
        V8Dimension::Two => (
            Some(finite_f64(primary, 0x90, raw, "text rotation")?),
            Vec::new(),
            0x98,
        ),
        V8Dimension::Three => (
            None,
            read_f64_array::<6>(primary, 0x80, raw)?.to_vec(),
            0xb0,
        ),
    };
    let origin = decode_point(primary, origin_offset, common.dimension, metadata, raw)?;
    Ok(V8ElementData::Text {
        font_id: read_u32(primary, 0x68).unwrap_or_default(),
        justification: read_u16(primary, 0x6c).unwrap_or_default(),
        width_multiplier_raw,
        height_multiplier_raw,
        width_uor,
        height_uor,
        width_master: width_uor * metadata.scale,
        height_master: height_uor * metadata.scale,
        rotation_radians,
        orientation,
        origin,
        editable_fields: read_u16(primary, editable_offset).unwrap_or_default(),
        encoding,
        text_bytes: Arc::from(raw_text),
        text,
    })
}

fn decode_ellipse(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let primary_axis_uor = finite_f64(primary, 0x68, raw, "ellipse primary axis")?;
    let secondary_axis_uor = finite_f64(primary, 0x70, raw, "ellipse secondary axis")?;
    let (rotation_radians, orientation, center_offset, required) = match common.dimension {
        V8Dimension::Two => (
            Some(finite_f64(primary, 0x78, raw, "ellipse rotation")?),
            Vec::new(),
            0x80,
            0x90,
        ),
        V8Dimension::Three => (
            None,
            read_f64_array::<4>(primary, 0x78, raw)?.to_vec(),
            0x98,
            0xb0,
        ),
    };
    require_primary_size(raw, primary, required)?;
    let center = decode_point(primary, center_offset, common.dimension, metadata, raw)?;
    Ok(V8ElementData::Ellipse {
        primary_axis_uor,
        secondary_axis_uor,
        primary_axis_master: primary_axis_uor * metadata.scale,
        secondary_axis_master: secondary_axis_uor * metadata.scale,
        rotation_radians,
        orientation,
        center,
    })
}

fn decode_arc(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let start_angle_radians = finite_f64(primary, 0x68, raw, "arc start angle")?;
    let sweep_angle_radians = finite_f64(primary, 0x70, raw, "arc sweep angle")?;
    let primary_axis_uor = finite_f64(primary, 0x78, raw, "arc primary axis")?;
    let secondary_axis_uor = finite_f64(primary, 0x80, raw, "arc secondary axis")?;
    let (rotation_radians, orientation, center_offset, required) = match common.dimension {
        V8Dimension::Two => (
            Some(finite_f64(primary, 0x88, raw, "arc rotation")?),
            Vec::new(),
            0x90,
            0xa0,
        ),
        V8Dimension::Three => (
            None,
            read_f64_array::<4>(primary, 0x88, raw)?.to_vec(),
            0xa8,
            0xc0,
        ),
    };
    require_primary_size(raw, primary, required)?;
    let center = decode_point(primary, center_offset, common.dimension, metadata, raw)?;
    Ok(V8ElementData::Arc {
        start_angle_radians,
        sweep_angle_radians,
        primary_axis_uor,
        secondary_axis_uor,
        primary_axis_master: primary_axis_uor * metadata.scale,
        secondary_axis_master: secondary_axis_uor * metadata.scale,
        rotation_radians,
        orientation,
        center,
    })
}

fn decode_text_node(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let child_count = count_at(raw, primary, 0x68)?;
    let origin_offset = primary
        .len()
        .checked_sub(point_bytes(common.dimension))
        .ok_or_else(|| geometry_error(raw, "text-node origin is missing"))?;
    let origin = decode_point(primary, origin_offset, common.dimension, metadata, raw)?;
    Ok(V8ElementData::TextNode {
        child_count,
        node_number: read_u32(primary, 0x6c).unwrap_or_default(),
        origin,
    })
}

fn decode_complex(raw: &V8RawObject, variant: V8ElementData) -> Result<V8ElementData, DgnError> {
    let child_count = count_at(raw, raw.primary_bytes(), 0x68)?;
    Ok(match variant {
        V8ElementData::ComplexChain { .. } => V8ElementData::ComplexChain { child_count },
        V8ElementData::ComplexShape { .. } => V8ElementData::ComplexShape { child_count },
        _ => unreachable!(),
    })
}

fn decode_cell(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let child_count = count_at(raw, primary, 0x68)?;
    let boundary_count = read_u32(primary, 0x6c)
        .ok_or_else(|| geometry_error(raw, "cell boundary count is truncated"))?;
    let origin = decode_point(primary, 0x70, common.dimension, metadata, raw)?;
    let (transform_offset, transform_count, required) = match common.dimension {
        V8Dimension::Two => (0x90, 6, 0xc0),
        V8Dimension::Three => (0xa0, 12, 0x100),
    };
    require_primary_size(raw, primary, required)?;
    let transform = read_f64_values(primary, transform_offset, transform_count, raw)?;
    Ok(V8ElementData::Cell {
        child_count,
        boundary_count,
        origin,
        transform,
    })
}

fn decode_shared_cell(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
    linkages: &[V8Linkage],
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let origin_offset = primary
        .len()
        .checked_sub(point_bytes(common.dimension))
        .ok_or_else(|| geometry_error(raw, "shared-cell origin is missing"))?;
    let origin = decode_point(primary, origin_offset, common.dimension, metadata, raw)?;
    let transform_start = 0x70;
    let transform_bytes = origin_offset.saturating_sub(transform_start);
    let transform_count = transform_bytes / 8;
    let transform = read_f64_values(primary, transform_start, transform_count, raw)?;
    let name = linkages
        .iter()
        .find(|linkage| linkage.property_id == Some(1))
        .and_then(|linkage| linkage.property_text.clone());
    Ok(V8ElementData::SharedCellInstance {
        name,
        origin,
        transform,
    })
}

fn decode_bspline_header(raw: &V8RawObject) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    Ok(V8ElementData::BSplineCurve {
        child_count: count_at(raw, primary, 0x68)?,
        properties_raw: read_u32(primary, 0x6c)
            .ok_or_else(|| geometry_error(raw, "B-spline properties are truncated"))?,
        declared_poles: usize::try_from(read_u32(primary, 0x70).unwrap_or_default())
            .unwrap_or(usize::MAX),
    })
}

fn decode_dimension(
    raw: &V8RawObject,
    common: &V8CommonHeader,
    metadata: &V8ModelMetadata,
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let anchor = match common.dimension {
        V8Dimension::Three if primary.len() >= 0x118 => Some(decode_point(
            primary,
            0x100,
            common.dimension,
            metadata,
            raw,
        )?),
        V8Dimension::Two if primary.len() >= 0x110 => Some(decode_point(
            primary,
            0x100,
            common.dimension,
            metadata,
            raw,
        )?),
        _ => None,
    };
    Ok(V8ElementData::Dimension { anchor })
}

fn build_hierarchy(elements: &mut [V8Element], max_depth: usize) -> Result<(), DgnError> {
    #[derive(Debug)]
    struct OpenHeader {
        index: usize,
        remaining: usize,
    }

    let mut stack = Vec::<OpenHeader>::new();
    for index in 0..elements.len() {
        while stack.last().is_some_and(|header| header.remaining == 0) {
            stack.pop();
        }
        let is_component = elements[index].raw.role.is_component();
        if is_component {
            let parent = stack
                .last_mut()
                .ok_or_else(|| DgnError::InvalidV8Hierarchy {
                    index,
                    context: "component has no open complex header".to_owned(),
                })?;
            parent.remaining -= 1;
            let parent_index = parent.index;
            elements[index].parent_index = Some(parent_index);
            elements[parent_index].child_indices.push(index);
        } else if let Some(parent) = stack.last() {
            return Err(DgnError::InvalidV8Hierarchy {
                index,
                context: format!(
                    "standalone object appears while header {} still expects {} direct children",
                    parent.index, parent.remaining
                ),
            });
        }

        if elements[index].raw.role.is_header() {
            let child_count = elements[index].data.declared_child_count().ok_or_else(|| {
                DgnError::InvalidV8Hierarchy {
                    index,
                    context: "header role has no supported child-count field".to_owned(),
                }
            })?;
            if child_count > 0 {
                stack.push(OpenHeader {
                    index,
                    remaining: child_count,
                });
                if stack.len() > max_depth {
                    return Err(DgnError::V8HierarchyDepthLimitExceeded {
                        index,
                        limit: max_depth,
                    });
                }
            }
        }
    }
    while stack.last().is_some_and(|header| header.remaining == 0) {
        stack.pop();
    }
    if let Some(header) = stack.last() {
        return Err(DgnError::InvalidV8Hierarchy {
            index: header.index,
            context: format!(
                "complex header is missing {} direct children at end of model",
                header.remaining
            ),
        });
    }
    Ok(())
}

fn resolve_container_dimensions(elements: &mut [V8Element]) {
    for index in (0..elements.len()).rev() {
        if !matches!(
            elements[index].data,
            V8ElementData::ComplexChain { .. } | V8ElementData::ComplexShape { .. }
        ) || elements[index].child_indices.is_empty()
        {
            continue;
        }
        elements[index].common.dimension = if elements[index]
            .child_indices
            .iter()
            .any(|child| elements[*child].common.dimension == V8Dimension::Three)
        {
            V8Dimension::Three
        } else {
            V8Dimension::Two
        };
    }
}

fn decode_point(
    bytes: &[u8],
    offset: usize,
    dimension: V8Dimension,
    metadata: &V8ModelMetadata,
    raw: &V8RawObject,
) -> Result<V8Point, DgnError> {
    let x = finite_f64(bytes, offset, raw, "point X")?;
    let y = finite_f64(bytes, offset + 8, raw, "point Y")?;
    let z = match dimension {
        V8Dimension::Two => 0.0,
        V8Dimension::Three => finite_f64(bytes, offset + 16, raw, "point Z")?,
    };
    let uor = V8Point3::new(x, y, z);
    Ok(V8Point {
        uor,
        master: metadata.to_master(uor),
    })
}

const fn point_bytes(dimension: V8Dimension) -> usize {
    match dimension {
        V8Dimension::Two => 16,
        V8Dimension::Three => 24,
    }
}

fn finite_f64(
    bytes: &[u8],
    offset: usize,
    raw: &V8RawObject,
    context: &str,
) -> Result<f64, DgnError> {
    let value = read_f64(bytes, offset)
        .ok_or_else(|| geometry_error(raw, format!("{context} at 0x{offset:x} is truncated")))?;
    if !value.is_finite() {
        return Err(geometry_error(
            raw,
            format!("{context} at 0x{offset:x} is not finite"),
        ));
    }
    Ok(value)
}

fn read_f64_array<const N: usize>(
    bytes: &[u8],
    offset: usize,
    raw: &V8RawObject,
) -> Result<[f64; N], DgnError> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = finite_f64(bytes, offset + index * 8, raw, "floating array value")?;
    }
    Ok(values)
}

fn read_f64_values(
    bytes: &[u8],
    offset: usize,
    count: usize,
    raw: &V8RawObject,
) -> Result<Vec<f64>, DgnError> {
    (0..count)
        .map(|index| finite_f64(bytes, offset + index * 8, raw, "transform value"))
        .collect()
}

fn count_at(raw: &V8RawObject, bytes: &[u8], offset: usize) -> Result<usize, DgnError> {
    usize::try_from(
        read_u32(bytes, offset)
            .ok_or_else(|| geometry_error(raw, format!("count at 0x{offset:x} is truncated")))?,
    )
    .map_err(|_| geometry_error(raw, "count does not fit usize"))
}

fn validate_vertex_count(
    raw: &V8RawObject,
    count: usize,
    minimum: usize,
    maximum: usize,
) -> Result<(), DgnError> {
    if count < minimum {
        return Err(geometry_error(
            raw,
            format!("declares {count} vertices; at least {minimum} are required"),
        ));
    }
    if count > maximum {
        return Err(DgnError::V8VertexLimitExceeded {
            element_id: raw.element_id.unwrap_or_default(),
            actual: count,
            limit: maximum,
        });
    }
    Ok(())
}

fn require_minimum(
    raw: &V8RawObject,
    bytes: &[u8],
    required: usize,
    context: &str,
) -> Result<(), DgnError> {
    if bytes.len() < required {
        return Err(geometry_error(
            raw,
            format!("{context} needs {required} bytes, got {}", bytes.len()),
        ));
    }
    Ok(())
}

fn require_primary_size(raw: &V8RawObject, bytes: &[u8], required: usize) -> Result<(), DgnError> {
    if bytes.len() != required {
        return Err(geometry_error(
            raw,
            format!(
                "known type requires {required} primary bytes, got {}",
                bytes.len()
            ),
        ));
    }
    Ok(())
}

fn geometry_error(raw: &V8RawObject, context: impl Into<String>) -> DgnError {
    DgnError::InvalidV8Geometry {
        element_id: raw.element_id.unwrap_or_default(),
        element_type: raw.element_type,
        context: context.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_text_escape_marker_without_losing_source_bytes() {
        let raw = [0xff, 0xfe, 1, 0, b'm', b'y', b'T', 0xe9, b'x', b't'];
        let decoded = decode_windows_1252(&raw[4..]);
        assert_eq!(decoded, "myTéxt");
    }
}
