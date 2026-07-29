//! PyO3 bindings for `ezdgn-core`.

use ezdgn_core::{
    decode_common_header, decode_design_settings, detect_format, inspect_v8_container, read_v7_2d,
    read_v8, scan_records, scan_v8_objects as core_scan_v8_objects, write_v7_2d,
    CommonElementHeader, DesignSettings, DgnError as CoreDgnError, ElementData2D, LinkageData,
    MasterPoint, Point2, RawPoint, RecordScan, ScanOptions, V7Document2D, V7ElementStyle,
    V7WriteOptions, V8AuxiliaryPage, V8AuxiliaryRecord, V8CommonHeader, V8Document, V8Element,
    V8ElementData, V8Linkage, V8ModelMetadata, V8ObjectPage, V8ObjectRole, V8Point, V8Range3,
    V8Range3I64, V8RawDocument, V8RawObject, V8ReadOptions, V8ScanOptions, WritableElement2D,
};
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

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
type V8LimitsRow = Vec<usize>;

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

/// Return the lossless bounded V8 page/object scan as a Python-native mapping.
#[pyfunction]
fn scan_v8_object_records(
    py: Python<'_>,
    data: &[u8],
    limits: V8LimitsRow,
) -> PyResult<Py<PyDict>> {
    let document =
        core_scan_v8_objects(data, v8_scan_options(&limits)?).map_err(core_error_to_python)?;
    Ok(v8_raw_document_dict(py, &document)?.unbind())
}

/// Return the native semantic V8 model while retaining the complete raw scan.
#[pyfunction]
fn read_v8_document(py: Python<'_>, data: &[u8], limits: V8LimitsRow) -> PyResult<Py<PyDict>> {
    let document = read_v8(data, v8_scan_options(&limits)?).map_err(core_error_to_python)?;
    Ok(v8_document_dict(py, &document)?.unbind())
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

fn v8_scan_options(limits: &[usize]) -> PyResult<V8ScanOptions> {
    let [max_file_size, max_cfb_entries, max_stream_size, max_total_stream_bytes, max_pages, max_objects, max_object_size, max_inflated_stream_size, max_total_inflated_bytes, max_models, max_string_bytes, max_vertices, max_hierarchy_depth] =
        limits
    else {
        return Err(InvalidDgnError::new_err(format!(
            "V8 limits row requires 13 values, got {}",
            limits.len()
        )));
    };
    Ok(V8ScanOptions {
        read: V8ReadOptions {
            max_file_size: *max_file_size,
            max_cfb_entries: *max_cfb_entries,
            max_stream_size: *max_stream_size,
            max_total_stream_bytes: *max_total_stream_bytes,
        },
        max_pages: *max_pages,
        max_objects: *max_objects,
        max_object_size: *max_object_size,
        max_inflated_stream_size: *max_inflated_stream_size,
        max_total_inflated_bytes: *max_total_inflated_bytes,
        max_models: *max_models,
        max_string_bytes: *max_string_bytes,
        max_vertices: *max_vertices,
        max_hierarchy_depth: *max_hierarchy_depth,
    })
}

fn v8_document_dict<'py>(py: Python<'py>, document: &V8Document) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("raw", v8_raw_document_dict(py, &document.raw)?)?;
    let models = PyList::empty(py);
    for model in &document.models {
        let value = PyDict::new(py);
        value.set_item("metadata", v8_model_metadata_dict(py, &model.metadata)?)?;
        let elements = PyList::empty(py);
        for element in &model.elements {
            elements.append(v8_element_dict(py, element)?)?;
        }
        value.set_item("elements", elements)?;
        models.append(value)?;
    }
    result.set_item("models", models)?;
    Ok(result)
}

fn v8_raw_document_dict<'py>(
    py: Python<'py>,
    document: &V8RawDocument,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("container", v8_container_dict(py, &document.container)?)?;
    result.set_item("total_inflated_bytes", document.total_inflated_bytes)?;
    result.set_item("graphical_object_count", document.graphical_object_count())?;
    result.set_item("total_object_count", document.total_object_count())?;

    let models = PyList::empty(py);
    for model in &document.models {
        let value = PyDict::new(py);
        value.set_item("index", v8_model_index_dict(py, &model.index)?)?;
        value.set_item("storage_path", &model.storage_path)?;
        value.set_item("model_header_stream_path", &model.model_header_stream.path)?;
        value.set_item(
            "model_header_stream_bytes",
            PyBytes::new(py, model.model_header_stream.as_bytes()),
        )?;
        value.set_item(
            "model_header_bytes",
            PyBytes::new(py, model.model_header_bytes.as_ref()),
        )?;
        value.set_item(
            "graphical_pages",
            v8_object_pages_list(py, &model.graphical_pages)?,
        )?;
        value.set_item(
            "graphical_auxiliary_pages",
            v8_auxiliary_pages_list(py, &model.graphical_auxiliary_pages)?,
        )?;
        value.set_item(
            "control_pages",
            v8_object_pages_list(py, &model.control_pages)?,
        )?;
        value.set_item(
            "control_auxiliary_pages",
            v8_auxiliary_pages_list(py, &model.control_auxiliary_pages)?,
        )?;
        models.append(value)?;
    }
    result.set_item("models", models)?;
    result.set_item(
        "named_pages",
        v8_object_pages_list(py, &document.named_pages)?,
    )?;
    result.set_item(
        "named_auxiliary_pages",
        v8_auxiliary_pages_list(py, &document.named_auxiliary_pages)?,
    )?;
    Ok(result)
}

fn v8_container_dict<'py>(
    py: Python<'py>,
    container: &ezdgn_core::V8ContainerInfo,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("cfb_version", container.cfb_version)?;
    result.set_item("has_dgn_v8_markers", container.has_dgn_v8_markers)?;
    result.set_item("missing_markers", &container.missing_markers)?;
    result.set_item("model_storage_paths", &container.model_storage_paths)?;
    let entries = PyList::empty(py);
    for entry in &container.entries {
        entries.append((&entry.path, entry.kind.as_str(), entry.size_bytes))?;
    }
    result.set_item("entries", entries)?;
    Ok(result)
}

fn v8_model_index_dict<'py>(
    py: Python<'py>,
    entry: &ezdgn_core::V8ModelIndexEntry,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("index", entry.index)?;
    result.set_item("raw_number", entry.raw_number)?;
    result.set_item("storage_index", entry.storage_index)?;
    result.set_item("model_number", entry.model_number)?;
    result.set_item("flags", entry.flags)?;
    result.set_item("model_id", entry.model_id)?;
    result.set_item("name", &entry.name)?;
    result.set_item("description", &entry.description)?;
    result.set_item("raw_bytes", PyBytes::new(py, entry.raw_bytes.as_ref()))?;
    Ok(result)
}

fn v8_page_header_dict<'py>(
    py: Python<'py>,
    header: ezdgn_core::V8PageHeader,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("record_count", header.record_count)?;
    result.set_item("format_version", header.format_version)?;
    result.set_item("page_number", header.page_number)?;
    result.set_item("population", header.population)?;
    Ok(result)
}

fn v8_object_pages_list<'py>(
    py: Python<'py>,
    pages: &[V8ObjectPage],
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for page in pages {
        let value = PyDict::new(py);
        value.set_item("stream_path", &page.stream_path)?;
        value.set_item("family", page.family.as_str())?;
        value.set_item("header", v8_page_header_dict(py, page.header)?)?;
        value.set_item("inflated_size", page.inflated_size)?;
        let objects = PyList::empty(py);
        for object in &page.objects {
            objects.append(v8_raw_object_dict(py, object)?)?;
        }
        value.set_item("objects", objects)?;
        result.append(value)?;
    }
    Ok(result)
}

fn v8_raw_object_dict<'py>(py: Python<'py>, object: &V8RawObject) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("index", object.index)?;
    result.set_item("page_index", object.page_index)?;
    result.set_item("family", object.family.as_str())?;
    result.set_item("stream_path", &object.stream_path)?;
    result.set_item("inflated_offset", object.inflated_offset)?;
    result.set_item("framing_prefix", object.framing_prefix)?;
    result.set_item("type_and_flags", object.type_and_flags)?;
    result.set_item("element_type", object.element_type)?;
    result.set_item("role", v8_role_name(object.role))?;
    result.set_item("words", object.words)?;
    result.set_item("attribute_words", object.attribute_words)?;
    result.set_item("level", object.level)?;
    result.set_item("element_id", object.element_id)?;
    result.set_item("model_id", object.model_id)?;
    result.set_item("raw_bytes", PyBytes::new(py, object.as_bytes()))?;
    Ok(result)
}

const fn v8_role_name(role: V8ObjectRole) -> &'static str {
    match role {
        V8ObjectRole::Standalone => "standalone",
        V8ObjectRole::Header => "header",
        V8ObjectRole::Component => "component",
        V8ObjectRole::HeaderComponent => "header_component",
    }
}

fn v8_auxiliary_pages_list<'py>(
    py: Python<'py>,
    pages: &[V8AuxiliaryPage],
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for page in pages {
        let value = PyDict::new(py);
        value.set_item("stream_path", &page.stream_path)?;
        value.set_item("header", v8_page_header_dict(py, page.header)?)?;
        value.set_item("inflated_size", page.inflated_size)?;
        let records = PyList::empty(py);
        for record in &page.records {
            records.append(v8_auxiliary_record_dict(py, record)?)?;
        }
        value.set_item("records", records)?;
        result.append(value)?;
    }
    Ok(result)
}

fn v8_auxiliary_record_dict<'py>(
    py: Python<'py>,
    record: &V8AuxiliaryRecord,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("index", record.index)?;
    result.set_item("stream_path", &record.stream_path)?;
    result.set_item("inflated_offset", record.inflated_offset)?;
    result.set_item("magic", record.magic)?;
    result.set_item("kind", record.kind)?;
    result.set_item("reserved", record.reserved)?;
    result.set_item("element_id", record.element_id)?;
    result.set_item("flags", record.flags)?;
    result.set_item("raw_bytes", PyBytes::new(py, record.as_bytes()))?;
    result.set_item("payload", PyBytes::new(py, record.payload()))?;
    Ok(result)
}

fn v8_model_metadata_dict<'py>(
    py: Python<'py>,
    metadata: &V8ModelMetadata,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("index", metadata.index)?;
    result.set_item("storage_path", &metadata.storage_path)?;
    result.set_item("storage_index", metadata.storage_index)?;
    result.set_item("model_number", metadata.model_number)?;
    result.set_item("model_id", metadata.model_id)?;
    result.set_item("index_model_id", metadata.index_model_id)?;
    result.set_item("name", &metadata.name)?;
    result.set_item("description", &metadata.description)?;
    result.set_item("dimension", metadata.dimension.value())?;
    result.set_item("type_and_flags", metadata.type_and_flags)?;
    result.set_item("model_flags", metadata.model_flags)?;
    result.set_item("uor_per_master", metadata.uor_per_master)?;
    result.set_item("scale", metadata.scale)?;
    result.set_item(
        "global_origin_uor",
        v8_point3_row(metadata.global_origin_uor),
    )?;
    result.set_item("extents_uor", v8_range_i64_row(metadata.extents_uor))?;
    result.set_item("extents_master", v8_range_row(metadata.extents_master))?;
    result.set_item("master_unit", &metadata.master_unit)?;
    result.set_item("sub_unit", &metadata.sub_unit)?;
    result.set_item("linkages", v8_linkages_list(py, &metadata.linkages)?)?;
    result.set_item("raw_header", PyBytes::new(py, metadata.raw_header.as_ref()))?;
    Ok(result)
}

fn v8_element_dict<'py>(py: Python<'py>, element: &V8Element) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("index", element.index)?;
    result.set_item("raw_object_index", element.raw.index)?;
    result.set_item("common", v8_common_header_dict(py, &element.common)?)?;
    result.set_item("data", v8_element_data_dict(py, &element.data)?)?;
    result.set_item("parent_index", element.parent_index)?;
    result.set_item("child_indices", &element.child_indices)?;
    result.set_item("linkages", v8_linkages_list(py, &element.linkages)?)?;
    result.set_item(
        "auxiliary_indices",
        element
            .auxiliary_records
            .iter()
            .map(|record| record.index)
            .collect::<Vec<_>>(),
    )?;
    Ok(result)
}

fn v8_common_header_dict<'py>(
    py: Python<'py>,
    common: &V8CommonHeader,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("level", common.level)?;
    result.set_item("element_id", common.element_id)?;
    result.set_item("model_id", common.model_id)?;
    result.set_item("graphic_group", common.graphic_group)?;
    result.set_item("properties", common.properties)?;
    result.set_item("geometry_flags", common.geometry_flags)?;
    result.set_item("line_style", common.line_style)?;
    result.set_item("line_weight", common.line_weight)?;
    result.set_item("color_index", common.color_index)?;
    result.set_item("stored_dimension", common.stored_dimension.value())?;
    result.set_item("dimension", common.dimension.value())?;
    result.set_item("range_uor", v8_range_i64_row(common.range_uor))?;
    result.set_item("range_master", v8_range_row(common.range_master))?;
    result.set_item("attribute_offset", common.attribute_offset)?;
    result.set_item("attribute_length", common.attribute_length)?;
    Ok(result)
}

fn v8_element_data_dict<'py>(
    py: Python<'py>,
    data: &V8ElementData,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("kind", data.kind_name())?;
    match data {
        V8ElementData::Line { start, end } => {
            result.set_item("vertices", [v8_point_row(*start), v8_point_row(*end)])?;
        }
        V8ElementData::LineString { vertices }
        | V8ElementData::Shape { vertices }
        | V8ElementData::Curve { vertices }
        | V8ElementData::BSplinePole { vertices } => {
            result.set_item("vertices", v8_point_rows(vertices))?;
        }
        V8ElementData::PointString {
            vertices,
            orientations,
        } => {
            result.set_item("vertices", v8_point_rows(vertices))?;
            result.set_item(
                "orientations",
                orientations
                    .iter()
                    .map(|orientation| orientation.values)
                    .collect::<Vec<_>>(),
            )?;
        }
        V8ElementData::Text {
            font_id,
            justification,
            width_uor,
            height_uor,
            width_master,
            height_master,
            rotation_radians,
            orientation,
            origin,
            editable_fields,
            encoding,
            text_bytes,
            text,
        } => {
            result.set_item("font_id", *font_id)?;
            result.set_item("justification", *justification)?;
            result.set_item("width_uor", *width_uor)?;
            result.set_item("height_uor", *height_uor)?;
            result.set_item("width_master", *width_master)?;
            result.set_item("height_master", *height_master)?;
            result.set_item("rotation_radians", *rotation_radians)?;
            result.set_item("orientation", orientation)?;
            result.set_item("origin", v8_point_row(*origin))?;
            result.set_item("editable_fields", *editable_fields)?;
            result.set_item("encoding", encoding.as_str())?;
            result.set_item("text_bytes", PyBytes::new(py, text_bytes.as_ref()))?;
            result.set_item("text", text)?;
        }
        V8ElementData::Ellipse {
            primary_axis_uor,
            secondary_axis_uor,
            primary_axis_master,
            secondary_axis_master,
            rotation_radians,
            orientation,
            center,
        } => {
            result.set_item("primary_axis_uor", *primary_axis_uor)?;
            result.set_item("secondary_axis_uor", *secondary_axis_uor)?;
            result.set_item("primary_axis_master", *primary_axis_master)?;
            result.set_item("secondary_axis_master", *secondary_axis_master)?;
            result.set_item("rotation_radians", *rotation_radians)?;
            result.set_item("orientation", orientation)?;
            result.set_item("center", v8_point_row(*center))?;
        }
        V8ElementData::Arc {
            start_angle_radians,
            sweep_angle_radians,
            primary_axis_uor,
            secondary_axis_uor,
            primary_axis_master,
            secondary_axis_master,
            rotation_radians,
            orientation,
            center,
        } => {
            result.set_item("start_angle_radians", *start_angle_radians)?;
            result.set_item("sweep_angle_radians", *sweep_angle_radians)?;
            result.set_item("primary_axis_uor", *primary_axis_uor)?;
            result.set_item("secondary_axis_uor", *secondary_axis_uor)?;
            result.set_item("primary_axis_master", *primary_axis_master)?;
            result.set_item("secondary_axis_master", *secondary_axis_master)?;
            result.set_item("rotation_radians", *rotation_radians)?;
            result.set_item("orientation", orientation)?;
            result.set_item("center", v8_point_row(*center))?;
        }
        V8ElementData::TextNode {
            child_count,
            node_number,
            origin,
        } => {
            result.set_item("child_count", *child_count)?;
            result.set_item("node_number", *node_number)?;
            result.set_item("origin", v8_point_row(*origin))?;
        }
        V8ElementData::ComplexChain { child_count }
        | V8ElementData::ComplexShape { child_count }
        | V8ElementData::UnknownComplex { child_count } => {
            result.set_item("child_count", *child_count)?;
        }
        V8ElementData::Cell {
            child_count,
            boundary_count,
            origin,
            transform,
        } => {
            result.set_item("child_count", *child_count)?;
            result.set_item("boundary_count", *boundary_count)?;
            result.set_item("origin", v8_point_row(*origin))?;
            result.set_item("transform", transform)?;
        }
        V8ElementData::SharedCellInstance {
            name,
            origin,
            transform,
        } => {
            result.set_item("name", name)?;
            result.set_item("origin", v8_point_row(*origin))?;
            result.set_item("transform", transform)?;
        }
        V8ElementData::BSplineCurve {
            child_count,
            properties_raw,
            declared_poles,
        } => {
            result.set_item("child_count", *child_count)?;
            result.set_item("properties_raw", *properties_raw)?;
            result.set_item("declared_poles", *declared_poles)?;
        }
        V8ElementData::Dimension { anchor } => match anchor {
            Some(point) => result.set_item("anchor", v8_point_row(*point))?,
            None => result.set_item("anchor", py.None())?,
        },
        V8ElementData::Unknown => {}
    }
    Ok(result)
}

fn v8_linkages_list<'py>(py: Python<'py>, linkages: &[V8Linkage]) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    for linkage in linkages {
        let value = PyDict::new(py);
        value.set_item("offset", linkage.offset)?;
        value.set_item("words_to_follow", linkage.words_to_follow)?;
        value.set_item("kind_code", linkage.kind_code)?;
        value.set_item("property_id", linkage.property_id)?;
        match &linkage.property_bytes {
            Some(bytes) => value.set_item("property_bytes", PyBytes::new(py, bytes.as_ref()))?,
            None => value.set_item("property_bytes", py.None())?,
        }
        value.set_item("property_text", &linkage.property_text)?;
        value.set_item("complete", linkage.complete)?;
        value.set_item("raw_bytes", PyBytes::new(py, linkage.raw_bytes.as_ref()))?;
        result.append(value)?;
    }
    Ok(result)
}

type V8Point3Row = (f64, f64, f64);
type V8PointRow = (V8Point3Row, V8Point3Row);

fn v8_point3_row(point: ezdgn_core::V8Point3) -> V8Point3Row {
    (point.x, point.y, point.z)
}

fn v8_point_row(point: V8Point) -> V8PointRow {
    (v8_point3_row(point.uor), v8_point3_row(point.master))
}

fn v8_point_rows(points: &[V8Point]) -> Vec<V8PointRow> {
    points.iter().copied().map(v8_point_row).collect()
}

fn v8_range_i64_row(range: V8Range3I64) -> ([i64; 3], [i64; 3]) {
    (range.low, range.high)
}

fn v8_range_row(range: V8Range3) -> (V8Point3Row, V8Point3Row) {
    (v8_point3_row(range.low), v8_point3_row(range.high))
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
    module.add(
        "DEFAULT_MAX_CFB_STREAM_SIZE_BYTES",
        ezdgn_core::DEFAULT_MAX_CFB_STREAM_SIZE_BYTES,
    )?;
    module.add(
        "DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES",
        ezdgn_core::DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES,
    )?;
    module.add("DEFAULT_MAX_V8_PAGES", ezdgn_core::DEFAULT_MAX_V8_PAGES)?;
    module.add("DEFAULT_MAX_V8_OBJECTS", ezdgn_core::DEFAULT_MAX_V8_OBJECTS)?;
    module.add(
        "DEFAULT_MAX_V8_OBJECT_SIZE_BYTES",
        ezdgn_core::DEFAULT_MAX_V8_OBJECT_SIZE_BYTES,
    )?;
    module.add(
        "DEFAULT_MAX_V8_INFLATED_STREAM_BYTES",
        ezdgn_core::DEFAULT_MAX_V8_INFLATED_STREAM_BYTES,
    )?;
    module.add(
        "DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES",
        ezdgn_core::DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES,
    )?;
    module.add("DEFAULT_MAX_V8_MODELS", ezdgn_core::DEFAULT_MAX_V8_MODELS)?;
    module.add(
        "DEFAULT_MAX_V8_STRING_BYTES",
        ezdgn_core::DEFAULT_MAX_V8_STRING_BYTES,
    )?;
    module.add(
        "DEFAULT_MAX_V8_VERTICES",
        ezdgn_core::DEFAULT_MAX_V8_VERTICES,
    )?;
    module.add(
        "DEFAULT_MAX_V8_HIERARCHY_DEPTH",
        ezdgn_core::DEFAULT_MAX_V8_HIERARCHY_DEPTH,
    )?;
    module.add("DgnError", py.get_type::<DgnError>())?;
    module.add("InvalidDgnError", py.get_type::<InvalidDgnError>())?;
    module.add("UnsupportedDgnError", py.get_type::<UnsupportedDgnError>())?;
    module.add("DgnLimitError", py.get_type::<DgnLimitError>())?;
    module.add_function(wrap_pyfunction!(core_version, module)?)?;
    module.add_function(wrap_pyfunction!(detect_format_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_v8_cfb, module)?)?;
    module.add_function(wrap_pyfunction!(scan_v8_object_records, module)?)?;
    module.add_function(wrap_pyfunction!(read_v8_document, module)?)?;
    module.add_function(wrap_pyfunction!(scan_v7_records, module)?)?;
    module.add_function(wrap_pyfunction!(read_v7_design_settings, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_v7_headers, module)?)?;
    module.add_function(wrap_pyfunction!(read_v7_2d_primitives, module)?)?;
    module.add_function(wrap_pyfunction!(write_v7_2d_bytes, module)?)?;
    Ok(())
}
