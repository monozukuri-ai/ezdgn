//! PyO3 bindings for `ezdgn-core`.

use ezdgn_core::{
    decode_common_header, decode_design_settings, detect_format, inspect_v8_container, read_v7_2d,
    scan_records, write_v7_2d, CommonElementHeader, DesignSettings, DgnError as CoreDgnError,
    ElementData2D, LinkageData, MasterPoint, Point2, RawPoint, RecordScan, ScanOptions,
    V7Document2D, V7ElementStyle, V7WriteOptions, WritableElement2D,
};
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

create_exception!(
    _core,
    DgnError,
    PyException,
    "Base exception raised by ezdgn."
);
create_exception!(
    _core,
    InvalidDgnError,
    DgnError,
    "The input is not a structurally valid supported DGN stream."
);
create_exception!(
    _core,
    UnsupportedDgnError,
    DgnError,
    "The DGN family was recognized but is not supported by this reader."
);
create_exception!(
    _core,
    DgnLimitError,
    DgnError,
    "A configured parser resource limit was exceeded."
);

type FormatRow = (String, Option<u8>);
type RecordRow = (usize, usize, u8, u8, bool, bool, bool, u16, usize);
type ScanRow = (FormatRow, Vec<RecordRow>, String, usize, usize, usize);
type RawPointRow = (i32, i32, Option<i32>);
type MasterPointRow = (f64, f64, Option<f64>);
type SettingsRow = (
    u8,
    u32,
    u32,
    (u8, u8),
    (u8, u8),
    (f64, f64, f64),
    u64,
    Option<f64>,
    Option<(f64, f64, f64)>,
);
type PropertiesRow = (u16, u8, u8, bool, bool, bool, bool, bool, bool, bool, bool);
type SymbologyRow = (u16, u8, u8, u8);
type CommonHeaderRow = (
    RawPointRow,
    RawPointRow,
    Option<MasterPointRow>,
    Option<MasterPointRow>,
    u16,
    u16,
    PropertiesRow,
    SymbologyRow,
    Option<usize>,
    usize,
);
type HeaderScanRow = (ScanRow, SettingsRow, Vec<Option<CommonHeaderRow>>);
type PointI32Row = (i32, i32);
type PointF64Row = (f64, f64);
type LineRow = (
    usize,
    PointI32Row,
    PointI32Row,
    (PointF64Row, PointF64Row),
    Option<(PointF64Row, PointF64Row)>,
);
type MultiPointRow = (
    usize,
    Vec<PointI32Row>,
    Vec<PointF64Row>,
    Option<Vec<PointF64Row>>,
);
type EllipseRow = (
    usize,
    PointF64Row,
    Option<PointF64Row>,
    (f64, f64),
    Option<(f64, f64)>,
    (i32, f64),
);
type ArcRow = (
    usize,
    PointF64Row,
    Option<PointF64Row>,
    (f64, f64),
    Option<(f64, f64)>,
    (i32, f64),
    (i32, f64),
    (i32, f64),
);
type TextRow = (
    usize,
    (u8, u8),
    (i32, i32),
    Option<(f64, f64)>,
    (i32, f64),
    PointI32Row,
    Option<PointF64Row>,
    (usize, usize, u8),
);
type ColorTableRow = (usize, u16, Vec<(u8, u8, u8)>);
type CellRow = (
    usize,
    (u16, (u16, u16), String),
    (u16, (u16, u16, u16, u16)),
    (PointI32Row, PointI32Row),
    Option<(PointF64Row, PointF64Row)>,
    ((i32, i32), (i32, i32)),
    ((f64, f64), (f64, f64)),
    PointI32Row,
    Option<PointF64Row>,
);
type TextNodeRow = (
    usize,
    (u16, u16, u16),
    (u8, u8, u8, u8),
    (i32, Option<f64>),
    (i32, i32),
    Option<(f64, f64)>,
    (i32, f64),
    PointI32Row,
    Option<PointF64Row>,
);
type ComplexRow = (usize, u16, u16);
type BSplineSurfaceRow = (
    usize,
    (i32, u8),
    (u8, u8, u16, u16, u16),
    (u8, u8, u16, u16, u16),
    u16,
);
type BSplineBoundaryRow = (
    usize,
    u16,
    Vec<PointI32Row>,
    Vec<PointF64Row>,
    Vec<PointF64Row>,
);
type BSplineScalarRow = (usize, Vec<i32>, Vec<f64>);
type BSplineCurveRow = (usize, i32, u8, u8, u8, u16, u16);
type BSplineRows = (
    Vec<MultiPointRow>,
    Vec<BSplineSurfaceRow>,
    Vec<BSplineBoundaryRow>,
    Vec<BSplineScalarRow>,
    Vec<BSplineCurveRow>,
    Vec<BSplineScalarRow>,
);
type HierarchyRow = (Option<usize>, Vec<usize>);
type HighPrecisionRow = (u16, Vec<(i16, i16)>, bool);
type LinkageRow = (
    usize,
    usize,
    Option<usize>,
    Option<u16>,
    String,
    Option<u16>,
    Option<u32>,
    Option<u8>,
    Option<u32>,
    Option<HighPrecisionRow>,
);
type Phase4Row = (
    Vec<MultiPointRow>,
    Vec<CellRow>,
    Vec<TextNodeRow>,
    Vec<ComplexRow>,
    Vec<ComplexRow>,
    BSplineRows,
    Vec<HierarchyRow>,
    Vec<Vec<LinkageRow>>,
);
type PrimitiveScanRow = (
    HeaderScanRow,
    Vec<LineRow>,
    Vec<MultiPointRow>,
    Vec<MultiPointRow>,
    Vec<EllipseRow>,
    Vec<ArcRow>,
    Vec<TextRow>,
    Vec<ColorTableRow>,
    Option<usize>,
    Phase4Row,
);
type WriteStyleRow = (u8, u8, u8, u8, u16, u16);
type WriteEntityRow = (
    String,
    Vec<PointF64Row>,
    Vec<f64>,
    Vec<u8>,
    (u8, u8),
    WriteStyleRow,
    Option<u8>,
);
type V8CfbEntryRow = (String, String, Option<u64>);
type V8ContainerRow = (u16, bool, Vec<String>, Vec<String>, Vec<V8CfbEntryRow>);

#[pyfunction]
fn core_version() -> String {
    ezdgn_core::version().to_owned()
}

#[pyfunction]
fn detect_format_bytes(data: &[u8]) -> PyResult<FormatRow> {
    detect_format(data)
        .map(format_row)
        .map_err(core_error_to_python)
}

#[pyfunction]
fn inspect_v8_cfb(data: &[u8], max_entries: usize) -> PyResult<V8ContainerRow> {
    let info = inspect_v8_container(data, max_entries).map_err(core_error_to_python)?;
    let entries = info
        .entries
        .into_iter()
        .map(|entry| (entry.path, entry.kind.as_str().to_owned(), entry.size_bytes))
        .collect();
    Ok((
        info.cfb_version,
        info.has_dgn_v8_markers,
        info.missing_markers,
        info.model_storage_paths,
        entries,
    ))
}

#[pyfunction]
fn scan_v7_records(
    data: &[u8],
    max_file_size: usize,
    max_records: usize,
    max_record_size: usize,
) -> PyResult<ScanRow> {
    let scan = scan_with_options(data, max_file_size, max_records, max_record_size)?;
    Ok(scan_row(&scan))
}

#[pyfunction]
fn read_v7_design_settings(
    data: &[u8],
    max_file_size: usize,
    max_records: usize,
    max_record_size: usize,
) -> PyResult<SettingsRow> {
    let scan = scan_with_options(data, max_file_size, max_records, max_record_size)?;
    decode_design_settings(&scan)
        .map(settings_row)
        .map_err(core_error_to_python)
}

#[pyfunction]
fn inspect_v7_headers(
    data: &[u8],
    max_file_size: usize,
    max_records: usize,
    max_record_size: usize,
) -> PyResult<HeaderScanRow> {
    let scan = scan_with_options(data, max_file_size, max_records, max_record_size)?;
    let settings = decode_design_settings(&scan).map_err(core_error_to_python)?;
    let common_headers = scan
        .records
        .iter()
        .copied()
        .map(|record| {
            decode_common_header(record, settings.dimension)
                .map(|header| header.map(|header| common_header_row(header, settings)))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(core_error_to_python)?;

    Ok((scan_row(&scan), settings_row(settings), common_headers))
}

#[pyfunction]
fn read_v7_2d_primitives(
    data: &[u8],
    max_file_size: usize,
    max_records: usize,
    max_record_size: usize,
) -> PyResult<PrimitiveScanRow> {
    let document = read_v7_2d(
        data,
        ScanOptions {
            max_file_size,
            max_records,
            max_record_size,
        },
    )
    .map_err(core_error_to_python)?;
    Ok(primitive_scan_row(&document))
}

#[pyfunction]
fn write_v7_2d_bytes(
    py: Python<'_>,
    seed: &[u8],
    entities: Vec<WriteEntityRow>,
    copy_color_table: bool,
    copy_seed_elements: bool,
) -> PyResult<Py<PyBytes>> {
    let elements = entities
        .into_iter()
        .map(write_element_from_row)
        .collect::<PyResult<Vec<_>>>()?;
    let output = write_v7_2d(
        seed,
        &elements,
        V7WriteOptions {
            copy_color_table,
            copy_seed_elements,
        },
    )
    .map_err(core_error_to_python)?;
    Ok(PyBytes::new(py, &output).unbind())
}

fn write_element_from_row(row: WriteEntityRow) -> PyResult<WritableElement2D> {
    let (kind, points, parameters, text, text_style, style, fill_color) = row;
    let normalized = kind.trim().to_ascii_uppercase().replace(['-', ' '], "_");
    let style = V7ElementStyle {
        level: style.0,
        color: style.1,
        line_style: style.2,
        line_weight: style.3,
        graphic_group: style.4,
        properties: style.5,
    };
    let points = points
        .into_iter()
        .map(|(x, y)| Point2 { x, y })
        .collect::<Vec<_>>();

    match normalized.as_str() {
        "LINE" => {
            require_writer_row(
                &normalized,
                points.len() == 2,
                "expected exactly two points",
            )?;
            require_writer_row(&normalized, parameters.is_empty(), "unexpected parameters")?;
            Ok(WritableElement2D::Line {
                start: points[0],
                end: points[1],
                style,
            })
        }
        "LINE_STRING" => {
            require_writer_row(&normalized, parameters.is_empty(), "unexpected parameters")?;
            Ok(WritableElement2D::LineString {
                vertices: points,
                style,
            })
        }
        "SHAPE" => {
            require_writer_row(&normalized, parameters.is_empty(), "unexpected parameters")?;
            Ok(WritableElement2D::Shape {
                vertices: points,
                fill_color,
                style,
            })
        }
        "CURVE" => {
            require_writer_row(&normalized, parameters.is_empty(), "unexpected parameters")?;
            Ok(WritableElement2D::Curve {
                vertices: points,
                style,
            })
        }
        "ELLIPSE" => {
            require_writer_row(&normalized, points.len() == 1, "expected one center point")?;
            require_writer_row(
                &normalized,
                parameters.len() == 3,
                "expected primary axis, secondary axis, and rotation",
            )?;
            Ok(WritableElement2D::Ellipse {
                center: points[0],
                primary_axis: parameters[0],
                secondary_axis: parameters[1],
                rotation_degrees: parameters[2],
                style,
            })
        }
        "ARC" => {
            require_writer_row(&normalized, points.len() == 1, "expected one center point")?;
            require_writer_row(
                &normalized,
                parameters.len() == 5,
                "expected primary axis, secondary axis, rotation, start, and sweep",
            )?;
            Ok(WritableElement2D::Arc {
                center: points[0],
                primary_axis: parameters[0],
                secondary_axis: parameters[1],
                rotation_degrees: parameters[2],
                start_angle_degrees: parameters[3],
                sweep_angle_degrees: parameters[4],
                style,
            })
        }
        "TEXT" => {
            require_writer_row(
                &normalized,
                points.len() == 1,
                "expected one insertion point",
            )?;
            require_writer_row(
                &normalized,
                parameters.len() == 3,
                "expected length multiplier, height multiplier, and rotation",
            )?;
            Ok(WritableElement2D::Text {
                origin: points[0],
                text,
                font_id: text_style.0,
                justification: text_style.1,
                length_multiplier: parameters[0],
                height_multiplier: parameters[1],
                rotation_degrees: parameters[2],
                style,
            })
        }
        _ => Err(InvalidDgnError::new_err(format!(
            "unsupported V7 writer entity kind: {kind}"
        ))),
    }
}

fn require_writer_row(kind: &str, condition: bool, context: &str) -> PyResult<()> {
    if condition {
        Ok(())
    } else {
        Err(InvalidDgnError::new_err(format!(
            "invalid {kind} writer row: {context}"
        )))
    }
}

fn scan_with_options(
    data: &[u8],
    max_file_size: usize,
    max_records: usize,
    max_record_size: usize,
) -> PyResult<RecordScan<'_>> {
    scan_records(
        data,
        ScanOptions {
            max_file_size,
            max_records,
            max_record_size,
        },
    )
    .map_err(core_error_to_python)
}

fn scan_row(scan: &RecordScan<'_>) -> ScanRow {
    let records = scan
        .records
        .iter()
        .map(|record| {
            (
                record.index,
                record.offset,
                record.header.level,
                record.header.element_type,
                record.header.complex_component,
                record.header.reserved,
                record.header.deleted,
                record.header.words_to_follow,
                record.bytes.len(),
            )
        })
        .collect();
    (
        format_row(scan.format),
        records,
        scan.termination.kind().to_owned(),
        scan.termination.offset(),
        scan.termination.trailing_bytes(),
        scan.source_size,
    )
}

fn settings_row(settings: DesignSettings) -> SettingsRow {
    let origin = settings.global_origin_uor;
    (
        settings.dimension.as_u8(),
        settings.subunits_per_master,
        settings.uor_per_subunit,
        (settings.master_unit_label[0], settings.master_unit_label[1]),
        (settings.sub_unit_label[0], settings.sub_unit_label[1]),
        (origin[0], origin[1], origin[2]),
        settings.uor_per_master(),
        settings.scale(),
        settings
            .global_origin_master()
            .map(|origin| (origin[0], origin[1], origin[2])),
    )
}

fn common_header_row(header: CommonElementHeader, settings: DesignSettings) -> CommonHeaderRow {
    let master = header.range.to_master(settings);
    let properties = header.properties;
    let symbology = header.symbology;
    (
        raw_point_row(header.range.low),
        raw_point_row(header.range.high),
        master.map(|range| master_point_row(range.low)),
        master.map(|range| master_point_row(range.high)),
        header.graphic_group,
        header.attribute_index,
        (
            properties.raw,
            properties.class,
            properties.reserved,
            properties.locked,
            properties.new,
            properties.modified,
            properties.has_attributes,
            properties.screen_relative,
            properties.non_planar,
            properties.not_snappable,
            properties.h_bit,
        ),
        (
            symbology.raw,
            symbology.style,
            symbology.weight,
            symbology.color,
        ),
        header.attribute_offset,
        header.attribute_length,
    )
}

fn primitive_scan_row(document: &V7Document2D<'_>) -> PrimitiveScanRow {
    let mut lines = Vec::new();
    let mut line_strings = Vec::new();
    let mut shapes = Vec::new();
    let mut curves = Vec::new();
    let mut cells = Vec::new();
    let mut text_nodes = Vec::new();
    let mut complex_chains = Vec::new();
    let mut complex_shapes = Vec::new();
    let mut ellipses = Vec::new();
    let mut arcs = Vec::new();
    let mut texts = Vec::new();
    let mut bspline_poles = Vec::new();
    let mut bspline_surfaces = Vec::new();
    let mut bspline_boundaries = Vec::new();
    let mut bspline_knots = Vec::new();
    let mut bspline_curves = Vec::new();
    let mut bspline_weights = Vec::new();
    let mut color_tables = Vec::new();

    for element in &document.elements {
        let index = element.raw.index;
        match &element.data {
            ElementData2D::Cell(cell) => cells.push((
                index,
                (
                    cell.total_length_words,
                    (cell.name_words[0], cell.name_words[1]),
                    cell.name.clone(),
                ),
                (
                    cell.class,
                    (
                        cell.levels[0],
                        cell.levels[1],
                        cell.levels[2],
                        cell.levels[3],
                    ),
                ),
                (
                    point_i32_row(cell.range_low_uor),
                    point_i32_row(cell.range_high_uor),
                ),
                pair_points(cell.range_low_master, cell.range_high_master),
                (
                    (cell.transform_raw[0][0], cell.transform_raw[0][1]),
                    (cell.transform_raw[1][0], cell.transform_raw[1][1]),
                ),
                (
                    (cell.transform[0][0], cell.transform[0][1]),
                    (cell.transform[1][0], cell.transform[1][1]),
                ),
                point_i32_row(cell.origin_uor),
                cell.origin_master.map(point_f64_row),
            )),
            ElementData2D::Line(line) => lines.push((
                index,
                point_i32_row(line.start_uor),
                point_i32_row(line.end_uor),
                (
                    point_f64_row(line.start_uor_precise),
                    point_f64_row(line.end_uor_precise),
                ),
                pair_points(line.start_master, line.end_master),
            )),
            ElementData2D::LineString(line_string) => line_strings.push(multipoint_row(
                index,
                &line_string.vertices_uor,
                &line_string.vertices_uor_precise,
                line_string.vertices_master.as_deref(),
            )),
            ElementData2D::Shape(shape) => shapes.push(multipoint_row(
                index,
                &shape.vertices_uor,
                &shape.vertices_uor_precise,
                shape.vertices_master.as_deref(),
            )),
            ElementData2D::TextNode(node) => text_nodes.push((
                index,
                (
                    node.total_length_words,
                    node.num_text_strings,
                    node.node_number,
                ),
                (
                    node.max_length,
                    node.max_used,
                    node.font_id,
                    node.justification,
                ),
                (node.line_spacing_raw, node.line_spacing_master),
                (node.length_multiplier_raw, node.height_multiplier_raw),
                pair_values(node.length_multiplier_master, node.height_multiplier_master),
                (node.rotation_raw, node.rotation_degrees),
                point_i32_row(node.origin_uor),
                node.origin_master.map(point_f64_row),
            )),
            ElementData2D::Curve(curve) => curves.push(multipoint_row(
                index,
                &curve.vertices_uor,
                &curve.vertices_uor_precise,
                curve.vertices_master.as_deref(),
            )),
            ElementData2D::ComplexChain(header) => {
                complex_chains.push((index, header.total_length_words, header.num_elements))
            }
            ElementData2D::ComplexShape(header) => {
                complex_shapes.push((index, header.total_length_words, header.num_elements))
            }
            ElementData2D::Ellipse(ellipse) => ellipses.push((
                index,
                point_f64_row(ellipse.center_uor),
                ellipse.center_master.map(point_f64_row),
                (ellipse.primary_axis_uor, ellipse.secondary_axis_uor),
                pair_values(ellipse.primary_axis_master, ellipse.secondary_axis_master),
                (ellipse.rotation_raw, ellipse.rotation_degrees),
            )),
            ElementData2D::Arc(arc) => arcs.push((
                index,
                point_f64_row(arc.center_uor),
                arc.center_master.map(point_f64_row),
                (arc.primary_axis_uor, arc.secondary_axis_uor),
                pair_values(arc.primary_axis_master, arc.secondary_axis_master),
                (arc.rotation_raw, arc.rotation_degrees),
                (arc.start_angle_raw, arc.start_angle_degrees),
                (arc.sweep_angle_raw, arc.sweep_angle_degrees),
            )),
            ElementData2D::Text(text) => texts.push((
                index,
                (text.font_id, text.justification),
                (text.length_multiplier_raw, text.height_multiplier_raw),
                pair_values(text.length_multiplier_master, text.height_multiplier_master),
                (text.rotation_raw, text.rotation_degrees),
                point_i32_row(text.origin_uor),
                text.origin_master.map(point_f64_row),
                (
                    text.text_offset,
                    text.text_bytes.len(),
                    text.editable_fields,
                ),
            )),
            ElementData2D::BSplinePole(poles) => bspline_poles.push(multipoint_row(
                index,
                &poles.vertices_uor,
                &poles.vertices_uor_precise,
                poles.vertices_master.as_deref(),
            )),
            ElementData2D::BSplineSurface(surface) => bspline_surfaces.push((
                index,
                (surface.description_words, surface.curve_type),
                (
                    surface.u_order,
                    surface.u_properties,
                    surface.num_poles_u,
                    surface.num_knots_u,
                    surface.rule_lines_u,
                ),
                (
                    surface.v_order,
                    surface.v_properties,
                    surface.num_poles_v,
                    surface.num_knots_v,
                    surface.rule_lines_v,
                ),
                surface.num_bounds,
            )),
            ElementData2D::BSplineSurfaceBoundary(boundary) => {
                bspline_boundaries.push((
                    index,
                    boundary.number,
                    boundary
                        .vertices_raw
                        .iter()
                        .copied()
                        .map(point_i32_row)
                        .collect(),
                    boundary
                        .vertices_raw_precise
                        .iter()
                        .copied()
                        .map(point_f64_row)
                        .collect(),
                    boundary
                        .vertices_uv
                        .iter()
                        .copied()
                        .map(point_f64_row)
                        .collect(),
                ));
            }
            ElementData2D::BSplineKnot(knots) => {
                bspline_knots.push((index, knots.values_raw.clone(), knots.values.clone()))
            }
            ElementData2D::BSplineCurve(curve) => bspline_curves.push((
                index,
                curve.description_words,
                curve.order,
                curve.properties,
                curve.curve_type,
                curve.num_poles,
                curve.num_knots,
            )),
            ElementData2D::BSplineWeight(weights) => {
                bspline_weights.push((index, weights.values_raw.clone(), weights.values.clone()))
            }
            ElementData2D::ColorTable(table) => color_tables.push((
                index,
                table.screen_flag,
                table
                    .colors
                    .iter()
                    .map(|color| (color[0], color[1], color[2]))
                    .collect(),
            )),
            ElementData2D::Unsupported => {}
        }
    }

    let headers = document
        .elements
        .iter()
        .map(|element| {
            element
                .common_header
                .map(|header| common_header_row(header, document.settings))
        })
        .collect();
    (
        (
            scan_row(&document.scan),
            settings_row(document.settings),
            headers,
        ),
        lines,
        line_strings,
        shapes,
        ellipses,
        arcs,
        texts,
        color_tables,
        document.active_color_table,
        (
            curves,
            cells,
            text_nodes,
            complex_chains,
            complex_shapes,
            (
                bspline_poles,
                bspline_surfaces,
                bspline_boundaries,
                bspline_knots,
                bspline_curves,
                bspline_weights,
            ),
            document
                .elements
                .iter()
                .map(|element| (element.parent_index, element.child_indices.clone()))
                .collect(),
            document
                .elements
                .iter()
                .map(|element| element.linkages.iter().map(linkage_row).collect())
                .collect(),
        ),
    )
}

fn multipoint_row(
    index: usize,
    raw: &[Point2<i32>],
    precise: &[Point2<f64>],
    master: Option<&[Point2<f64>]>,
) -> MultiPointRow {
    (
        index,
        raw.iter().copied().map(point_i32_row).collect(),
        precise.iter().copied().map(point_f64_row).collect(),
        master.map(|points| points.iter().copied().map(point_f64_row).collect()),
    )
}

fn linkage_row(linkage: &ezdgn_core::AttributeLinkage<'_>) -> LinkageRow {
    let mut entity_number = None;
    let mut mslink = None;
    let mut color_index = None;
    let mut association_id = None;
    let mut high_precision = None;
    match &linkage.data {
        LinkageData::Dmrs {
            entity_number: entity,
            mslink: link,
        }
        | LinkageData::Database {
            entity_number: entity,
            mslink: link,
        } => {
            entity_number = Some(*entity);
            mslink = Some(*link);
        }
        LinkageData::ShapeFill { color_index: color } => color_index = Some(*color),
        LinkageData::AssociationId { association_id: id } => association_id = Some(*id),
        LinkageData::HighPrecision {
            delta_words,
            deltas,
            complete,
        } => {
            high_precision = Some((
                *delta_words,
                deltas.iter().map(|delta| (delta.x, delta.y)).collect(),
                *complete,
            ));
        }
        LinkageData::User | LinkageData::Unparsed => {}
    }
    (
        linkage.offset,
        linkage.raw.len(),
        linkage.declared_size,
        linkage.linkage_type,
        linkage.data.kind().to_owned(),
        entity_number,
        mslink,
        color_index,
        association_id,
        high_precision,
    )
}

fn point_i32_row(point: Point2<i32>) -> PointI32Row {
    (point.x, point.y)
}

fn point_f64_row(point: Point2<f64>) -> PointF64Row {
    (point.x, point.y)
}

fn pair_points(
    first: Option<Point2<f64>>,
    second: Option<Point2<f64>>,
) -> Option<(PointF64Row, PointF64Row)> {
    Some((point_f64_row(first?), point_f64_row(second?)))
}

fn pair_values(first: Option<f64>, second: Option<f64>) -> Option<(f64, f64)> {
    Some((first?, second?))
}

fn raw_point_row(point: RawPoint) -> RawPointRow {
    (point.x, point.y, point.z)
}

fn master_point_row(point: MasterPoint) -> MasterPointRow {
    (point.x, point.y, point.z)
}

fn format_row(format: ezdgn_core::DgnFormat) -> FormatRow {
    (
        format.kind().to_owned(),
        format.dimension().map(ezdgn_core::V7Dimension::as_u8),
    )
}

fn core_error_to_python(error: CoreDgnError) -> PyErr {
    let message = error.to_string();
    if error.is_unsupported_error() {
        UnsupportedDgnError::new_err(message)
    } else if error.is_limit_error() {
        DgnLimitError::new_err(message)
    } else {
        InvalidDgnError::new_err(message)
    }
}

#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    module.add(
        "DEFAULT_MAX_FILE_SIZE_BYTES",
        ezdgn_core::DEFAULT_MAX_FILE_SIZE_BYTES,
    )?;
    module.add("DEFAULT_MAX_RECORDS", ezdgn_core::DEFAULT_MAX_RECORDS)?;
    module.add(
        "MAX_V7_RECORD_SIZE_BYTES",
        ezdgn_core::MAX_V7_RECORD_SIZE_BYTES,
    )?;
    module.add(
        "DEFAULT_MAX_CFB_ENTRIES",
        ezdgn_core::DEFAULT_MAX_CFB_ENTRIES,
    )?;
    module.add("DgnError", py.get_type::<DgnError>())?;
    module.add("InvalidDgnError", py.get_type::<InvalidDgnError>())?;
    module.add("UnsupportedDgnError", py.get_type::<UnsupportedDgnError>())?;
    module.add("DgnLimitError", py.get_type::<DgnLimitError>())?;
    module.add_function(wrap_pyfunction!(core_version, module)?)?;
    module.add_function(wrap_pyfunction!(detect_format_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_v8_cfb, module)?)?;
    module.add_function(wrap_pyfunction!(scan_v7_records, module)?)?;
    module.add_function(wrap_pyfunction!(read_v7_design_settings, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_v7_headers, module)?)?;
    module.add_function(wrap_pyfunction!(read_v7_2d_primitives, module)?)?;
    module.add_function(wrap_pyfunction!(write_v7_2d_bytes, module)?)?;
    Ok(())
}
