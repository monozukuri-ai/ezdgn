//! Seed-based V7 2D writer for the native primitive subset.

use crate::numbers::encode_vax_d_f64;
use crate::{
    decode_design_settings, scan_records, DesignSettings, DgnError, Point2, ScanOptions,
    V7Dimension, MAX_V7_RECORD_SIZE_BYTES,
};

const TCB: u8 = 9;
const DIGITIZER_SETUP: u8 = 8;
const LEVEL_SYMBOLOGY: u8 = 10;
const GROUP_DATA: u8 = 5;
const COLOR_TABLE_LEVEL: u8 = 1;
const LINE: u8 = 3;
const LINE_STRING: u8 = 4;
const SHAPE: u8 = 6;
const CURVE: u8 = 11;
const ELLIPSE: u8 = 15;
const ARC: u8 = 16;
const TEXT: u8 = 17;
const MAX_MULTIPOINT_VERTICES: usize = 101;
const ANGLE_UNITS_PER_DEGREE: f64 = 360_000.0;
const MIN_DESIGN_COORDINATE: f64 = -2_147_483_647.0;
const MAX_DESIGN_COORDINATE: f64 = 2_147_483_647.0;
const ATTRIBUTE_PROPERTY: u16 = 0x0800;
const END_MARKER: [u8; 2] = [0xff, 0xff];

/// Common display attributes applied to a generated V7 graphic element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V7ElementStyle {
    pub level: u8,
    pub color: u8,
    pub line_style: u8,
    pub line_weight: u8,
    pub graphic_group: u16,
    pub properties: u16,
}

impl Default for V7ElementStyle {
    fn default() -> Self {
        Self {
            level: 1,
            color: 0,
            line_style: 0,
            line_weight: 0,
            graphic_group: 0,
            properties: 0x0200,
        }
    }
}

/// Controls which records are inherited from the seed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V7WriteOptions {
    /// Copy the last type-5/level-1 color table when only the mandatory seed
    /// controls are inherited.
    pub copy_color_table: bool,
    /// Copy every seed record before appending generated elements. This can
    /// intentionally retain graphics already present in a non-empty seed.
    pub copy_seed_elements: bool,
}

impl Default for V7WriteOptions {
    fn default() -> Self {
        Self {
            copy_color_table: true,
            copy_seed_elements: false,
        }
    }
}

/// Owned write request for one supported V7 2D entity.
#[derive(Debug, Clone, PartialEq)]
pub enum WritableElement2D {
    Line {
        start: Point2<f64>,
        end: Point2<f64>,
        style: V7ElementStyle,
    },
    LineString {
        vertices: Vec<Point2<f64>>,
        style: V7ElementStyle,
    },
    Shape {
        vertices: Vec<Point2<f64>>,
        fill_color: Option<u8>,
        style: V7ElementStyle,
    },
    Curve {
        vertices: Vec<Point2<f64>>,
        style: V7ElementStyle,
    },
    Ellipse {
        center: Point2<f64>,
        primary_axis: f64,
        secondary_axis: f64,
        rotation_degrees: f64,
        style: V7ElementStyle,
    },
    Arc {
        center: Point2<f64>,
        primary_axis: f64,
        secondary_axis: f64,
        rotation_degrees: f64,
        start_angle_degrees: f64,
        sweep_angle_degrees: f64,
        style: V7ElementStyle,
    },
    Text {
        origin: Point2<f64>,
        text: Vec<u8>,
        font_id: u8,
        justification: u8,
        length_multiplier: f64,
        height_multiplier: f64,
        rotation_degrees: f64,
        style: V7ElementStyle,
    },
}

impl WritableElement2D {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Line { .. } => "LINE",
            Self::LineString { .. } => "LINE_STRING",
            Self::Shape { .. } => "SHAPE",
            Self::Curve { .. } => "CURVE",
            Self::Ellipse { .. } => "ELLIPSE",
            Self::Arc { .. } => "ARC",
            Self::Text { .. } => "TEXT",
        }
    }
}

/// Creates a standalone V7 2D byte stream from a seed and native entities.
///
/// Coordinates and distances are expressed in the seed's master units. The
/// seed's TCB is copied byte-for-byte, so its units, global origin, design
/// plane, views, and related settings remain authoritative.
pub fn write_v7_2d(
    seed: &[u8],
    elements: &[WritableElement2D],
    options: V7WriteOptions,
) -> Result<Vec<u8>, DgnError> {
    let scan = scan_records(seed, ScanOptions::default())?;
    let settings = decode_design_settings(&scan)?;
    if settings.dimension != V7Dimension::Two {
        return Err(DgnError::UnsupportedDimension {
            dimension: settings.dimension,
        });
    }
    if settings.scale().is_none() {
        return Err(DgnError::InvalidWriterSeed {
            context: "TCB has a zero UOR scale",
        });
    }
    validate_seed_controls(&scan.records)?;

    let mut output = Vec::new();
    if options.copy_seed_elements {
        for record in &scan.records {
            output.extend_from_slice(record.bytes);
        }
    } else {
        for record in &scan.records[..3] {
            output.extend_from_slice(record.bytes);
        }
        if options.copy_color_table {
            if let Some(record) = scan.records.iter().rev().find(|record| {
                record.header.element_type == GROUP_DATA
                    && record.header.level == COLOR_TABLE_LEVEL
                    && !record.header.deleted
            }) {
                output.extend_from_slice(record.bytes);
            }
        }
    }

    for element in elements {
        let record = encode_element(element, settings)?;
        output.extend_from_slice(&record);
    }
    output.extend_from_slice(&END_MARKER);
    Ok(output)
}

fn validate_seed_controls(records: &[crate::RawElementRef<'_>]) -> Result<(), DgnError> {
    if records.len() < 3 {
        return Err(DgnError::InvalidWriterSeed {
            context: "expected TCB, digitizer setup, and level symbology records",
        });
    }
    for (record, expected) in records.iter().zip([TCB, DIGITIZER_SETUP, LEVEL_SYMBOLOGY]) {
        if record.header.element_type != expected || record.header.deleted {
            return Err(DgnError::InvalidWriterSeed {
                context:
                    "first three records are not active TCB, digitizer setup, and level symbology",
            });
        }
    }
    Ok(())
}

fn encode_element(
    element: &WritableElement2D,
    settings: DesignSettings,
) -> Result<Vec<u8>, DgnError> {
    match element {
        WritableElement2D::Line { start, end, style } => {
            validate_style(*style, element.kind())?;
            let points = encode_integer_points(&[*start, *end], settings, element.kind())?;
            let mut body = Vec::with_capacity(16);
            push_point(&mut body, points[0]);
            push_point(&mut body, points[1]);
            graphic_record(
                LINE,
                *style,
                point_bounds(&points),
                body,
                Vec::new(),
                element.kind(),
            )
        }
        WritableElement2D::LineString { vertices, style } => encode_multipoint(
            LINE_STRING,
            vertices,
            2,
            *style,
            None,
            settings,
            element.kind(),
        ),
        WritableElement2D::Shape {
            vertices,
            fill_color,
            style,
        } => encode_multipoint(
            SHAPE,
            vertices,
            3,
            *style,
            *fill_color,
            settings,
            element.kind(),
        ),
        WritableElement2D::Curve { vertices, style } => {
            encode_multipoint(CURVE, vertices, 2, *style, None, settings, element.kind())
        }
        WritableElement2D::Ellipse {
            center,
            primary_axis,
            secondary_axis,
            rotation_degrees,
            style,
        } => encode_ellipse(
            *center,
            *primary_axis,
            *secondary_axis,
            *rotation_degrees,
            *style,
            settings,
        ),
        WritableElement2D::Arc {
            center,
            primary_axis,
            secondary_axis,
            rotation_degrees,
            start_angle_degrees,
            sweep_angle_degrees,
            style,
        } => encode_arc(
            *center,
            *primary_axis,
            *secondary_axis,
            *rotation_degrees,
            *start_angle_degrees,
            *sweep_angle_degrees,
            *style,
            settings,
        ),
        WritableElement2D::Text {
            origin,
            text,
            font_id,
            justification,
            length_multiplier,
            height_multiplier,
            rotation_degrees,
            style,
        } => encode_text(
            *origin,
            text,
            *font_id,
            *justification,
            *length_multiplier,
            *height_multiplier,
            *rotation_degrees,
            *style,
            settings,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_multipoint(
    element_type: u8,
    vertices: &[Point2<f64>],
    minimum: usize,
    style: V7ElementStyle,
    fill_color: Option<u8>,
    settings: DesignSettings,
    entity: &'static str,
) -> Result<Vec<u8>, DgnError> {
    validate_style(style, entity)?;
    if vertices.len() < minimum {
        return Err(DgnError::InvalidWriterEntity {
            entity,
            context: "too few vertices",
        });
    }
    if vertices.len() > MAX_MULTIPOINT_VERTICES {
        return Err(DgnError::InvalidWriterEntity {
            entity,
            context: "more than 101 vertices require a complex element",
        });
    }
    let count = u16::try_from(vertices.len()).map_err(|_| DgnError::InvalidWriterEntity {
        entity,
        context: "vertex count does not fit a V7 element",
    })?;
    let points = encode_integer_points(vertices, settings, entity)?;
    let mut body = Vec::with_capacity(2 + points.len() * 8);
    body.extend_from_slice(&count.to_le_bytes());
    for point in &points {
        push_point(&mut body, *point);
    }
    let attributes = fill_color.map(shape_fill_linkage).unwrap_or_default();
    graphic_record(
        element_type,
        style,
        point_bounds(&points),
        body,
        attributes,
        entity,
    )
}

fn encode_ellipse(
    center: Point2<f64>,
    primary_axis: f64,
    secondary_axis: f64,
    rotation_degrees: f64,
    style: V7ElementStyle,
    settings: DesignSettings,
) -> Result<Vec<u8>, DgnError> {
    let entity = "ELLIPSE";
    validate_style(style, entity)?;
    validate_positive(
        primary_axis,
        entity,
        "primary axis must be finite and positive",
    )?;
    validate_positive(
        secondary_axis,
        entity,
        "secondary axis must be finite and positive",
    )?;
    let center_raw = master_to_raw_f64(center, settings, entity)?;
    let primary_raw = distance_to_raw(primary_axis, settings, entity)?;
    let secondary_raw = distance_to_raw(secondary_axis, settings, entity)?;
    let rotation_raw = angle_to_i32(rotation_degrees, entity)?;
    let radius = primary_raw.max(secondary_raw);
    let bounds = fractional_bounds(center_raw, radius, entity)?;

    let mut body = Vec::with_capacity(36);
    body.extend_from_slice(&encode_vax_d_f64(primary_raw));
    body.extend_from_slice(&encode_vax_d_f64(secondary_raw));
    body.extend_from_slice(&encode_middle_i32(rotation_raw));
    body.extend_from_slice(&encode_vax_d_f64(center_raw.x));
    body.extend_from_slice(&encode_vax_d_f64(center_raw.y));
    graphic_record(ELLIPSE, style, bounds, body, Vec::new(), entity)
}

#[allow(clippy::too_many_arguments)]
fn encode_arc(
    center: Point2<f64>,
    primary_axis: f64,
    secondary_axis: f64,
    rotation_degrees: f64,
    start_angle_degrees: f64,
    sweep_angle_degrees: f64,
    style: V7ElementStyle,
    settings: DesignSettings,
) -> Result<Vec<u8>, DgnError> {
    let entity = "ARC";
    validate_style(style, entity)?;
    validate_positive(
        primary_axis,
        entity,
        "primary axis must be finite and positive",
    )?;
    validate_positive(
        secondary_axis,
        entity,
        "secondary axis must be finite and positive",
    )?;
    let center_raw = master_to_raw_f64(center, settings, entity)?;
    let primary_raw = distance_to_raw(primary_axis, settings, entity)?;
    let secondary_raw = distance_to_raw(secondary_axis, settings, entity)?;
    let rotation_raw = angle_to_i32(rotation_degrees, entity)?;
    let start_raw = angle_to_i32(start_angle_degrees, entity)?;
    let sweep_raw = encode_sweep(sweep_angle_degrees)?;
    let radius = primary_raw.max(secondary_raw);
    let bounds = fractional_bounds(center_raw, radius, entity)?;

    let mut body = Vec::with_capacity(44);
    body.extend_from_slice(&encode_middle_i32(start_raw));
    body.extend_from_slice(&encode_middle_u32(sweep_raw));
    body.extend_from_slice(&encode_vax_d_f64(primary_raw));
    body.extend_from_slice(&encode_vax_d_f64(secondary_raw));
    body.extend_from_slice(&encode_middle_i32(rotation_raw));
    body.extend_from_slice(&encode_vax_d_f64(center_raw.x));
    body.extend_from_slice(&encode_vax_d_f64(center_raw.y));
    graphic_record(ARC, style, bounds, body, Vec::new(), entity)
}

#[allow(clippy::too_many_arguments)]
fn encode_text(
    origin: Point2<f64>,
    text: &[u8],
    font_id: u8,
    justification: u8,
    length_multiplier: f64,
    height_multiplier: f64,
    rotation_degrees: f64,
    style: V7ElementStyle,
    settings: DesignSettings,
) -> Result<Vec<u8>, DgnError> {
    let entity = "TEXT";
    validate_style(style, entity)?;
    if text.len() > usize::from(u8::MAX) {
        return Err(DgnError::InvalidWriterEntity {
            entity,
            context: "text payload exceeds the V7 255-byte field",
        });
    }
    validate_positive(
        length_multiplier,
        entity,
        "length multiplier must be finite and positive",
    )?;
    validate_positive(
        height_multiplier,
        entity,
        "height multiplier must be finite and positive",
    )?;
    let origin_raw = master_to_raw_i32(origin, settings, entity)?;
    let multiplier_factor = settings.scale().ok_or(DgnError::InvalidWriterSeed {
        context: "TCB has a zero UOR scale",
    })? * 6.0
        / 1000.0;
    let length_raw = positive_i32(
        length_multiplier / multiplier_factor,
        entity,
        "length multiplier is outside the V7 range",
    )?;
    let height_raw = positive_i32(
        height_multiplier / multiplier_factor,
        entity,
        "height multiplier is outside the V7 range",
    )?;
    let rotation_raw = angle_to_i32(rotation_degrees, entity)?;

    let width = length_multiplier * text.len() as f64;
    let low = Point2 {
        x: origin.x - width,
        y: origin.y - height_multiplier,
    };
    let high = Point2 {
        x: origin.x + width,
        y: origin.y + height_multiplier,
    };
    let bounds = master_box_bounds(low, high, settings, entity)?;

    let mut body = vec![0; 24 + text.len()];
    body[0] = font_id;
    body[1] = justification;
    body[2..6].copy_from_slice(&encode_middle_i32(length_raw));
    body[6..10].copy_from_slice(&encode_middle_i32(height_raw));
    body[10..14].copy_from_slice(&encode_middle_i32(rotation_raw));
    body[14..18].copy_from_slice(&encode_middle_i32(origin_raw.x));
    body[18..22].copy_from_slice(&encode_middle_i32(origin_raw.y));
    body[22] = text.len() as u8;
    body[23] = 0;
    body[24..].copy_from_slice(text);
    if body.len() % 2 != 0 {
        body.push(0);
    }
    graphic_record(TEXT, style, bounds, body, Vec::new(), entity)
}

fn graphic_record(
    element_type: u8,
    style: V7ElementStyle,
    bounds: (Point2<i32>, Point2<i32>),
    body: Vec<u8>,
    attributes: Vec<u8>,
    entity: &'static str,
) -> Result<Vec<u8>, DgnError> {
    let semantic_size = 36_usize
        .checked_add(body.len())
        .ok_or(DgnError::WriterRecordTooLarge {
            entity,
            size: usize::MAX,
            limit: MAX_V7_RECORD_SIZE_BYTES,
        })?;
    let size =
        semantic_size
            .checked_add(attributes.len())
            .ok_or(DgnError::WriterRecordTooLarge {
                entity,
                size: usize::MAX,
                limit: MAX_V7_RECORD_SIZE_BYTES,
            })?;
    if size % 2 != 0 || size > MAX_V7_RECORD_SIZE_BYTES {
        return Err(DgnError::WriterRecordTooLarge {
            entity,
            size,
            limit: MAX_V7_RECORD_SIZE_BYTES,
        });
    }
    let words_to_follow =
        u16::try_from(size / 2 - 2).map_err(|_| DgnError::WriterRecordTooLarge {
            entity,
            size,
            limit: MAX_V7_RECORD_SIZE_BYTES,
        })?;
    let attribute_index =
        u16::try_from((semantic_size - 32) / 2).map_err(|_| DgnError::WriterRecordTooLarge {
            entity,
            size,
            limit: MAX_V7_RECORD_SIZE_BYTES,
        })?;

    let mut record = vec![0; size];
    record[0] = style.level;
    record[1] = element_type;
    record[2..4].copy_from_slice(&words_to_follow.to_le_bytes());
    write_range(&mut record, bounds);
    record[28..30].copy_from_slice(&style.graphic_group.to_le_bytes());
    record[30..32].copy_from_slice(&attribute_index.to_le_bytes());
    let properties = if attributes.is_empty() {
        style.properties & !ATTRIBUTE_PROPERTY
    } else {
        style.properties | ATTRIBUTE_PROPERTY
    };
    record[32..34].copy_from_slice(&properties.to_le_bytes());
    record[34] = style.line_style | (style.line_weight << 3);
    record[35] = style.color;
    record[36..semantic_size].copy_from_slice(&body);
    record[semantic_size..].copy_from_slice(&attributes);
    Ok(record)
}

fn validate_style(style: V7ElementStyle, entity: &'static str) -> Result<(), DgnError> {
    if style.level > 63 {
        return Err(DgnError::InvalidWriterEntity {
            entity,
            context: "level must be between 0 and 63",
        });
    }
    if style.line_style > 7 {
        return Err(DgnError::InvalidWriterEntity {
            entity,
            context: "line style must be between 0 and 7",
        });
    }
    if style.line_weight > 31 {
        return Err(DgnError::InvalidWriterEntity {
            entity,
            context: "line weight must be between 0 and 31",
        });
    }
    Ok(())
}

fn encode_integer_points(
    points: &[Point2<f64>],
    settings: DesignSettings,
    entity: &'static str,
) -> Result<Vec<Point2<i32>>, DgnError> {
    points
        .iter()
        .copied()
        .map(|point| master_to_raw_i32(point, settings, entity))
        .collect()
}

fn master_to_raw_i32(
    point: Point2<f64>,
    settings: DesignSettings,
    entity: &'static str,
) -> Result<Point2<i32>, DgnError> {
    let raw = master_to_raw_f64(point, settings, entity)?;
    Ok(Point2 {
        x: checked_coordinate(raw.x.round(), entity, "x")?,
        y: checked_coordinate(raw.y.round(), entity, "y")?,
    })
}

fn master_to_raw_f64(
    point: Point2<f64>,
    settings: DesignSettings,
    entity: &'static str,
) -> Result<Point2<f64>, DgnError> {
    if !point.x.is_finite() {
        return Err(DgnError::WriterCoordinateOutOfRange { entity, axis: "x" });
    }
    if !point.y.is_finite() {
        return Err(DgnError::WriterCoordinateOutOfRange { entity, axis: "y" });
    }
    let scale = settings.scale().ok_or(DgnError::InvalidWriterSeed {
        context: "TCB has a zero UOR scale",
    })?;
    let origin = settings
        .global_origin_master()
        .ok_or(DgnError::InvalidWriterSeed {
            context: "TCB has a zero UOR scale",
        })?;
    let raw = Point2 {
        x: (point.x + origin[0]) / scale,
        y: (point.y + origin[1]) / scale,
    };
    checked_coordinate_f64(raw.x, entity, "x")?;
    checked_coordinate_f64(raw.y, entity, "y")?;
    Ok(raw)
}

fn distance_to_raw(
    distance: f64,
    settings: DesignSettings,
    entity: &'static str,
) -> Result<f64, DgnError> {
    let scale = settings.scale().ok_or(DgnError::InvalidWriterSeed {
        context: "TCB has a zero UOR scale",
    })?;
    let raw = distance / scale;
    if !raw.is_finite() || raw <= 0.0 || raw > MAX_DESIGN_COORDINATE {
        return Err(DgnError::InvalidWriterEntity {
            entity,
            context: "axis length is outside the V7 design plane",
        });
    }
    Ok(raw)
}

fn validate_positive(
    value: f64,
    entity: &'static str,
    context: &'static str,
) -> Result<(), DgnError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(DgnError::InvalidWriterEntity { entity, context });
    }
    Ok(())
}

fn positive_i32(value: f64, entity: &'static str, context: &'static str) -> Result<i32, DgnError> {
    if !value.is_finite() || value <= 0.0 || value > MAX_DESIGN_COORDINATE {
        return Err(DgnError::InvalidWriterEntity { entity, context });
    }
    let rounded = value.round();
    if rounded < 1.0 {
        return Err(DgnError::InvalidWriterEntity { entity, context });
    }
    Ok(rounded as i32)
}

fn angle_to_i32(degrees: f64, entity: &'static str) -> Result<i32, DgnError> {
    let value = degrees * ANGLE_UNITS_PER_DEGREE;
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(DgnError::InvalidWriterEntity {
            entity,
            context: "angle is outside the V7 signed angle range",
        });
    }
    Ok(value.round() as i32)
}

fn encode_sweep(degrees: f64) -> Result<u32, DgnError> {
    if !degrees.is_finite() || !(-360.0..=360.0).contains(&degrees) {
        return Err(DgnError::InvalidWriterEntity {
            entity: "ARC",
            context: "sweep angle must be finite and between -360 and 360 degrees",
        });
    }
    if degrees == 0.0 || degrees == 360.0 {
        return Ok(0);
    }
    let magnitude = (degrees.abs() * ANGLE_UNITS_PER_DEGREE).round() as u32;
    if magnitude == 0 {
        return Err(DgnError::InvalidWriterEntity {
            entity: "ARC",
            context: "sweep angle is smaller than one V7 angle unit",
        });
    }
    Ok(if degrees.is_sign_negative() {
        magnitude | 0x8000_0000
    } else {
        magnitude
    })
}

fn checked_coordinate(
    value: f64,
    entity: &'static str,
    axis: &'static str,
) -> Result<i32, DgnError> {
    checked_coordinate_f64(value, entity, axis)?;
    Ok(value as i32)
}

fn checked_coordinate_f64(
    value: f64,
    entity: &'static str,
    axis: &'static str,
) -> Result<(), DgnError> {
    if !value.is_finite() || !(MIN_DESIGN_COORDINATE..=MAX_DESIGN_COORDINATE).contains(&value) {
        return Err(DgnError::WriterCoordinateOutOfRange { entity, axis });
    }
    Ok(())
}

fn point_bounds(points: &[Point2<i32>]) -> (Point2<i32>, Point2<i32>) {
    let mut low = points[0];
    let mut high = points[0];
    for point in &points[1..] {
        low.x = low.x.min(point.x);
        low.y = low.y.min(point.y);
        high.x = high.x.max(point.x);
        high.y = high.y.max(point.y);
    }
    (low, high)
}

fn fractional_bounds(
    center: Point2<f64>,
    radius: f64,
    entity: &'static str,
) -> Result<(Point2<i32>, Point2<i32>), DgnError> {
    Ok((
        Point2 {
            x: checked_coordinate((center.x - radius).floor(), entity, "x")?,
            y: checked_coordinate((center.y - radius).floor(), entity, "y")?,
        },
        Point2 {
            x: checked_coordinate((center.x + radius).ceil(), entity, "x")?,
            y: checked_coordinate((center.y + radius).ceil(), entity, "y")?,
        },
    ))
}

fn master_box_bounds(
    low: Point2<f64>,
    high: Point2<f64>,
    settings: DesignSettings,
    entity: &'static str,
) -> Result<(Point2<i32>, Point2<i32>), DgnError> {
    let low = master_to_raw_f64(low, settings, entity)?;
    let high = master_to_raw_f64(high, settings, entity)?;
    Ok((
        Point2 {
            x: checked_coordinate(low.x.floor(), entity, "x")?,
            y: checked_coordinate(low.y.floor(), entity, "y")?,
        },
        Point2 {
            x: checked_coordinate(high.x.ceil(), entity, "x")?,
            y: checked_coordinate(high.y.ceil(), entity, "y")?,
        },
    ))
}

fn shape_fill_linkage(color: u8) -> Vec<u8> {
    let mut linkage = vec![
        0x07, 0x10, 0x41, 0x00, 0x02, 0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    linkage[8] = color;
    linkage
}

fn write_range(record: &mut [u8], bounds: (Point2<i32>, Point2<i32>)) {
    for (offset, value) in [
        (4, bounds.0.x),
        (8, bounds.0.y),
        (12, 0),
        (16, bounds.1.x),
        (20, bounds.1.y),
        (24, 0),
    ] {
        record[offset..offset + 4].copy_from_slice(&encode_offset_i32(value));
    }
}

fn push_point(output: &mut Vec<u8>, point: Point2<i32>) {
    output.extend_from_slice(&encode_middle_i32(point.x));
    output.extend_from_slice(&encode_middle_i32(point.y));
}

fn encode_middle_i32(value: i32) -> [u8; 4] {
    encode_middle_u32(value as u32)
}

fn encode_offset_i32(value: i32) -> [u8; 4] {
    encode_middle_u32((value as u32) ^ 0x8000_0000)
}

fn encode_middle_u32(value: u32) -> [u8; 4] {
    [
        (value >> 16) as u8,
        (value >> 24) as u8,
        value as u8,
        (value >> 8) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_matches_public_writer_contract() {
        assert_eq!(
            V7ElementStyle::default(),
            V7ElementStyle {
                level: 1,
                color: 0,
                line_style: 0,
                line_weight: 0,
                graphic_group: 0,
                properties: 0x0200,
            }
        );
    }

    #[test]
    fn fill_linkage_has_expected_shape_fill_layout() {
        let linkage = shape_fill_linkage(83);
        assert_eq!(linkage.len(), 16);
        assert_eq!(&linkage[..4], &[0x07, 0x10, 0x41, 0x00]);
        assert_eq!(linkage[8], 83);
    }
}
