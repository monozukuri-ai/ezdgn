//! Lossless V7 2D entity, hierarchy, and B-spline decoding.

use std::collections::HashSet;

use crate::numbers::{decode_middle_endian_i32, decode_middle_endian_u32, decode_vax_d_f64};
use crate::{
    decode_attribute_linkages, decode_common_header, decode_design_settings, scan_records,
    AttributeLinkage, CommonElementHeader, DesignSettings, DgnError, LinkageData, RawElementRef,
    RecordScan, ScanOptions, V7Dimension,
};

const CELL_HEADER: u8 = 2;
const LINE: u8 = 3;
const LINE_STRING: u8 = 4;
const GROUP_DATA: u8 = 5;
const COLOR_TABLE_LEVEL: u8 = 1;
const SHAPE: u8 = 6;
const TEXT_NODE: u8 = 7;
const CURVE: u8 = 11;
const COMPLEX_CHAIN: u8 = 12;
const COMPLEX_SHAPE: u8 = 14;
const ELLIPSE: u8 = 15;
const ARC: u8 = 16;
const TEXT: u8 = 17;
const BSPLINE_POLE: u8 = 21;
const BSPLINE_SURFACE: u8 = 24;
const BSPLINE_SURFACE_BOUNDARY: u8 = 25;
const BSPLINE_KNOT: u8 = 26;
const BSPLINE_CURVE: u8 = 27;
const BSPLINE_WEIGHT: u8 = 28;
const ANGLE_UNITS_PER_DEGREE: f64 = 360_000.0;
const UNIT_I32_MAX: f64 = 2_147_483_647.0;
const SUB_UOR_DIVISOR: f64 = 32_767.0;
const CELL_MATRIX_UNIT: f64 = 10_000.0 / 2_147_483_648.0;

type VertexCoordinates = (Vec<Point2<i32>>, Vec<Point2<f64>>, Option<Vec<Point2<f64>>>);

/// A two-dimensional point in raw, precise UOR, master, or parameter space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point2<T> {
    pub x: T,
    pub y: T,
}

/// Type-3 line with integer storage, sub-UOR corrected coordinates, and
/// optional transformed master-unit points.
#[derive(Debug, Clone, PartialEq)]
pub struct Line2D {
    pub start_uor: Point2<i32>,
    pub end_uor: Point2<i32>,
    pub start_uor_precise: Point2<f64>,
    pub end_uor_precise: Point2<f64>,
    pub start_master: Option<Point2<f64>>,
    pub end_master: Option<Point2<f64>>,
}

/// Type-4 open line string.
#[derive(Debug, Clone, PartialEq)]
pub struct LineString2D {
    pub vertices_uor: Vec<Point2<i32>>,
    pub vertices_uor_precise: Vec<Point2<f64>>,
    pub vertices_master: Option<Vec<Point2<f64>>>,
}

/// Type-6 closed shape. Its encoded sequence is not normalized or implicitly
/// closed, so repeated end vertices remain observable.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape2D {
    pub vertices_uor: Vec<Point2<i32>>,
    pub vertices_uor_precise: Vec<Point2<f64>>,
    pub vertices_master: Option<Vec<Point2<f64>>>,
}

/// Type-11 native parametric curve control sequence. It is never replaced by
/// a display polyline while reading.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve2D {
    pub vertices_uor: Vec<Point2<i32>>,
    pub vertices_uor_precise: Vec<Point2<f64>>,
    pub vertices_master: Option<Vec<Point2<f64>>>,
}

/// Type-2 unshared/nested cell header and placement transform.
#[derive(Debug, Clone, PartialEq)]
pub struct CellHeader2D {
    pub total_length_words: u16,
    pub name_words: [u16; 2],
    pub name: String,
    pub class: u16,
    pub levels: [u16; 4],
    pub range_low_uor: Point2<i32>,
    pub range_high_uor: Point2<i32>,
    pub range_low_master: Option<Point2<f64>>,
    pub range_high_master: Option<Point2<f64>>,
    pub transform_raw: [[i32; 2]; 2],
    pub transform: [[f64; 2]; 2],
    pub origin_uor: Point2<i32>,
    pub origin_master: Option<Point2<f64>>,
}

/// Type-7 text-node header. Direct children are text records and are linked
/// through the enclosing [`Element2D`] indices.
#[derive(Debug, Clone, PartialEq)]
pub struct TextNode2D {
    pub total_length_words: u16,
    pub num_text_strings: u16,
    pub node_number: u16,
    pub max_length: u8,
    pub max_used: u8,
    pub font_id: u8,
    pub justification: u8,
    pub line_spacing_raw: i32,
    pub line_spacing_master: Option<f64>,
    pub length_multiplier_raw: i32,
    pub height_multiplier_raw: i32,
    pub length_multiplier_master: Option<f64>,
    pub height_multiplier_master: Option<f64>,
    pub rotation_raw: i32,
    pub rotation_degrees: f64,
    pub origin_uor: Point2<i32>,
    pub origin_master: Option<Point2<f64>>,
}

/// Shared payload for type-12 and type-14 complex headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComplexHeader2D {
    pub total_length_words: u16,
    pub num_elements: u16,
}

/// Type-15 ellipse preserved as native parameters rather than a polyline.
#[derive(Debug, Clone, PartialEq)]
pub struct Ellipse2D {
    pub center_uor: Point2<f64>,
    pub center_master: Option<Point2<f64>>,
    pub primary_axis_uor: f64,
    pub secondary_axis_uor: f64,
    pub primary_axis_master: Option<f64>,
    pub secondary_axis_master: Option<f64>,
    pub rotation_raw: i32,
    pub rotation_degrees: f64,
}

/// Type-16 elliptical arc preserved as native parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Arc2D {
    pub center_uor: Point2<f64>,
    pub center_master: Option<Point2<f64>>,
    pub primary_axis_uor: f64,
    pub secondary_axis_uor: f64,
    pub primary_axis_master: Option<f64>,
    pub secondary_axis_master: Option<f64>,
    pub rotation_raw: i32,
    pub rotation_degrees: f64,
    pub start_angle_raw: i32,
    pub start_angle_degrees: f64,
    /// Signed sweep after decoding the sign-magnitude representation. Stored
    /// zero remains zero here and maps to 360 degrees.
    pub sweep_angle_raw: i32,
    pub sweep_angle_degrees: f64,
}

/// Type-17 text. The source does not declare an encoding, so text bytes are
/// borrowed verbatim and decoding remains a caller decision.
#[derive(Debug, Clone, PartialEq)]
pub struct Text2D<'a> {
    pub font_id: u8,
    pub justification: u8,
    pub length_multiplier_raw: i32,
    pub height_multiplier_raw: i32,
    pub length_multiplier_master: Option<f64>,
    pub height_multiplier_master: Option<f64>,
    pub rotation_raw: i32,
    pub rotation_degrees: f64,
    pub origin_uor: Point2<i32>,
    pub origin_master: Option<Point2<f64>>,
    pub editable_fields: u8,
    pub text_offset: usize,
    pub text_bytes: &'a [u8],
}

/// Type-21 B-spline pole row.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplinePole2D {
    pub vertices_uor: Vec<Point2<i32>>,
    pub vertices_uor_precise: Vec<Point2<f64>>,
    pub vertices_master: Option<Vec<Point2<f64>>>,
}

/// Type-24 B-spline surface header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BSplineSurface2D {
    pub description_words: i32,
    pub curve_type: u8,
    pub u_order: u8,
    pub u_properties: u8,
    pub num_poles_u: u16,
    pub num_knots_u: u16,
    pub rule_lines_u: u16,
    pub v_order: u8,
    pub v_properties: u8,
    pub num_poles_v: u16,
    pub num_knots_v: u16,
    pub rule_lines_v: u16,
    pub num_bounds: u16,
}

impl BSplineSurface2D {
    #[must_use]
    pub const fn is_rational(self) -> bool {
        self.u_properties & 0x40 != 0
    }

    #[must_use]
    pub const fn is_u_closed(self) -> bool {
        self.u_properties & 0x80 != 0
    }

    #[must_use]
    pub const fn is_v_closed(self) -> bool {
        self.v_properties & 0x80 != 0
    }
}

/// Type-25 trimming boundary in normalized U-V parameter space.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineSurfaceBoundary2D {
    pub number: u16,
    pub vertices_raw: Vec<Point2<i32>>,
    pub vertices_raw_precise: Vec<Point2<f64>>,
    pub vertices_uv: Vec<Point2<f64>>,
}

/// Type-26 non-uniform knot array.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineKnot2D {
    pub values_raw: Vec<i32>,
    pub values: Vec<f64>,
}

/// Type-27 B-spline curve header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BSplineCurve2D {
    pub description_words: i32,
    pub order: u8,
    pub properties: u8,
    pub curve_type: u8,
    pub num_poles: u16,
    pub num_knots: u16,
}

impl BSplineCurve2D {
    #[must_use]
    pub const fn curve_display(self) -> bool {
        self.properties & 0x10 != 0
    }

    #[must_use]
    pub const fn polygon_display(self) -> bool {
        self.properties & 0x20 != 0
    }

    #[must_use]
    pub const fn is_rational(self) -> bool {
        self.properties & 0x40 != 0
    }

    #[must_use]
    pub const fn is_closed(self) -> bool {
        self.properties & 0x80 != 0
    }
}

/// Type-28 rational pole weights.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineWeight2D {
    pub values_raw: Vec<i32>,
    pub values: Vec<f64>,
}

/// Type-5, level-1 color table. Components are ordered red, green, blue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorTable {
    pub screen_flag: u16,
    pub colors: [[u8; 3]; 256],
}

/// Semantic payload for one record in a V7 2D document.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementData2D<'a> {
    Cell(CellHeader2D),
    Line(Line2D),
    LineString(LineString2D),
    Shape(Shape2D),
    TextNode(TextNode2D),
    Curve(Curve2D),
    ComplexChain(ComplexHeader2D),
    ComplexShape(ComplexHeader2D),
    Ellipse(Ellipse2D),
    Arc(Arc2D),
    Text(Text2D<'a>),
    BSplinePole(BSplinePole2D),
    BSplineSurface(BSplineSurface2D),
    BSplineSurfaceBoundary(BSplineSurfaceBoundary2D),
    BSplineKnot(BSplineKnot2D),
    BSplineCurve(BSplineCurve2D),
    BSplineWeight(BSplineWeight2D),
    ColorTable(Box<ColorTable>),
    /// Control, application, and not-yet-supported element types retain their
    /// complete [`RawElementRef`] on the enclosing element.
    Unsupported,
}

impl ElementData2D<'_> {
    /// Stable public name used by the Python object model and CLI.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Cell(_) => "CELL",
            Self::Line(_) => "LINE",
            Self::LineString(_) => "LINE_STRING",
            Self::Shape(_) => "SHAPE",
            Self::TextNode(_) => "TEXT_NODE",
            Self::Curve(_) => "CURVE",
            Self::ComplexChain(_) => "COMPLEX_CHAIN",
            Self::ComplexShape(_) => "COMPLEX_SHAPE",
            Self::Ellipse(_) => "ELLIPSE",
            Self::Arc(_) => "ARC",
            Self::Text(_) => "TEXT",
            Self::BSplinePole(_) => "BSPLINE_POLE",
            Self::BSplineSurface(_) => "BSPLINE_SURFACE",
            Self::BSplineSurfaceBoundary(_) => "BSPLINE_SURFACE_BOUNDARY",
            Self::BSplineKnot(_) => "BSPLINE_KNOT",
            Self::BSplineCurve(_) => "BSPLINE_CURVE",
            Self::BSplineWeight(_) => "BSPLINE_WEIGHT",
            Self::ColorTable(_) => "COLOR_TABLE",
            Self::Unsupported => "UNSUPPORTED",
        }
    }

    /// Whether this record represents a drawable CAD entity. B-spline support
    /// records are excluded; their curve/surface header is the entity.
    #[must_use]
    pub const fn is_graphic(&self) -> bool {
        matches!(
            self,
            Self::Cell(_)
                | Self::Line(_)
                | Self::LineString(_)
                | Self::Shape(_)
                | Self::TextNode(_)
                | Self::Curve(_)
                | Self::ComplexChain(_)
                | Self::ComplexShape(_)
                | Self::Ellipse(_)
                | Self::Arc(_)
                | Self::Text(_)
                | Self::BSplineSurface(_)
                | Self::BSplineCurve(_)
        )
    }
}

/// One ordered record with raw bytes, typed linkages, and parent/child indices.
#[derive(Debug, Clone, PartialEq)]
pub struct Element2D<'a> {
    pub raw: RawElementRef<'a>,
    pub common_header: Option<CommonElementHeader>,
    pub linkages: Vec<AttributeLinkage<'a>>,
    pub parent_index: Option<usize>,
    pub child_indices: Vec<usize>,
    pub data: ElementData2D<'a>,
}

/// Fully scanned V7 2D document. Records are never removed when their semantic
/// type is unsupported or when they are complex components.
#[derive(Debug)]
pub struct V7Document2D<'a> {
    pub scan: RecordScan<'a>,
    pub settings: DesignSettings,
    pub elements: Vec<Element2D<'a>>,
    /// Index into `elements` for the last color table in file order.
    pub active_color_table: Option<usize>,
}

impl V7Document2D<'_> {
    #[must_use]
    pub fn active_colors(&self) -> Option<&ColorTable> {
        let index = self.active_color_table?;
        match &self.elements.get(index)?.data {
            ElementData2D::ColorTable(table) => Some(table),
            _ => None,
        }
    }

    /// File-order indices of records which are not complex components.
    #[must_use]
    pub fn root_indices(&self) -> Vec<usize> {
        self.elements
            .iter()
            .enumerate()
            .filter_map(|(index, element)| element.parent_index.is_none().then_some(index))
            .collect()
    }
}

/// Reads a bounded V7 stream and decodes the Phase-4 native 2D object model.
pub fn read_v7_2d(input: &[u8], options: ScanOptions) -> Result<V7Document2D<'_>, DgnError> {
    let scan = scan_records(input, options)?;
    let settings = decode_design_settings(&scan)?;
    if settings.dimension != V7Dimension::Two {
        return Err(DgnError::UnsupportedDimension {
            dimension: settings.dimension,
        });
    }

    let mut elements = Vec::with_capacity(scan.records.len());
    let mut active_color_table = None;
    for record in scan.records.iter().copied() {
        let common_header = decode_common_header(record, settings.dimension)?;
        let linkages = decode_attribute_linkages(record, common_header);
        let data = decode_element_data(record, common_header, &linkages, settings)?;
        if matches!(data, ElementData2D::ColorTable(_)) {
            active_color_table = Some(elements.len());
        }
        elements.push(Element2D {
            raw: record,
            common_header,
            linkages,
            parent_index: None,
            child_indices: Vec::new(),
            data,
        });
    }
    build_hierarchy(&mut elements, scan.termination.offset())?;
    validate_bspline_groups(&elements)?;

    Ok(V7Document2D {
        scan,
        settings,
        elements,
        active_color_table,
    })
}

fn decode_element_data<'a>(
    record: RawElementRef<'a>,
    common_header: Option<CommonElementHeader>,
    linkages: &[AttributeLinkage<'a>],
    settings: DesignSettings,
) -> Result<ElementData2D<'a>, DgnError> {
    match (record.header.element_type, record.header.level) {
        (CELL_HEADER, _) => decode_cell(record, common_header, settings).map(ElementData2D::Cell),
        (LINE, _) => {
            decode_line(record, common_header, linkages, settings).map(ElementData2D::Line)
        }
        (LINE_STRING, _) => decode_vertices(record, common_header, linkages, settings, 2).map(
            |(vertices_uor, vertices_uor_precise, vertices_master)| {
                ElementData2D::LineString(LineString2D {
                    vertices_uor,
                    vertices_uor_precise,
                    vertices_master,
                })
            },
        ),
        (GROUP_DATA, COLOR_TABLE_LEVEL) => decode_color_table(record, common_header)
            .map(Box::new)
            .map(ElementData2D::ColorTable),
        (SHAPE, _) => decode_vertices(record, common_header, linkages, settings, 2).map(
            |(vertices_uor, vertices_uor_precise, vertices_master)| {
                ElementData2D::Shape(Shape2D {
                    vertices_uor,
                    vertices_uor_precise,
                    vertices_master,
                })
            },
        ),
        (TEXT_NODE, _) => {
            decode_text_node(record, common_header, settings).map(ElementData2D::TextNode)
        }
        (CURVE, _) => decode_vertices(record, common_header, linkages, settings, 2).map(
            |(vertices_uor, vertices_uor_precise, vertices_master)| {
                ElementData2D::Curve(Curve2D {
                    vertices_uor,
                    vertices_uor_precise,
                    vertices_master,
                })
            },
        ),
        (COMPLEX_CHAIN, _) => {
            decode_complex_header(record, common_header).map(ElementData2D::ComplexChain)
        }
        (COMPLEX_SHAPE, _) => {
            decode_complex_header(record, common_header).map(ElementData2D::ComplexShape)
        }
        (ELLIPSE, _) => decode_ellipse(record, common_header, settings).map(ElementData2D::Ellipse),
        (ARC, _) => decode_arc(record, common_header, settings).map(ElementData2D::Arc),
        (TEXT, _) => decode_text(record, common_header, settings).map(ElementData2D::Text),
        (BSPLINE_POLE, _) => decode_vertices(record, common_header, linkages, settings, 2).map(
            |(vertices_uor, vertices_uor_precise, vertices_master)| {
                ElementData2D::BSplinePole(BSplinePole2D {
                    vertices_uor,
                    vertices_uor_precise,
                    vertices_master,
                })
            },
        ),
        (BSPLINE_SURFACE, _) => {
            decode_bspline_surface(record, common_header).map(ElementData2D::BSplineSurface)
        }
        (BSPLINE_SURFACE_BOUNDARY, _) => decode_bspline_boundary(record, common_header, linkages)
            .map(ElementData2D::BSplineSurfaceBoundary),
        (BSPLINE_KNOT, _) => decode_scalar_array(record, common_header, "B-spline knots").map(
            |(values_raw, values)| ElementData2D::BSplineKnot(BSplineKnot2D { values_raw, values }),
        ),
        (BSPLINE_CURVE, _) => {
            decode_bspline_curve(record, common_header).map(ElementData2D::BSplineCurve)
        }
        (BSPLINE_WEIGHT, _) => decode_scalar_array(record, common_header, "B-spline weights").map(
            |(values_raw, values)| {
                ElementData2D::BSplineWeight(BSplineWeight2D { values_raw, values })
            },
        ),
        _ => Ok(ElementData2D::Unsupported),
    }
}

fn decode_cell(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
    settings: DesignSettings,
) -> Result<CellHeader2D, DgnError> {
    require_data_size(record, common_header, 92, "2D cell header")?;
    let name_words = [read_u16(record.bytes, 38), read_u16(record.bytes, 40)];
    let range_low_uor = read_point_i32(record.bytes, 52);
    let range_high_uor = read_point_i32(record.bytes, 60);
    let transform_raw = [
        [
            decode_middle_endian_i32(read_four(record.bytes, 68)),
            decode_middle_endian_i32(read_four(record.bytes, 72)),
        ],
        [
            decode_middle_endian_i32(read_four(record.bytes, 76)),
            decode_middle_endian_i32(read_four(record.bytes, 80)),
        ],
    ];
    let origin_uor = read_point_i32(record.bytes, 84);
    Ok(CellHeader2D {
        total_length_words: read_u16(record.bytes, 36),
        name_words,
        name: decode_radix50_name(name_words),
        class: read_u16(record.bytes, 42),
        levels: [
            read_u16(record.bytes, 44),
            read_u16(record.bytes, 46),
            read_u16(record.bytes, 48),
            read_u16(record.bytes, 50),
        ],
        range_low_uor,
        range_high_uor,
        range_low_master: transform_integer_point(settings, range_low_uor),
        range_high_master: transform_integer_point(settings, range_high_uor),
        transform_raw,
        transform: transform_raw.map(|row| row.map(|value| f64::from(value) * CELL_MATRIX_UNIT)),
        origin_uor,
        origin_master: transform_integer_point(settings, origin_uor),
    })
}

fn decode_line(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
    linkages: &[AttributeLinkage<'_>],
    settings: DesignSettings,
) -> Result<Line2D, DgnError> {
    require_data_size(record, common_header, 52, "2D line coordinates")?;
    let start_uor = read_point_i32(record.bytes, 36);
    let end_uor = read_point_i32(record.bytes, 44);
    let precise = precise_points(&[start_uor, end_uor], linkages);
    Ok(Line2D {
        start_uor,
        end_uor,
        start_uor_precise: precise[0],
        end_uor_precise: precise[1],
        start_master: transform_fractional_point(settings, precise[0]),
        end_master: transform_fractional_point(settings, precise[1]),
    })
}

fn decode_vertices(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
    linkages: &[AttributeLinkage<'_>],
    settings: DesignSettings,
    minimum: usize,
) -> Result<VertexCoordinates, DgnError> {
    require_data_size(record, common_header, 38, "2D vertex count")?;
    let count = usize::from(read_u16(record.bytes, 36));
    if count < minimum {
        return Err(DgnError::InvalidVertexCount {
            offset: record.offset,
            element_type: record.header.element_type,
            count,
            minimum,
        });
    }
    let needed = checked_variable_end(record, 38, count, 8, "2D vertices")?;
    require_data_size(record, common_header, needed, "2D vertices")?;
    let vertices_uor = (0..count)
        .map(|index| read_point_i32(record.bytes, 38 + index * 8))
        .collect::<Vec<_>>();
    let vertices_uor_precise = precise_points(&vertices_uor, linkages);
    let vertices_master = vertices_uor_precise
        .iter()
        .copied()
        .map(|point| transform_fractional_point(settings, point))
        .collect::<Option<Vec<_>>>();
    Ok((vertices_uor, vertices_uor_precise, vertices_master))
}

fn decode_text_node(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
    settings: DesignSettings,
) -> Result<TextNode2D, DgnError> {
    require_data_size(record, common_header, 70, "2D text node header")?;
    let line_spacing_raw = decode_middle_endian_i32(read_four(record.bytes, 46));
    let length_multiplier_raw = decode_middle_endian_i32(read_four(record.bytes, 50));
    let height_multiplier_raw = decode_middle_endian_i32(read_four(record.bytes, 54));
    let rotation_raw = decode_middle_endian_i32(read_four(record.bytes, 58));
    let origin_uor = read_point_i32(record.bytes, 62);
    let multiplier_scale = settings.scale().map(|scale| scale * 6.0 / 1000.0);
    Ok(TextNode2D {
        total_length_words: read_u16(record.bytes, 36),
        num_text_strings: read_u16(record.bytes, 38),
        node_number: read_u16(record.bytes, 40),
        max_length: record.bytes[42],
        max_used: record.bytes[43],
        font_id: record.bytes[44],
        justification: record.bytes[45],
        line_spacing_raw,
        line_spacing_master: settings.transform_distance(f64::from(line_spacing_raw)),
        length_multiplier_raw,
        height_multiplier_raw,
        length_multiplier_master: multiplier_scale
            .map(|scale| f64::from(length_multiplier_raw) * scale),
        height_multiplier_master: multiplier_scale
            .map(|scale| f64::from(height_multiplier_raw) * scale),
        rotation_raw,
        rotation_degrees: angle_degrees(rotation_raw),
        origin_uor,
        origin_master: transform_integer_point(settings, origin_uor),
    })
}

fn decode_complex_header(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
) -> Result<ComplexHeader2D, DgnError> {
    require_data_size(record, common_header, 40, "complex header")?;
    Ok(ComplexHeader2D {
        total_length_words: read_u16(record.bytes, 36),
        num_elements: read_u16(record.bytes, 38),
    })
}

fn decode_ellipse(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
    settings: DesignSettings,
) -> Result<Ellipse2D, DgnError> {
    require_data_size(record, common_header, 72, "2D ellipse parameters")?;
    let primary_axis_uor = decode_vax_d_f64(read_eight(record.bytes, 36));
    let secondary_axis_uor = decode_vax_d_f64(read_eight(record.bytes, 44));
    let rotation_raw = decode_middle_endian_i32(read_four(record.bytes, 52));
    let center_uor = Point2 {
        x: decode_vax_d_f64(read_eight(record.bytes, 56)),
        y: decode_vax_d_f64(read_eight(record.bytes, 64)),
    };
    Ok(Ellipse2D {
        center_uor,
        center_master: transform_fractional_point(settings, center_uor),
        primary_axis_uor,
        secondary_axis_uor,
        primary_axis_master: settings.transform_distance(primary_axis_uor),
        secondary_axis_master: settings.transform_distance(secondary_axis_uor),
        rotation_raw,
        rotation_degrees: angle_degrees(rotation_raw),
    })
}

fn decode_arc(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
    settings: DesignSettings,
) -> Result<Arc2D, DgnError> {
    require_data_size(record, common_header, 80, "2D arc parameters")?;
    let start_angle_raw = decode_middle_endian_i32(read_four(record.bytes, 36));
    let sweep_encoded = decode_middle_endian_u32(read_four(record.bytes, 40));
    let sweep_magnitude = (sweep_encoded & 0x7fff_ffff) as i32;
    let sweep_angle_raw = if sweep_encoded & 0x8000_0000 != 0 {
        -sweep_magnitude
    } else {
        sweep_magnitude
    };
    let primary_axis_uor = decode_vax_d_f64(read_eight(record.bytes, 44));
    let secondary_axis_uor = decode_vax_d_f64(read_eight(record.bytes, 52));
    let rotation_raw = decode_middle_endian_i32(read_four(record.bytes, 60));
    let center_uor = Point2 {
        x: decode_vax_d_f64(read_eight(record.bytes, 64)),
        y: decode_vax_d_f64(read_eight(record.bytes, 72)),
    };
    Ok(Arc2D {
        center_uor,
        center_master: transform_fractional_point(settings, center_uor),
        primary_axis_uor,
        secondary_axis_uor,
        primary_axis_master: settings.transform_distance(primary_axis_uor),
        secondary_axis_master: settings.transform_distance(secondary_axis_uor),
        rotation_raw,
        rotation_degrees: angle_degrees(rotation_raw),
        start_angle_raw,
        start_angle_degrees: angle_degrees(start_angle_raw),
        sweep_angle_raw,
        sweep_angle_degrees: if sweep_angle_raw == 0 {
            360.0
        } else {
            angle_degrees(sweep_angle_raw)
        },
    })
}

fn decode_text<'a>(
    record: RawElementRef<'a>,
    common_header: Option<CommonElementHeader>,
    settings: DesignSettings,
) -> Result<Text2D<'a>, DgnError> {
    require_data_size(record, common_header, 60, "2D text header")?;
    let length_multiplier_raw = decode_middle_endian_i32(read_four(record.bytes, 38));
    let height_multiplier_raw = decode_middle_endian_i32(read_four(record.bytes, 42));
    let rotation_raw = decode_middle_endian_i32(read_four(record.bytes, 46));
    let origin_uor = read_point_i32(record.bytes, 50);
    let text_length = usize::from(record.bytes[58]);
    let text_offset = 60;
    let needed = text_offset + text_length;
    require_data_size(record, common_header, needed, "2D text bytes")?;
    let multiplier_scale = settings.scale().map(|scale| scale * 6.0 / 1000.0);
    Ok(Text2D {
        font_id: record.bytes[36],
        justification: record.bytes[37],
        length_multiplier_raw,
        height_multiplier_raw,
        length_multiplier_master: multiplier_scale
            .map(|scale| f64::from(length_multiplier_raw) * scale),
        height_multiplier_master: multiplier_scale
            .map(|scale| f64::from(height_multiplier_raw) * scale),
        rotation_raw,
        rotation_degrees: angle_degrees(rotation_raw),
        origin_uor,
        origin_master: transform_integer_point(settings, origin_uor),
        editable_fields: record.bytes[59],
        text_offset,
        text_bytes: &record.bytes[text_offset..needed],
    })
}

fn decode_bspline_surface(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
) -> Result<BSplineSurface2D, DgnError> {
    require_data_size(record, common_header, 58, "B-spline surface header")?;
    Ok(BSplineSurface2D {
        description_words: decode_middle_endian_i32(read_four(record.bytes, 36)),
        curve_type: record.bytes[41],
        u_order: (record.bytes[40] & 0x0f) + 2,
        u_properties: record.bytes[40] & 0xf0,
        num_poles_u: read_u16(record.bytes, 42),
        num_knots_u: read_u16(record.bytes, 44),
        rule_lines_u: read_u16(record.bytes, 46),
        v_order: (record.bytes[48] & 0x0f) + 2,
        v_properties: record.bytes[48] & 0xf0,
        num_poles_v: read_u16(record.bytes, 50),
        num_knots_v: read_u16(record.bytes, 52),
        rule_lines_v: read_u16(record.bytes, 54),
        num_bounds: read_u16(record.bytes, 56),
    })
}

fn decode_bspline_boundary(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
    linkages: &[AttributeLinkage<'_>],
) -> Result<BSplineSurfaceBoundary2D, DgnError> {
    require_data_size(record, common_header, 40, "B-spline boundary header")?;
    let count = usize::from(read_u16(record.bytes, 38));
    if count == 0 {
        return Err(DgnError::InvalidVertexCount {
            offset: record.offset,
            element_type: record.header.element_type,
            count,
            minimum: 1,
        });
    }
    let needed = checked_variable_end(record, 40, count, 8, "B-spline boundary vertices")?;
    require_data_size(record, common_header, needed, "B-spline boundary vertices")?;
    let vertices_raw = (0..count)
        .map(|index| read_point_i32(record.bytes, 40 + index * 8))
        .collect::<Vec<_>>();
    let vertices_raw_precise = precise_points(&vertices_raw, linkages);
    let vertices_uv = vertices_raw_precise
        .iter()
        .map(|point| Point2 {
            x: point.x / UNIT_I32_MAX,
            y: point.y / UNIT_I32_MAX,
        })
        .collect();
    Ok(BSplineSurfaceBoundary2D {
        number: read_u16(record.bytes, 36),
        vertices_raw,
        vertices_raw_precise,
        vertices_uv,
    })
}

fn decode_scalar_array(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
    context: &'static str,
) -> Result<(Vec<i32>, Vec<f64>), DgnError> {
    let end = semantic_data_end(record, common_header);
    let data_bytes = end.saturating_sub(36);
    if end < 40 || data_bytes % 4 != 0 {
        return Err(DgnError::InvalidScalarArrayLength {
            offset: record.offset,
            element_type: record.header.element_type,
            data_bytes,
            context,
        });
    }
    let values_raw = (36..end)
        .step_by(4)
        .map(|offset| decode_middle_endian_i32(read_four(record.bytes, offset)))
        .collect::<Vec<_>>();
    let values = values_raw
        .iter()
        .map(|value| f64::from(*value) / UNIT_I32_MAX)
        .collect();
    Ok((values_raw, values))
}

fn decode_bspline_curve(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
) -> Result<BSplineCurve2D, DgnError> {
    require_data_size(record, common_header, 46, "B-spline curve header")?;
    Ok(BSplineCurve2D {
        description_words: decode_middle_endian_i32(read_four(record.bytes, 36)),
        order: (record.bytes[40] & 0x0f) + 2,
        properties: record.bytes[40] & 0xf0,
        curve_type: record.bytes[41],
        num_poles: read_u16(record.bytes, 42),
        num_knots: read_u16(record.bytes, 44),
    })
}

fn decode_color_table(
    record: RawElementRef<'_>,
    common_header: Option<CommonElementHeader>,
) -> Result<ColorTable, DgnError> {
    require_data_size(record, common_header, 806, "V7 color table")?;
    let mut colors = [[0_u8; 3]; 256];
    colors[255].copy_from_slice(&record.bytes[38..41]);
    for (index, color) in colors.iter_mut().take(255).enumerate() {
        let offset = 41 + index * 3;
        color.copy_from_slice(&record.bytes[offset..offset + 3]);
    }
    Ok(ColorTable {
        screen_flag: read_u16(record.bytes, 36),
        colors,
    })
}

#[derive(Debug, Clone, Copy)]
enum ContainerKind {
    Cell,
    Counted(usize),
    BSplineCurve,
    BSplineSurface,
}

#[derive(Debug, Clone, Copy)]
struct ContainerDescriptor {
    end: usize,
    kind: ContainerKind,
}

fn build_hierarchy(elements: &mut [Element2D<'_>], stream_end: usize) -> Result<(), DgnError> {
    let boundaries = elements
        .iter()
        .map(|element| element.raw.offset)
        .chain(std::iter::once(stream_end))
        .collect::<HashSet<_>>();
    let descriptors = elements
        .iter()
        .map(|element| container_descriptor(element, stream_end))
        .collect::<Result<Vec<_>, _>>()?;
    for (element, descriptor) in elements.iter().zip(descriptors.iter().flatten()) {
        if !boundaries.contains(&descriptor.end) {
            return Err(DgnError::InvalidDescriptionBoundary {
                offset: element.raw.offset,
                element_type: element.raw.header.element_type,
                declared_end: descriptor.end,
            });
        }
    }

    let mut parents = vec![None; elements.len()];
    let mut children = vec![Vec::new(); elements.len()];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let offset = element.raw.offset;
        while stack.last().is_some_and(|(_, end)| *end == offset) {
            stack.pop();
        }
        if stack.last().is_some_and(|(_, end)| *end < offset) {
            let (parent, end) = stack.pop().expect("stack checked above");
            return Err(DgnError::InvalidDescriptionBoundary {
                offset: elements[parent].raw.offset,
                element_type: elements[parent].raw.header.element_type,
                declared_end: end,
            });
        }

        if let Some(&(parent, parent_end)) = stack.last() {
            if !element.raw.header.complex_component {
                return Err(DgnError::MissingComplexComponentFlag {
                    parent_offset: elements[parent].raw.offset,
                    parent_type: elements[parent].raw.header.element_type,
                    component_offset: offset,
                    component_type: element.raw.header.element_type,
                });
            }
            parents[index] = Some(parent);
            children[parent].push(index);
            if let Some(descriptor) = descriptors[index] {
                if descriptor.end > parent_end {
                    return Err(DgnError::InvalidDescriptionRange {
                        offset,
                        element_type: element.raw.header.element_type,
                        declared_end: descriptor.end as i64,
                        record_end: offset + element.raw.bytes.len(),
                        stream_end: parent_end,
                    });
                }
            }
        } else if element.raw.header.complex_component {
            return Err(DgnError::OrphanComplexComponent {
                offset,
                element_type: element.raw.header.element_type,
            });
        }

        if let Some(descriptor) = descriptors[index] {
            stack.push((index, descriptor.end));
        }
    }
    while let Some((parent, end)) = stack.pop() {
        if end != stream_end {
            return Err(DgnError::InvalidDescriptionBoundary {
                offset: elements[parent].raw.offset,
                element_type: elements[parent].raw.header.element_type,
                declared_end: end,
            });
        }
    }

    for (index, descriptor) in descriptors.iter().enumerate() {
        if let Some(ContainerDescriptor {
            kind: ContainerKind::Counted(declared),
            ..
        }) = descriptor
        {
            let actual = children[index].len();
            if *declared != actual {
                return Err(DgnError::ComplexElementCountMismatch {
                    offset: elements[index].raw.offset,
                    element_type: elements[index].raw.header.element_type,
                    declared: *declared,
                    actual,
                });
            }
        }
    }
    for (index, element) in elements.iter_mut().enumerate() {
        element.parent_index = parents[index];
        element.child_indices = std::mem::take(&mut children[index]);
    }
    Ok(())
}

fn container_descriptor(
    element: &Element2D<'_>,
    stream_end: usize,
) -> Result<Option<ContainerDescriptor>, DgnError> {
    let (base, words, kind) = match &element.data {
        ElementData2D::Cell(cell) => (
            38_i64,
            i64::from(cell.total_length_words),
            ContainerKind::Cell,
        ),
        ElementData2D::TextNode(node) => (
            38,
            i64::from(node.total_length_words),
            ContainerKind::Counted(usize::from(node.num_text_strings)),
        ),
        ElementData2D::ComplexChain(header) | ElementData2D::ComplexShape(header) => (
            38,
            i64::from(header.total_length_words),
            ContainerKind::Counted(usize::from(header.num_elements)),
        ),
        ElementData2D::BSplineCurve(curve) => (
            40,
            i64::from(curve.description_words),
            ContainerKind::BSplineCurve,
        ),
        ElementData2D::BSplineSurface(surface) => (
            40,
            i64::from(surface.description_words),
            ContainerKind::BSplineSurface,
        ),
        _ => return Ok(None),
    };
    let record_end = element.raw.offset + element.raw.bytes.len();
    let calculated = i64::try_from(element.raw.offset).ok().and_then(|offset| {
        words
            .checked_mul(2)
            .and_then(|length| offset.checked_add(base + length))
    });
    let Some(declared_end) = calculated else {
        return Err(DgnError::InvalidDescriptionRange {
            offset: element.raw.offset,
            element_type: element.raw.header.element_type,
            declared_end: i64::MAX,
            record_end,
            stream_end,
        });
    };
    if words < 0
        || declared_end < i64::try_from(record_end).unwrap_or(i64::MAX)
        || declared_end > i64::try_from(stream_end).unwrap_or(i64::MAX)
    {
        return Err(DgnError::InvalidDescriptionRange {
            offset: element.raw.offset,
            element_type: element.raw.header.element_type,
            declared_end,
            record_end,
            stream_end,
        });
    }
    Ok(Some(ContainerDescriptor {
        end: declared_end as usize,
        kind,
    }))
}

fn validate_bspline_groups(elements: &[Element2D<'_>]) -> Result<(), DgnError> {
    for element in elements {
        match element.data {
            ElementData2D::BSplineCurve(curve) => validate_bspline_curve(element, curve, elements)?,
            ElementData2D::BSplineSurface(surface) => {
                validate_bspline_surface(element, surface, elements)?
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_bspline_curve(
    element: &Element2D<'_>,
    curve: BSplineCurve2D,
    elements: &[Element2D<'_>],
) -> Result<(), DgnError> {
    let fail = |context| DgnError::InvalidBSplineComponents {
        offset: element.raw.offset,
        element_type: element.raw.header.element_type,
        context,
    };
    if curve.num_poles == 0 || curve.num_poles > 101 {
        return Err(fail("curve pole count must be between 1 and 101"));
    }
    let Some(expected_knots) =
        expected_non_uniform_knots(curve.num_poles, curve.order, curve.is_closed())
    else {
        return Err(fail("curve order exceeds its pole count"));
    };
    if curve.num_knots > 0 && usize::from(curve.num_knots) != expected_knots {
        return Err(fail(
            "non-uniform curve knot count is inconsistent with poles/order/closure",
        ));
    }
    let mut position = 0;
    if curve.num_knots > 0 {
        let Some(child) = child_at(element, elements, position) else {
            return Err(fail("missing knot element"));
        };
        let ElementData2D::BSplineKnot(knots) = &child.data else {
            return Err(fail("knot element must immediately follow the header"));
        };
        if knots.values.len() != usize::from(curve.num_knots) {
            return Err(fail("knot count does not match the curve header"));
        }
        position += 1;
    }
    let Some(child) = child_at(element, elements, position) else {
        return Err(fail("missing pole element"));
    };
    let ElementData2D::BSplinePole(poles) = &child.data else {
        return Err(fail("pole element is out of order"));
    };
    if poles.vertices_uor.len() != usize::from(curve.num_poles) {
        return Err(fail("pole count does not match the curve header"));
    }
    position += 1;
    if curve.is_rational() {
        let Some(child) = child_at(element, elements, position) else {
            return Err(fail("missing rational weight element"));
        };
        let ElementData2D::BSplineWeight(weights) = &child.data else {
            return Err(fail("weight element must immediately follow poles"));
        };
        if weights.values.len() != usize::from(curve.num_poles) {
            return Err(fail("weight count does not match the curve poles"));
        }
        position += 1;
    }
    if position != element.child_indices.len() {
        return Err(fail("unexpected component after the curve definition"));
    }
    Ok(())
}

fn validate_bspline_surface(
    element: &Element2D<'_>,
    surface: BSplineSurface2D,
    elements: &[Element2D<'_>],
) -> Result<(), DgnError> {
    let fail = |context| DgnError::InvalidBSplineComponents {
        offset: element.raw.offset,
        element_type: element.raw.header.element_type,
        context,
    };
    if surface.num_poles_u == 0 || surface.num_poles_u > 101 || surface.num_poles_v == 0 {
        return Err(fail(
            "surface pole counts must be non-zero and U must not exceed 101",
        ));
    }
    let Some(expected_knots_u) =
        expected_non_uniform_knots(surface.num_poles_u, surface.u_order, surface.is_u_closed())
    else {
        return Err(fail("surface U order exceeds its pole count"));
    };
    let Some(expected_knots_v) =
        expected_non_uniform_knots(surface.num_poles_v, surface.v_order, surface.is_v_closed())
    else {
        return Err(fail("surface V order exceeds its pole count"));
    };
    if (surface.num_knots_u > 0 && usize::from(surface.num_knots_u) != expected_knots_u)
        || (surface.num_knots_v > 0 && usize::from(surface.num_knots_v) != expected_knots_v)
    {
        return Err(fail(
            "non-uniform surface knot counts are inconsistent with poles/orders/closure",
        ));
    }
    let mut position = 0;
    let knot_count = usize::from(surface.num_knots_u) + usize::from(surface.num_knots_v);
    if knot_count > 0 {
        let Some(child) = child_at(element, elements, position) else {
            return Err(fail("missing surface knot element"));
        };
        let ElementData2D::BSplineKnot(knots) = &child.data else {
            return Err(fail("surface knot element is out of order"));
        };
        if knots.values.len() != knot_count {
            return Err(fail("surface knot count does not match the header"));
        }
        position += 1;
    }

    let mut boundary_numbers = HashSet::new();
    while let Some(child) = child_at(element, elements, position) {
        let ElementData2D::BSplineSurfaceBoundary(boundary) = &child.data else {
            break;
        };
        boundary_numbers.insert(boundary.number);
        position += 1;
    }
    if boundary_numbers.len() != usize::from(surface.num_bounds) {
        return Err(fail(
            "logical boundary count does not match the surface header",
        ));
    }

    for _ in 0..surface.num_poles_v {
        let Some(child) = child_at(element, elements, position) else {
            return Err(fail("missing surface pole row"));
        };
        let ElementData2D::BSplinePole(poles) = &child.data else {
            return Err(fail("surface pole row is out of order"));
        };
        if poles.vertices_uor.len() != usize::from(surface.num_poles_u) {
            return Err(fail("surface pole-row width does not match the header"));
        }
        position += 1;
        if surface.is_rational() {
            let Some(child) = child_at(element, elements, position) else {
                return Err(fail("missing surface weight row"));
            };
            let ElementData2D::BSplineWeight(weights) = &child.data else {
                return Err(fail("surface weight row must immediately follow its poles"));
            };
            if weights.values.len() != usize::from(surface.num_poles_u) {
                return Err(fail("surface weight-row width does not match its poles"));
            }
            position += 1;
        }
    }
    if position != element.child_indices.len() {
        return Err(fail("unexpected component after the surface definition"));
    }
    Ok(())
}

fn child_at<'a>(
    element: &Element2D<'_>,
    elements: &'a [Element2D<'_>],
    position: usize,
) -> Option<&'a Element2D<'a>> {
    elements.get(*element.child_indices.get(position)?)
}

fn expected_non_uniform_knots(poles: u16, order: u8, closed: bool) -> Option<usize> {
    let poles = usize::from(poles);
    let order = usize::from(order);
    if poles < order {
        return None;
    }
    Some(if closed { poles - 1 } else { poles - order })
}

fn precise_points(points: &[Point2<i32>], linkages: &[AttributeLinkage<'_>]) -> Vec<Point2<f64>> {
    let deltas = linkages.iter().find_map(|linkage| match &linkage.data {
        LinkageData::HighPrecision {
            deltas,
            complete: true,
            ..
        } => Some(deltas.as_slice()),
        _ => None,
    });
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let delta = deltas.and_then(|items| items.get(index));
            Point2 {
                x: f64::from(point.x)
                    + delta.map_or(0.0, |delta| f64::from(delta.x) / SUB_UOR_DIVISOR),
                y: f64::from(point.y)
                    + delta.map_or(0.0, |delta| f64::from(delta.y) / SUB_UOR_DIVISOR),
            }
        })
        .collect()
}

fn semantic_data_end(record: RawElementRef<'_>, header: Option<CommonElementHeader>) -> usize {
    header
        .and_then(|header| header.attribute_offset)
        .unwrap_or(record.bytes.len())
}

fn require_data_size(
    record: RawElementRef<'_>,
    header: Option<CommonElementHeader>,
    needed: usize,
    context: &'static str,
) -> Result<(), DgnError> {
    let actual = semantic_data_end(record, header);
    if actual < needed {
        return Err(DgnError::ElementTooShort {
            offset: record.offset,
            element_type: record.header.element_type,
            needed,
            actual,
            context,
        });
    }
    Ok(())
}

fn checked_variable_end(
    record: RawElementRef<'_>,
    start: usize,
    count: usize,
    stride: usize,
    context: &'static str,
) -> Result<usize, DgnError> {
    start
        .checked_add(count.checked_mul(stride).ok_or(DgnError::ElementTooShort {
            offset: record.offset,
            element_type: record.header.element_type,
            needed: usize::MAX,
            actual: record.bytes.len(),
            context,
        })?)
        .ok_or(DgnError::ElementTooShort {
            offset: record.offset,
            element_type: record.header.element_type,
            needed: usize::MAX,
            actual: record.bytes.len(),
            context,
        })
}

fn transform_integer_point(settings: DesignSettings, point: Point2<i32>) -> Option<Point2<f64>> {
    transform_fractional_point(
        settings,
        Point2 {
            x: f64::from(point.x),
            y: f64::from(point.y),
        },
    )
}

fn transform_fractional_point(settings: DesignSettings, point: Point2<f64>) -> Option<Point2<f64>> {
    settings
        .transform_xy([point.x, point.y])
        .map(|point| Point2 {
            x: point[0],
            y: point[1],
        })
}

fn angle_degrees(raw: i32) -> f64 {
    f64::from(raw) / ANGLE_UNITS_PER_DEGREE
}

fn decode_radix50_name(words: [u16; 2]) -> String {
    let mut bytes = Vec::with_capacity(6);
    for mut word in words {
        for divisor in [1600_u16, 40, 1] {
            let value = word / divisor;
            bytes.push(match value {
                0 | 29 => b' ',
                1..=26 => b'A' + value as u8 - 1,
                27 => b'$',
                28 => b'.',
                30..=39 => b'0' + value as u8 - 30,
                _ => b' ',
            });
            word -= value * divisor;
        }
    }
    while bytes.last() == Some(&b' ') {
        bytes.pop();
    }
    String::from_utf8(bytes).expect("RADIX-50 maps only to ASCII")
}

fn read_point_i32(bytes: &[u8], offset: usize) -> Point2<i32> {
    Point2 {
        x: decode_middle_endian_i32(read_four(bytes, offset)),
        y: decode_middle_endian_i32(read_four(bytes, offset + 4)),
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RawElementHeader;

    fn settings() -> DesignSettings {
        DesignSettings {
            dimension: V7Dimension::Two,
            subunits_per_master: 10,
            uor_per_subunit: 10,
            master_unit_label: *b"mu",
            sub_unit_label: *b"su",
            global_origin_uor: [0.0; 3],
        }
    }

    fn record(bytes: &[u8], element_type: u8, level: u8) -> RawElementRef<'_> {
        RawElementRef {
            index: 4,
            offset: 512,
            header: RawElementHeader {
                level,
                element_type,
                complex_component: false,
                reserved: false,
                deleted: false,
                words_to_follow: (bytes.len() / 2 - 2) as u16,
            },
            bytes,
        }
    }

    fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
        let value = value as u32;
        bytes[offset..offset + 4].copy_from_slice(&[
            (value >> 16) as u8,
            (value >> 24) as u8,
            value as u8,
            (value >> 8) as u8,
        ]);
    }

    #[test]
    fn decodes_vertices_without_normalizing_them() {
        let mut bytes = [0_u8; 62];
        bytes[36..38].copy_from_slice(&3_u16.to_le_bytes());
        for (index, (x, y)) in [(100, -200), (300, 400), (100, -200)]
            .into_iter()
            .enumerate()
        {
            put_i32(&mut bytes, 38 + index * 8, x);
            put_i32(&mut bytes, 42 + index * 8, y);
        }
        let data =
            decode_element_data(record(&bytes, LINE_STRING, 2), None, &[], settings()).unwrap();
        let ElementData2D::LineString(line_string) = data else {
            panic!("expected line string");
        };
        assert_eq!(
            line_string.vertices_uor,
            [
                Point2 { x: 100, y: -200 },
                Point2 { x: 300, y: 400 },
                Point2 { x: 100, y: -200 },
            ]
        );
        assert_eq!(
            line_string.vertices_uor_precise[1],
            Point2 { x: 300.0, y: 400.0 }
        );
        assert_eq!(
            line_string.vertices_master.unwrap()[1],
            Point2 { x: 3.0, y: 4.0 }
        );
    }

    #[test]
    fn decodes_high_precision_deltas_without_overwriting_integer_storage() {
        let points = [Point2 { x: 10, y: 20 }, Point2 { x: -30, y: 40 }];
        let raw = [
            7, 0x10, 0xa9, 0x51, 4, 0, 0, 0, 0xff, 0xff, 2, 0, 3, 0, 0xfc, 0xff,
        ];
        let links = [AttributeLinkage {
            offset: 0,
            linkage_type: Some(0x51a9),
            declared_size: Some(16),
            raw: &raw,
            data: LinkageData::HighPrecision {
                delta_words: 4,
                deltas: vec![
                    crate::PrecisionDelta { x: -1, y: 2 },
                    crate::PrecisionDelta { x: 3, y: -4 },
                ],
                complete: true,
            },
        }];
        let precise = precise_points(&points, &links);
        assert_eq!(points[0], Point2 { x: 10, y: 20 });
        assert!((precise[0].x - (10.0 - 1.0 / 32767.0)).abs() < 1e-12);
        assert!((precise[1].y - (40.0 - 4.0 / 32767.0)).abs() < 1e-12);
    }

    #[test]
    fn decodes_arc_sign_magnitude_and_radix50() {
        let mut bytes = [0_u8; 80];
        put_i32(&mut bytes, 36, 45 * 360_000);
        put_i32(
            &mut bytes,
            40,
            (0x8000_0000_u32 | (90 * 360_000_u32)) as i32,
        );
        bytes[44..52].copy_from_slice(&[0x80, 0x40, 0, 0, 0, 0, 0, 0]);
        bytes[52..60].copy_from_slice(&[0x00, 0x41, 0, 0, 0, 0, 0, 0]);
        let ElementData2D::Arc(arc) =
            decode_element_data(record(&bytes, ARC, 2), None, &[], settings()).unwrap()
        else {
            panic!("expected arc");
        };
        assert_eq!(arc.sweep_angle_degrees, -90.0);
        assert_eq!(decode_radix50_name([1683, 50_913]), "ABC123");
    }

    #[test]
    fn rejects_impossible_variable_lengths() {
        let mut vertices = [0_u8; 38];
        vertices[36..38].copy_from_slice(&1_u16.to_le_bytes());
        assert!(matches!(
            decode_element_data(record(&vertices, SHAPE, 2), None, &[], settings()),
            Err(DgnError::InvalidVertexCount {
                count: 1,
                minimum: 2,
                ..
            })
        ));

        let scalar = [0_u8; 42];
        assert!(matches!(
            decode_element_data(record(&scalar, BSPLINE_KNOT, 2), None, &[], settings()),
            Err(DgnError::InvalidScalarArrayLength { data_bytes: 6, .. })
        ));
    }
}
