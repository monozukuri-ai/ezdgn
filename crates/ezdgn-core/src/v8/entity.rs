use std::collections::HashMap;
use std::sync::Arc;

use crate::DgnError;

use super::item::{decode_v8_item_instances, V8ItemInstance, V8ItemInstances};
use super::linkage::{decode_v8_linkages, decode_windows_1252, V8Linkage};
use super::metadata::{
    decode_model_metadata, read_f64, read_i64, V8Dimension, V8ModelMetadata, V8Point3, V8Range3,
    V8Range3I64,
};
use super::raw::{read_u16, read_u32, read_u64};
use super::{
    scan_v8_objects, V8AuxiliaryRecord, V8ObjectRole, V8RawDocument, V8RawObject, V8ScanOptions,
};

const COMMON_HEADER_BYTES: usize = 0x68;
/// Identification prefix (type/flags, attribute offset, size, level, element
/// id, model id) shared by short component records such as B-spline knot and
/// weight vectors, which do not carry the graphic fields of the full header.
const COMPONENT_PREFIX_BYTES: usize = 0x20;
const ELEMENT_3D_FLAG: u32 = 0x0000_0800;
/// A shared-cell instance stores a three-component origin even when its 3D flag is clear.
const SHARED_CELL_ORIGIN_BYTES: usize = 24;
const SHARED_CELL_DEFINITION_TYPE: u16 = 34;
const SHARED_CELL_DEFINITION_BYTES: usize = 0x100;
const SHARED_CELL_VALUES_OFFSET: usize = 0x70;
const SHARED_CELL_VALUE_COUNT: usize = 15;
const SHARED_CELL_RANGE_VALUES: usize = 6;
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
    Utf16Le,
    Utf16Be,
}

impl V8TextEncoding {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Windows1252 => "windows-1252",
            Self::EscapedWindows1252 => "v8-escaped-windows-1252",
            Self::Utf16Le => "utf-16-le",
            Self::Utf16Be => "utf-16-be",
        }
    }
}

/// One face loop of an indexed mesh: zero-based point indices and, for the
/// edge that starts at each of them, whether the edge is drawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct V8MeshFace {
    pub point_indices: Vec<usize>,
    pub visible_edges: Vec<bool>,
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
    /// Header of a shared-cell definition. `transform` holds the same
    /// fifteen values as an instance: the six range values of the definition
    /// (low XYZ, high XYZ) and a 3x3 matrix by rows.
    SharedCellDefinition {
        child_count: usize,
        name: Option<String>,
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
    Mesh {
        child_count: usize,
        style: u32,
        points: Vec<V8Point>,
        faces: Vec<V8MeshFace>,
    },
    MatrixHeader {
        child_count: usize,
        columns: u32,
    },
    MatrixIntegerData {
        values: Vec<i32>,
    },
    MatrixDoubleData {
        values: Vec<f64>,
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
            Self::SharedCellDefinition { .. } => "SHARED_CELL_DEFINITION",
            Self::BSplineCurve { .. } => "BSPLINE_CURVE",
            Self::BSplinePole { .. } => "BSPLINE_POLE",
            Self::Dimension { .. } => "DIMENSION",
            Self::Mesh { .. } => "MESH",
            Self::MatrixHeader { .. } => "MATRIX_HEADER",
            Self::MatrixIntegerData { .. } => "MATRIX_INTEGER_DATA",
            Self::MatrixDoubleData { .. } => "MATRIX_DOUBLE_DATA",
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
            | Self::SharedCellDefinition { child_count, .. }
            | Self::BSplineCurve { child_count, .. }
            | Self::Mesh { child_count, .. }
            | Self::MatrixHeader { child_count, .. }
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
                | Self::SharedCellDefinition { .. }
                | Self::BSplinePole { .. }
                | Self::TextNode { .. }
                | Self::MatrixHeader { .. }
                | Self::MatrixIntegerData { .. }
                | Self::MatrixDoubleData { .. }
        )
    }
}

impl V8ElementData {
    /// The stored orientation quaternion `(w, x, y, z)` of a 3D text,
    /// ellipse or arc, exactly as stored.
    ///
    /// A 3D ellipse or arc stores the four values alone. A 3D text stores
    /// two other values first, so the quaternion is the last four of its six
    /// `orientation` values. A 2D element has a rotation angle and no
    /// quaternion.
    #[must_use]
    pub fn quaternion(&self) -> Option<[f64; 4]> {
        let values = match self {
            Self::Text { orientation, .. } if orientation.len() == 6 => &orientation[2..],
            Self::Ellipse { orientation, .. } | Self::Arc { orientation, .. }
                if orientation.len() == 4 =>
            {
                &orientation[..]
            }
            _ => return None,
        };
        <[f64; 4]>::try_from(values).ok()
    }

    /// Place a point of a shared-cell definition with this instance.
    ///
    /// `point` is in the definition's own space, in UOR. The result is
    /// `origin + M * point` in the UOR of the model, where `M` is the 3x3
    /// matrix that the last nine `transform` values hold by rows. Returns
    /// `None` for another element kind and for an instance that does not
    /// hold fifteen values.
    #[must_use]
    pub fn place_definition_point(&self, point: V8Point3) -> Option<V8Point3> {
        let Self::SharedCellInstance {
            origin, transform, ..
        } = self
        else {
            return None;
        };
        if transform.len() != SHARED_CELL_VALUE_COUNT {
            return None;
        }
        let matrix = &transform[SHARED_CELL_RANGE_VALUES..];
        let row = |row: usize| {
            matrix[3 * row] * point.x
                + matrix[3 * row + 1] * point.y
                + matrix[3 * row + 2] * point.z
        };
        Some(V8Point3::new(
            origin.uor.x + row(0),
            origin.uor.y + row(1),
            origin.uor.z + row(2),
        ))
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

    /// Item instances decoded from the auxiliary records, in stored order.
    ///
    /// They are decoded on each call and are not stored with the element. An
    /// entry that does not match the observed layout is not listed and stays
    /// available through `auxiliary_records`. One call returns at most
    /// [`V8_MAX_ITEM_BYTES`](super::V8_MAX_ITEM_BYTES) stored bytes as
    /// instances; use [`Self::decode_item_instances`] to learn whether that
    /// limit left an entry out.
    #[must_use]
    pub fn item_instances(&self) -> Vec<V8ItemInstance> {
        self.decode_item_instances().instances
    }

    /// The item instances of [`Self::item_instances`] together with the
    /// number of item entries that the size limit of one call left raw.
    #[must_use]
    pub fn decode_item_instances(&self) -> V8ItemInstances {
        decode_v8_item_instances(
            self.auxiliary_records
                .iter()
                .filter_map(V8AuxiliaryRecord::entries)
                .flatten(),
        )
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

/// One shared-cell definition from the named object pages.
///
/// Its coordinates are in the definition's own space. The master values use
/// the scale of the model the definitions were decoded for, with no global
/// origin. A shared-cell instance places a definition point `p` at
/// `origin + M * p`, where `M` is the 3x3 matrix by rows in the last nine of
/// the instance's fifteen `transform` values.
#[derive(Debug, Clone)]
pub struct V8SharedCellDefinition {
    /// Name from the definition's name linkage; empty when it has none.
    pub name: String,
    /// The definition header, then its components. `index`, `parent_index`
    /// and `child_indices` are positions in this list.
    pub elements: Vec<V8Element>,
}

impl V8SharedCellDefinition {
    /// The definition header element.
    #[must_use]
    pub fn header(&self) -> Option<&V8Element> {
        self.elements.first()
    }

    /// The six range values the header stores: low XYZ, then high XYZ.
    #[must_use]
    pub fn range_values(&self) -> Option<&[f64]> {
        match &self.header()?.data {
            V8ElementData::SharedCellDefinition { transform, .. } => {
                transform.get(..SHARED_CELL_RANGE_VALUES)
            }
            _ => None,
        }
    }

    /// Whether a shared-cell instance refers to this definition.
    ///
    /// An instance repeats the name and the six range values of its
    /// definition. Both must be equal: one file can hold several definitions
    /// with one name. The values are compared bit for bit, so `-0.0` does
    /// not match `0.0`.
    #[must_use]
    pub fn matches(&self, instance: &V8ElementData) -> bool {
        let V8ElementData::SharedCellInstance {
            name: Some(name),
            transform,
            ..
        } = instance
        else {
            return false;
        };
        *name == self.name
            && transform.len() == SHARED_CELL_VALUE_COUNT
            && self.range_values().is_some_and(|range| {
                range
                    .iter()
                    .zip(transform)
                    .all(|(stored, repeated)| stored.to_bits() == repeated.to_bits())
            })
    }
}

/// Decode the shared-cell definitions of a scanned document.
///
/// A definition is a type-34 header in the named object pages followed by
/// its components. `metadata` supplies the unit scale for the master values.
/// A run that does not decode as a definition is skipped and stays raw;
/// resource-limit errors are returned. Components that follow after the
/// declared child counts are used up belong to no header: they are not part
/// of the definition and stay raw. No count of them is given.
pub fn decode_v8_shared_cell_definitions(
    raw: &V8RawDocument,
    metadata: &V8ModelMetadata,
    options: V8ScanOptions,
) -> Result<Vec<V8SharedCellDefinition>, DgnError> {
    let mut local = metadata.clone();
    local.global_origin_uor = V8Point3::new(0.0, 0.0, 0.0);
    let mut auxiliary = HashMap::<u64, Vec<V8AuxiliaryRecord>>::new();
    for record in raw
        .named_auxiliary_pages
        .iter()
        .flat_map(|page| page.records.iter())
    {
        auxiliary
            .entry(record.element_id)
            .or_default()
            .push(record.clone());
    }
    let objects = raw
        .named_pages
        .iter()
        .flat_map(|page| page.objects.iter())
        .collect::<Vec<_>>();
    let mut definitions = Vec::new();
    let mut start = 0usize;
    while start < objects.len() {
        let header = objects[start];
        if header.element_type != SHARED_CELL_DEFINITION_TYPE || header.role != V8ObjectRole::Header
        {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < objects.len() && objects[end].role.is_component() {
            end += 1;
        }
        let run = objects[start..end].iter().copied();
        match decode_objects(
            run,
            end - start,
            &local,
            options,
            &mut auxiliary,
            ObjectPages::Named,
        ) {
            Ok(mut elements) => {
                // Once every declared child is placed, a further component
                // has no parent. It and what follows are not components of
                // this definition.
                let members = elements
                    .iter()
                    .skip(1)
                    .position(|element| element.parent_index.is_none())
                    .map_or(elements.len(), |orphan| orphan + 1);
                elements.truncate(members);
                if let Some(V8ElementData::SharedCellDefinition { name, .. }) =
                    elements.first().map(|element| &element.data)
                {
                    definitions.push(V8SharedCellDefinition {
                        name: name.clone().unwrap_or_default(),
                        elements,
                    });
                }
            }
            Err(DgnError::InvalidV8Geometry { .. } | DgnError::InvalidV8Hierarchy { .. }) => {}
            Err(error) => return Err(error),
        }
        start = end;
    }
    Ok(definitions)
}

/// Scan a V8 file and decode its shared-cell definitions with the unit scale
/// of the model at `model_index`. An index that is not a model of the
/// document is [`DgnError::V8ModelIndexOutOfRange`].
pub fn read_v8_shared_cell_definitions(
    input: &[u8],
    options: V8ScanOptions,
    model_index: usize,
) -> Result<Vec<V8SharedCellDefinition>, DgnError> {
    let raw = scan_v8_objects(input, options)?;
    let model = raw
        .models
        .get(model_index)
        .ok_or_else(|| missing_model(model_index, raw.models.len()))?;
    let metadata = decode_model_metadata(model, options)?;
    decode_v8_shared_cell_definitions(&raw, &metadata, options)
}

/// A wrong index is the caller's error. A document without any model has no
/// right index: that is a property of the file.
fn missing_model(index: usize, count: usize) -> DgnError {
    if count == 0 {
        return DgnError::InvalidV8ModelIndex {
            offset: 0,
            context: "the document has no model".to_owned(),
        };
    }
    DgnError::V8ModelIndexOutOfRange { index, count }
}

impl V8Document {
    /// The shared-cell definitions of the document, decoded on each call
    /// with the unit scale of the model at `model_index`. An index that is
    /// not a model of the document is [`DgnError::V8ModelIndexOutOfRange`].
    /// Each call builds a new result: a caller that keeps the result of one
    /// call for each model holds the definitions once for each model.
    pub fn shared_cell_definitions(
        &self,
        model_index: usize,
        options: V8ScanOptions,
    ) -> Result<Vec<V8SharedCellDefinition>, DgnError> {
        let model = self
            .models
            .get(model_index)
            .ok_or_else(|| missing_model(model_index, self.models.len()))?;
        decode_v8_shared_cell_definitions(&self.raw, &model.metadata, options)
    }
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

    decode_objects(
        model.graphical_objects(),
        model.graphical_objects().count(),
        metadata,
        options,
        &mut auxiliary,
        ObjectPages::Graphical,
    )
}

/// The pages a run of objects comes from. A shared-cell definition header
/// is read as one only in the named pages; in the graphical pages of a model
/// a type-34 header stays the unknown complex element it always was.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjectPages {
    Graphical,
    Named,
}

/// Decode a run of raw objects into elements with their hierarchy. Records
/// of `auxiliary` move to the element with the same element ID.
fn decode_objects<'a>(
    objects: impl Iterator<Item = &'a V8RawObject>,
    count: usize,
    metadata: &V8ModelMetadata,
    options: V8ScanOptions,
    auxiliary: &mut HashMap<u64, Vec<V8AuxiliaryRecord>>,
    pages: ObjectPages,
) -> Result<Vec<V8Element>, DgnError> {
    let mut elements = Vec::with_capacity(count);
    for raw in objects {
        let common = decode_common(raw, metadata)?;
        let linkages = decode_v8_linkages(
            raw.attribute_bytes(),
            options,
            &format!("element {}", common.element_id),
        )?;
        let data = match decode_element(raw, &common, metadata, &linkages, options, pages) {
            Ok(data) => data,
            // Component records (B-spline knots/weights and other
            // writer-specific payloads shorter than a graphical element)
            // must not reject the model; the enclosing header still gets its
            // decodable children, everything else stays a raw child.
            Err(_) if raw.role.is_component() => V8ElementData::Unknown,
            // 1要素のジオメトリが壊れている(実ファイル: 頂点1個の線列)だけなら、その要素を
            // Unknown にしてモデル全体は読む。オブジェクトの枠は生走査で検証済みなので、
            // 他の要素の読み取りには影響しない
            Err(DgnError::InvalidV8Geometry { .. }) => V8ElementData::Unknown,
            Err(error) => return Err(error),
        };
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
    resolve_meshes(&mut elements, metadata, options.max_vertices)?;
    Ok(elements)
}

fn decode_common(
    raw: &V8RawObject,
    metadata: &V8ModelMetadata,
) -> Result<V8CommonHeader, DgnError> {
    let bytes = raw.as_bytes();
    if bytes.len() < COMMON_HEADER_BYTES && !raw.role.is_component() {
        return Err(geometry_error(
            raw,
            format!(
                "common graphical header needs {COMMON_HEADER_BYTES} bytes, got {}",
                bytes.len()
            ),
        ));
    }
    if bytes.len() < COMPONENT_PREFIX_BYTES {
        // Even component records (B-spline knot/weight vectors carry only the
        // identification prefix of the graphical header) need type, id, and
        // attribute fields.
        return Err(geometry_error(
            raw,
            format!(
                "component header needs {COMPONENT_PREFIX_BYTES} bytes, got {}",
                bytes.len()
            ),
        ));
    }
    // The header stores the low corner followed by the extent, not the high
    // corner.
    let low = [
        read_i64(bytes, 0x38).unwrap_or_default(),
        read_i64(bytes, 0x40).unwrap_or_default(),
        read_i64(bytes, 0x48).unwrap_or_default(),
    ];
    let extent = [
        read_i64(bytes, 0x50).unwrap_or_default(),
        read_i64(bytes, 0x58).unwrap_or_default(),
        read_i64(bytes, 0x60).unwrap_or_default(),
    ];
    let range_uor = V8Range3I64 {
        low,
        high: [
            low[0].saturating_add(extent[0]),
            low[1].saturating_add(extent[1]),
            low[2].saturating_add(extent[2]),
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
    pages: ObjectPages,
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
        SHARED_CELL_DEFINITION_TYPE
            if pages == ObjectPages::Named
                && raw.role.is_header()
                && raw.primary_bytes().len() == SHARED_CELL_DEFINITION_BYTES =>
        {
            decode_shared_cell_definition(raw, linkages)
        }
        35 => decode_shared_cell(raw, metadata, linkages),
        36 => decode_dimension(raw, common, metadata),
        101 => decode_matrix_header(raw),
        102 => decode_matrix_values(raw, options, MATRIX_INTEGER_BYTES, |bytes, offset| {
            read_i32(bytes, offset).map(Ok)
        })
        .map(|values| V8ElementData::MatrixIntegerData { values }),
        103 => decode_matrix_values(raw, options, MATRIX_DOUBLE_BYTES, |bytes, offset| {
            read_f64(bytes, offset).map(|value| {
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(geometry_error(raw, "matrix value is not finite"))
                }
            })
        })
        .map(|values| V8ElementData::MatrixDoubleData { values }),
        105 => decode_mesh_header(raw),
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

/// `FF FE` / `FE FF` + UTF-16 text, as some producers store V8 strings (production
/// drawings in Latvian, Czech and English, 2026). The payload must be an even number
/// of bytes apart from a trailing NUL, decode without unpaired surrogates and contain
/// no control characters other than tab and line ends; a code page string that merely
/// begins with those two bytes fails that test and stays on the code page path.
fn utf16_byte_order(raw: &[u8]) -> Option<V8TextEncoding> {
    let encoding = match raw.get(..2)? {
        [0xff, 0xfe] => V8TextEncoding::Utf16Le,
        [0xfe, 0xff] => V8TextEncoding::Utf16Be,
        _ => return None,
    };
    decode_utf16_payload(&raw[2..], encoding).map(|_| encoding)
}

fn decode_utf16_payload(body: &[u8], encoding: V8TextEncoding) -> Option<String> {
    let body = if body.len() % 2 == 1 {
        if body.last() != Some(&0) {
            return None;
        }
        &body[..body.len() - 1]
    } else {
        body
    };
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|pair| {
            let bytes = [pair[0], pair[1]];
            if encoding == V8TextEncoding::Utf16Be {
                u16::from_be_bytes(bytes)
            } else {
                u16::from_le_bytes(bytes)
            }
        })
        .collect();
    let text = String::from_utf16(&units).ok()?;
    let text = text.trim_end_matches('\0');
    if text
        .chars()
        .any(|c| c < ' ' && !matches!(c, '\t' | '\n' | '\r'))
    {
        return None;
    }
    Some(text.to_owned())
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
    // The text payload follows the fixed-size header (editable-field word at
    // 0xa8 / 0xc8). Objects are usually padded to an even length, but some
    // writers pad to a 4-byte boundary, so the payload position is anchored on
    // the header layout rather than derived from the object length.
    let editable_offset: usize = match common.dimension {
        V8Dimension::Two => 0xa8,
        V8Dimension::Three => 0xc8,
    };
    let text_start = editable_offset + 2;
    let text_end = text_start
        .checked_add(text_length)
        .ok_or_else(|| geometry_error(raw, "padded text length overflows"))?;
    if text_end > primary.len() {
        return Err(geometry_error(
            raw,
            format!(
                "text payload of {text_length} bytes at 0x{text_start:x} exceeds the object ({} bytes)",
                primary.len()
            ),
        ));
    }
    let raw_text = primary
        .get(text_start..text_start + text_length)
        .ok_or_else(|| geometry_error(raw, "text payload is truncated"))?;
    let (encoding, decoded_bytes) = if let Some(bytes) = raw_text.strip_prefix(&[0xff, 0xfe, 1, 0])
    {
        (V8TextEncoding::EscapedWindows1252, bytes)
    } else if let Some(encoding) = utf16_byte_order(raw_text) {
        (encoding, &raw_text[2..])
    } else if std::str::from_utf8(raw_text).is_ok() {
        (V8TextEncoding::Utf8, raw_text)
    } else {
        (V8TextEncoding::Windows1252, raw_text)
    };
    let text = match encoding {
        V8TextEncoding::Utf8 => std::str::from_utf8(decoded_bytes)
            .unwrap_or_default()
            .to_owned(),
        V8TextEncoding::Utf16Le | V8TextEncoding::Utf16Be => {
            decode_utf16_payload(decoded_bytes, encoding).unwrap_or_default()
        }
        V8TextEncoding::Windows1252 | V8TextEncoding::EscapedWindows1252 => {
            decode_windows_1252(decoded_bytes)
        }
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
    metadata: &V8ModelMetadata,
    linkages: &[V8Linkage],
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let origin_offset = primary
        .len()
        .checked_sub(SHARED_CELL_ORIGIN_BYTES)
        .ok_or_else(|| geometry_error(raw, "shared-cell origin is missing"))?;
    let origin = decode_point(primary, origin_offset, V8Dimension::Three, metadata, raw)?;
    let transform_start = 0x70;
    let transform_bytes = origin_offset.saturating_sub(transform_start);
    let transform_count = transform_bytes / 8;
    let transform = read_f64_values(primary, transform_start, transform_count, raw)?;
    Ok(V8ElementData::SharedCellInstance {
        name: shared_cell_name(linkages),
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

fn decode_shared_cell_definition(
    raw: &V8RawObject,
    linkages: &[V8Linkage],
) -> Result<V8ElementData, DgnError> {
    let primary = raw.primary_bytes();
    let child_count = count_at(raw, primary, COMMON_HEADER_BYTES)?;
    let transform = read_f64_values(
        primary,
        SHARED_CELL_VALUES_OFFSET,
        SHARED_CELL_VALUE_COUNT,
        raw,
    )?;
    Ok(V8ElementData::SharedCellDefinition {
        child_count,
        name: shared_cell_name(linkages),
        transform,
    })
}

fn shared_cell_name(linkages: &[V8Linkage]) -> Option<String> {
    linkages
        .iter()
        .find(|linkage| linkage.property_id == Some(1))
        .and_then(|linkage| linkage.property_text.clone())
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
            // A component without an open header (writer inconsistency) is kept
            // as a standalone element instead of rejecting the model.
            if let Some(parent) = stack.last_mut() {
                parent.remaining -= 1;
                let parent_index = parent.index;
                elements[index].parent_index = Some(parent_index);
                elements[parent_index].child_indices.push(index);
            }
        } else if !stack.is_empty() {
            // Some writers declare more direct children than they emit; a
            // standalone object simply closes the pending headers (best effort)
            // so the rest of the model stays readable.
            stack.clear();
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
    // Headers still open at the end of the model (declared children never
    // written) are accepted with the children that were actually seen.
    Ok(())
}

const MESH_STYLE_INDEXED_FACE_LOOPS: u32 = 1;
const MATRIX_VALUES_OFFSET: usize = 0x78;
const MATRIX_INTEGER_BYTES: usize = 4;
const MATRIX_DOUBLE_BYTES: usize = 8;
const POINT_COMPONENTS: usize = 3;
const MATRIX_VALUES_PER_VERTEX_LIMIT: usize = 16;

fn decode_mesh_header(raw: &V8RawObject) -> Result<V8ElementData, DgnError> {
    let bytes = raw.primary_bytes();
    let child_count = count_at(raw, bytes, 0x68)?;
    let Some(style) = read_u32(bytes, 0x6c) else {
        return Ok(V8ElementData::UnknownComplex { child_count });
    };
    Ok(V8ElementData::Mesh {
        child_count,
        style,
        points: Vec::new(),
        faces: Vec::new(),
    })
}

fn decode_matrix_header(raw: &V8RawObject) -> Result<V8ElementData, DgnError> {
    let bytes = raw.primary_bytes();
    let child_count = count_at(raw, bytes, 0x68)?;
    let Some(columns) = read_u32(bytes, 0x6c) else {
        return Ok(V8ElementData::UnknownComplex { child_count });
    };
    Ok(V8ElementData::MatrixHeader {
        child_count,
        columns,
    })
}

/// Matrix data elements store their scalar count at 0x68 (repeated at 0x6c,
/// followed by a type tag) and the values from 0x78.
fn decode_matrix_values<T>(
    raw: &V8RawObject,
    options: V8ScanOptions,
    value_bytes: usize,
    read_value: impl Fn(&[u8], usize) -> Option<Result<T, DgnError>>,
) -> Result<Vec<T>, DgnError> {
    let bytes = raw.primary_bytes();
    let count = count_at(raw, bytes, 0x68)?;
    let limit = options
        .max_vertices
        .saturating_mul(MATRIX_VALUES_PER_VERTEX_LIMIT);
    if count > limit {
        return Err(DgnError::V8VertexLimitExceeded {
            element_id: raw.element_id.unwrap_or_default(),
            actual: count,
            limit,
        });
    }
    let end = count
        .checked_mul(value_bytes)
        .and_then(|size| size.checked_add(MATRIX_VALUES_OFFSET))
        .ok_or_else(|| geometry_error(raw, "matrix size overflows"))?;
    if end > bytes.len() {
        return Err(geometry_error(
            raw,
            format!(
                "matrix of {count} values needs {end} bytes, got {}",
                bytes.len()
            ),
        ));
    }
    (0..count)
        .map(|position| {
            read_value(bytes, MATRIX_VALUES_OFFSET + position * value_bytes)
                .unwrap_or_else(|| Err(geometry_error(raw, "matrix value is truncated")))
        })
        .collect()
}

/// Fill each indexed-face-loop mesh header from its matrix children: the
/// double data holds XYZ points in UOR and the integer data one-based point
/// indices, zero ending a face and a negative index marking a hidden edge.
/// A mesh of another style, or whose children do not form such a mesh, is
/// kept as an unknown complex element.
fn resolve_meshes(
    elements: &mut [V8Element],
    metadata: &V8ModelMetadata,
    max_vertices: usize,
) -> Result<(), DgnError> {
    for index in 0..elements.len() {
        let V8ElementData::Mesh {
            child_count, style, ..
        } = elements[index].data
        else {
            continue;
        };
        let resolved = if style == MESH_STYLE_INDEXED_FACE_LOOPS {
            indexed_face_loop_mesh(elements, index, metadata, max_vertices)?
        } else {
            None
        };
        elements[index].data = match resolved {
            Some(IndexedFaceLoopMesh { points, faces }) => V8ElementData::Mesh {
                child_count,
                style,
                points,
                faces,
            },
            None => V8ElementData::UnknownComplex { child_count },
        };
    }
    Ok(())
}

struct IndexedFaceLoopMesh {
    points: Vec<V8Point>,
    faces: Vec<V8MeshFace>,
}

fn indexed_face_loop_mesh(
    elements: &[V8Element],
    index: usize,
    metadata: &V8ModelMetadata,
    max_vertices: usize,
) -> Result<Option<IndexedFaceLoopMesh>, DgnError> {
    // Only the observed XYZ and point-index matrix pair is supported. Do not
    // guess when a different matrix layout or incomplete hierarchy is present.
    let [point_header, point_data, index_header, index_data] =
        elements[index].child_indices.as_slice()
    else {
        return Ok(None);
    };
    let (
        V8ElementData::MatrixHeader {
            child_count: 0,
            columns: 3,
        },
        V8ElementData::MatrixDoubleData {
            values: coordinates,
        },
        V8ElementData::MatrixHeader {
            child_count: 0,
            columns: 1,
        },
        V8ElementData::MatrixIntegerData { values: indices },
    ) = (
        &elements[*point_header].data,
        &elements[*point_data].data,
        &elements[*index_header].data,
        &elements[*index_data].data,
    )
    else {
        return Ok(None);
    };
    if coordinates.is_empty() || coordinates.len() % POINT_COMPONENTS != 0 {
        return Ok(None);
    }
    let point_count = coordinates.len() / POINT_COMPONENTS;
    if point_count > max_vertices {
        return Err(DgnError::V8VertexLimitExceeded {
            element_id: elements[index].common.element_id,
            actual: point_count,
            limit: max_vertices,
        });
    }
    let Some(faces) = face_loops(indices, point_count) else {
        return Ok(None);
    };
    let points: Vec<V8Point> = coordinates
        .chunks_exact(POINT_COMPONENTS)
        .map(|xyz| {
            let uor = V8Point3::new(xyz[0], xyz[1], xyz[2]);
            V8Point {
                uor,
                master: metadata.to_master(uor),
            }
        })
        .collect();
    Ok(Some(IndexedFaceLoopMesh { points, faces }))
}

fn face_loops(indices: &[i32], point_count: usize) -> Option<Vec<V8MeshFace>> {
    let mut faces = Vec::new();
    let mut face = V8MeshFace::default();
    for &value in indices {
        if value == 0 {
            if !face.point_indices.is_empty() {
                if face.point_indices.len() < 3 {
                    return None;
                }
                faces.push(std::mem::take(&mut face));
            }
            continue;
        }
        let point_index = usize::try_from(value.unsigned_abs()).ok()?.checked_sub(1)?;
        if point_index >= point_count {
            return None;
        }
        face.point_indices.push(point_index);
        face.visible_edges.push(value > 0);
    }
    if !face.point_indices.is_empty() {
        if face.point_indices.len() < 3 {
            return None;
        }
        faces.push(face);
    }
    (!faces.is_empty()).then_some(faces)
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

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    read_u32(bytes, offset).map(|value| i32::from_le_bytes(value.to_le_bytes()))
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
    use crate::v8::auxiliary::test_support::{entry, payload};
    use crate::v8::item::test_support::{item_data, memory as item_memory, MEMORY_PER_STORED_BYTE};
    use crate::v8::raw::test_support::{
        auxiliary_record, dgn_with_named_pages, dgn_with_objects, dgn_with_objects_and_auxiliary,
        dgn_with_pages, MODEL_ID,
    };
    use crate::v8::V8_ITEM_INSTANCE_ENTRY_TYPE;

    const MODEL_HEADER_TYPE: u32 = 66;
    const MODEL_HEADER_BYTES: usize = 0xe8;
    const MODEL_UOR_PER_MASTER_OFFSET: usize = 0xe0;
    const SHARED_CELL_RECORD_BYTES: usize = 0x100;
    const SHARED_CELL_TRANSFORM_OFFSET: usize = 0x70;
    const SHARED_CELL_TRANSFORM_VALUES: [f64; 15] = [
        -1.0, -1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
    ];

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn write_f64(bytes: &mut [u8], offset: usize, value: f64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn model_header() -> Vec<u8> {
        let mut record = vec![0; MODEL_HEADER_BYTES];
        write_u32(&mut record, 0, MODEL_HEADER_TYPE);
        write_u32(&mut record, 4, (MODEL_HEADER_BYTES / 2) as u32);
        write_u32(&mut record, 8, (MODEL_HEADER_BYTES / 2) as u32);
        write_u64(&mut record, 0x18, MODEL_ID);
        write_f64(&mut record, MODEL_UOR_PER_MASTER_OFFSET, 1000.0);
        let mut stream = 0_u32.to_le_bytes().to_vec();
        stream.extend(record);
        stream
    }

    fn shared_cell_record(geometry_flags: u32, origin_uor: [f64; 3]) -> Vec<u8> {
        let mut record = vec![0; SHARED_CELL_RECORD_BYTES];
        write_u32(&mut record, 0, 0x1000_0023);
        write_u32(&mut record, 4, (SHARED_CELL_RECORD_BYTES / 2) as u32);
        write_u32(&mut record, 8, (SHARED_CELL_RECORD_BYTES / 2) as u32);
        write_u32(&mut record, 0x0c, 64);
        write_u64(&mut record, 0x10, 602);
        write_u64(&mut record, 0x18, MODEL_ID);
        write_u32(&mut record, 0x28, geometry_flags);
        for (index, value) in SHARED_CELL_TRANSFORM_VALUES.iter().enumerate() {
            write_f64(
                &mut record,
                SHARED_CELL_TRANSFORM_OFFSET + index * 8,
                *value,
            );
        }
        let origin_offset = SHARED_CELL_RECORD_BYTES - SHARED_CELL_ORIGIN_BYTES;
        for (index, value) in origin_uor.iter().enumerate() {
            write_f64(&mut record, origin_offset + index * 8, *value);
        }
        record
    }

    fn read_shared_cell(geometry_flags: u32, origin_uor: [f64; 3]) -> V8Element {
        let dgn = dgn_with_objects(
            &model_header(),
            &[shared_cell_record(geometry_flags, origin_uor)],
        );
        let document = read_v8(&dgn, V8ScanOptions::default()).unwrap();
        let elements = &document.models[0].elements;
        assert_eq!(elements.len(), 1);
        elements[0].clone()
    }

    const SHARED_CELL_ELEMENT_ID: u64 = 602;
    const ITEM_ATTRIBUTES: [(&str, &str); 2] = [
        ("instanceId", ""),
        ("xmlns", "Sample__x0020__Library.01.00"),
    ];

    fn read_with_auxiliary(records: &[Vec<u8>]) -> V8Document {
        let dgn = dgn_with_objects_and_auxiliary(
            &model_header(),
            &[shared_cell_record(0, [1.0, 2.0, 0.0])],
            records,
        );
        read_v8(&dgn, V8ScanOptions::default()).unwrap()
    }

    #[test]
    fn item_instances_are_decoded_from_the_auxiliary_record_of_their_element() {
        let pump = item_data(
            900,
            "Pump",
            &ITEM_ATTRIBUTES,
            &[("Tag", "P-101"), ("Note", "")],
        );
        let history = item_data(
            900,
            "History",
            &ITEM_ATTRIBUTES,
            &[("Installed", "1986-01-01")],
        );
        let other = [3, 0, 0, 0, 9, 9];
        let record = auxiliary_record(
            SHARED_CELL_ELEMENT_ID,
            3,
            &payload(&[
                entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &pump),
                entry(0x5709_0000, 0, true, &other),
                entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 2, true, &history),
            ]),
        );
        let stray = auxiliary_record(
            SHARED_CELL_ELEMENT_ID + 1,
            1,
            &payload(&[entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &pump)]),
        );
        let document = read_with_auxiliary(&[record, stray]);
        let element = &document.models[0].elements[0];
        assert_eq!(element.kind(), "SHARED_CELL_INSTANCE");
        assert_eq!(element.auxiliary_records.len(), 1);
        let entries = element.auxiliary_records[0].entries().unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.type_id, entry.entry_id))
                .collect::<Vec<_>>(),
            [
                (V8_ITEM_INSTANCE_ENTRY_TYPE, 1),
                (0x5709_0000, 0),
                (V8_ITEM_INSTANCE_ENTRY_TYPE, 2)
            ]
        );
        assert_eq!(entries[0].data(), pump);
        assert_eq!(entries[1].data(), other);

        let items = element.item_instances();
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].entry_id, &*items[0].class_name), (1, "Pump"));
        assert_eq!(items[0].schema_element_id, 900);
        assert_eq!(items[0].property("Tag"), Some("P-101"));
        assert_eq!(items[0].property("Note"), Some(""));
        assert_eq!(
            items[0].schema_reference(),
            Some("Sample__x0020__Library.01.00")
        );
        assert_eq!((items[1].entry_id, &*items[1].class_name), (2, "History"));
        assert_eq!(items[1].property("Installed"), Some("1986-01-01"));
    }

    #[test]
    fn item_data_over_the_size_limit_stays_raw_and_is_counted() {
        // One table string of 127 three-byte characters; every attribute
        // pair is four stored bytes that name it twice.
        let long = "\u{3042}".repeat(127);
        // enough pairs for more than half of the limit in stored bytes
        let pairs = crate::v8::V8_MAX_ITEM_BYTES / 8 + 1;
        let attributes = vec![(long.as_str(), long.as_str()); pairs];
        let large = item_data(900, &long, &attributes, &[]);
        assert!(large.len() > crate::v8::V8_MAX_ITEM_BYTES / 2);
        let small = item_data(900, "Pump", &ITEM_ATTRIBUTES, &[("Tag", "P-101")]);
        let record = auxiliary_record(
            SHARED_CELL_ELEMENT_ID,
            3,
            &payload(&[
                entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &large),
                entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 2, true, &large),
                entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 3, true, &small),
            ]),
        );
        // the file is read, and the record keeps its exact bytes
        let document = read_with_auxiliary(std::slice::from_ref(&record));
        let element = &document.models[0].elements[0];
        assert_eq!(element.kind(), "SHARED_CELL_INSTANCE");
        assert_eq!(element.auxiliary_records[0].as_bytes(), record.as_slice());
        assert_eq!(element.auxiliary_records[0].entries().unwrap().len(), 3);

        // The second large entry does not fit in what the call has left: it
        // is counted and stays raw. The small entry after it is decoded.
        let decoded = element.decode_item_instances();
        assert_eq!(
            decoded
                .instances
                .iter()
                .map(|item| item.entry_id)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(decoded.over_limit, 1);
        assert_eq!(element.item_instances(), decoded.instances);
        assert_eq!(decoded.instances[0].attributes.len(), pairs);
        let stored = large.len() + small.len();
        assert!(stored <= crate::v8::V8_MAX_ITEM_BYTES);
        let memory = decoded.instances.iter().map(item_memory).sum::<usize>();
        assert!(memory <= MEMORY_PER_STORED_BYTE * stored);
    }

    #[test]
    fn the_item_data_of_many_hostile_elements_takes_memory_in_line_with_the_stored_bytes() {
        // Every element carries one instance that names one long table
        // string 600 times. The 600 uses are 1,200 of its stored bytes and
        // name 228,600 bytes of text. All instances of the document are
        // decoded and kept.
        const ELEMENTS: usize = 2_000;
        const PAIRS: usize = 300;
        let long = "\u{3042}".repeat(127);
        let attributes = vec![(long.as_str(), long.as_str()); PAIRS];
        let hostile = item_data(900, &long, &attributes, &[]);
        assert_eq!(hostile.len() - 4 * PAIRS, 288);
        let mut objects = Vec::new();
        let mut records = Vec::new();
        for element_id in (1_000..).take(ELEMENTS) {
            let mut object = shared_cell_record(0, [1.0, 2.0, 0.0]);
            write_u64(&mut object, 0x10, element_id);
            objects.push(object);
            records.push(auxiliary_record(
                element_id,
                1,
                &payload(&[entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &hostile)]),
            ));
        }
        let dgn = dgn_with_objects_and_auxiliary(&model_header(), &objects, &records);
        let document = read_v8(&dgn, V8ScanOptions::default()).unwrap();
        let elements = &document.models[0].elements;
        assert_eq!(elements.len(), ELEMENTS);

        let kept = elements
            .iter()
            .map(V8Element::decode_item_instances)
            .collect::<Vec<_>>();
        assert!(kept
            .iter()
            .all(|decoded| decoded.instances.len() == 1 && decoded.over_limit == 0));
        let instances = kept.iter().flat_map(|decoded| &decoded.instances);

        // The stored bytes are the item data of every auxiliary record.
        let stored = ELEMENTS * hostile.len();
        let named = instances
            .clone()
            .flat_map(|item| &item.attributes)
            .map(|pair| pair.name.len() + pair.value.len())
            .sum::<usize>();
        assert_eq!(named, ELEMENTS * PAIRS * 2 * long.len());
        assert!(named > 100 * stored);
        // The memory of all instances together is at most a fixed multiple
        // of the stored bytes.
        let memory = instances.clone().map(item_memory).sum::<usize>();
        assert!(
            memory <= MEMORY_PER_STORED_BYTE * stored,
            "{memory} bytes of memory for {stored} stored bytes"
        );
        assert!(memory < named / 15);
        // Each instance holds its table string once.
        assert!(instances
            .clone()
            .all(|item| Arc::strong_count(&item.class_name) == 1 + 2 * PAIRS));
    }

    #[test]
    fn item_data_that_does_not_decode_keeps_the_element_and_its_raw_record() {
        let good = item_data(900, "Pump", &ITEM_ATTRIBUTES, &[("Tag", "P-101")]);
        let mut truncated = good.clone();
        truncated.pop();
        let mut nested = good.clone();
        let end = nested.len() - 1;
        nested.insert(end, 0x02);
        for (case, records, expected_items) in [
            (
                "one damaged entry beside a good one",
                vec![auxiliary_record(
                    SHARED_CELL_ELEMENT_ID,
                    2,
                    &payload(&[
                        entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &truncated),
                        entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 2, true, &good),
                    ]),
                )],
                vec![2],
            ),
            (
                "a layout that was not observed",
                vec![auxiliary_record(
                    SHARED_CELL_ELEMENT_ID,
                    1,
                    &payload(&[entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &nested)]),
                )],
                vec![],
            ),
            (
                "an entry count that does not match the payload",
                vec![auxiliary_record(
                    SHARED_CELL_ELEMENT_ID,
                    2,
                    &payload(&[entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &good)]),
                )],
                vec![],
            ),
            (
                "a payload without the closing word",
                vec![auxiliary_record(
                    SHARED_CELL_ELEMENT_ID,
                    1,
                    &entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &good),
                )],
                vec![],
            ),
            (
                "an empty payload",
                vec![auxiliary_record(SHARED_CELL_ELEMENT_ID, 0, &[])],
                vec![],
            ),
        ] {
            let document = read_with_auxiliary(&records);
            let element = &document.models[0].elements[0];
            assert_eq!(element.kind(), "SHARED_CELL_INSTANCE", "{case}");
            assert_eq!(element.auxiliary_records.len(), 1, "{case}");
            assert_eq!(
                element.auxiliary_records[0].as_bytes(),
                records[0].as_slice(),
                "{case}"
            );
            assert_eq!(
                element
                    .item_instances()
                    .iter()
                    .map(|item| item.entry_id)
                    .collect::<Vec<_>>(),
                expected_items,
                "{case}"
            );
        }
    }

    #[test]
    fn every_prefix_and_mutation_of_a_file_with_item_data_is_panic_free() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let item = item_data(900, "Pump", &ITEM_ATTRIBUTES, &[("Tag", "P-101")]);
        let record = auxiliary_record(
            SHARED_CELL_ELEMENT_ID,
            1,
            &payload(&[entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &item)]),
        );
        // every single-byte change of the record, with the page rebuilt
        for offset in 0..record.len() {
            for value in [0x00, 0x01, 0x7f, 0x80, 0xff] {
                let mut mutated = record.clone();
                mutated[offset] = value;
                let dgn = dgn_with_objects_and_auxiliary(
                    &model_header(),
                    &[shared_cell_record(0, [1.0, 2.0, 0.0])],
                    &[mutated],
                );
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    let _ = read_v8(&dgn, V8ScanOptions::default());
                }));
                assert!(
                    outcome.is_ok(),
                    "panic at record offset {offset} value {value}"
                );
            }
        }
        // every prefix and sampled byte flips of the whole container
        let dgn = dgn_with_objects_and_auxiliary(
            &model_header(),
            &[shared_cell_record(0, [1.0, 2.0, 0.0])],
            &[record],
        );
        for end in (0..=dgn.len()).step_by(7) {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let _ = read_v8(&dgn[..end], V8ScanOptions::default());
            }));
            assert!(outcome.is_ok(), "panic at prefix {end}");
        }
        for offset in (0..dgn.len()).step_by(13) {
            let mut mutated = dgn.clone();
            mutated[offset] ^= 0xff;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let _ = read_v8(&mutated, V8ScanOptions::default());
            }));
            assert!(outcome.is_ok(), "panic after mutation at {offset}");
        }
    }

    /// A stored-3D type-17 text: `extents` at 0x80, `quaternion` at 0x90,
    /// origin at 0xb0, editable-field word at 0xc8, payload at 0xca.
    fn text_3d_record(extents: [f64; 2], quaternion: [f64; 4], text: &[u8]) -> Vec<u8> {
        let size = (0xca + text.len() + 1) & !1;
        let mut record = vec![0; size];
        write_u32(&mut record, 0, 0x1000_0011);
        write_u32(&mut record, 4, (size / 2) as u32);
        write_u32(&mut record, 8, (size / 2) as u32);
        write_u32(&mut record, 0x0c, 64);
        write_u64(&mut record, 0x10, 700);
        write_u64(&mut record, 0x18, MODEL_ID);
        write_u32(&mut record, 0x28, 0x0000_0800);
        record[0x6e..0x70].copy_from_slice(&(text.len() as u16).to_le_bytes());
        write_f64(&mut record, 0x70, 1000.0);
        write_f64(&mut record, 0x78, 1000.0);
        write_f64(&mut record, 0x80, extents[0]);
        write_f64(&mut record, 0x88, extents[1]);
        for (index, value) in quaternion.iter().enumerate() {
            write_f64(&mut record, 0x90 + index * 8, *value);
        }
        for (index, value) in [10.0, 20.0, 30.0].iter().enumerate() {
            write_f64(&mut record, 0xb0 + index * 8, *value);
        }
        record[0xca..0xca + text.len()].copy_from_slice(text);
        record
    }

    #[test]
    fn a_3d_text_exposes_its_stored_quaternion() {
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let dgn = dgn_with_objects(
            &model_header(),
            &[
                text_3d_record([400.0, 60.0], [half, 0.0, 0.0, -half], b"UP"),
                shared_cell_record(0, [1.0, 2.0, 0.0]),
            ],
        );
        let document = read_v8(&dgn, V8ScanOptions::default()).unwrap();
        let elements = &document.models[0].elements;
        let V8ElementData::Text {
            text,
            orientation,
            rotation_radians,
            origin,
            ..
        } = &elements[0].data
        else {
            panic!("expected a text, got {}", elements[0].kind());
        };
        assert_eq!(text, "UP");
        assert_eq!(
            (origin.uor.x, origin.uor.y, origin.uor.z),
            (10.0, 20.0, 30.0)
        );
        assert_eq!(*rotation_radians, None);
        // the six stored values stay as they were: two extents, then w, x, y, z
        assert_eq!(orientation, &[400.0, 60.0, half, 0.0, 0.0, -half]);
        assert_eq!(elements[0].data.quaternion(), Some([half, 0.0, 0.0, -half]));
        assert_eq!(elements[1].data.quaternion(), None);
    }

    const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    /// A name linkage: property 1 with UTF-16 text after the `ff fd` mark.
    fn name_linkage(name: &str) -> Vec<u8> {
        let mut payload = vec![0xff, 0xfd];
        payload.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
        let mut linkage = vec![0, 0x10, 0xd2, 0x56];
        linkage.extend(1_u32.to_le_bytes());
        linkage.extend((payload.len() as u32).to_le_bytes());
        linkage.extend(payload);
        linkage.resize(linkage.len().div_ceil(4) * 4, 0);
        linkage[0] = (linkage.len() / 2 - 1) as u8;
        linkage
    }

    /// The 0x100 primary bytes that a definition header and an instance
    /// share: the six range values and the matrix at 0x70.
    fn shared_cell_primary(
        type_and_flags: u32,
        id: u64,
        range: [f64; 6],
        matrix: [f64; 9],
    ) -> Vec<u8> {
        let mut record = vec![0; 0x100];
        write_u32(&mut record, 0, type_and_flags);
        write_u32(&mut record, 0x0c, 64);
        write_u64(&mut record, 0x10, id);
        write_u64(&mut record, 0x18, MODEL_ID);
        for (index, value) in range.iter().chain(matrix.iter()).enumerate() {
            write_f64(&mut record, 0x70 + index * 8, *value);
        }
        record
    }

    fn with_name(mut record: Vec<u8>, name: &str) -> Vec<u8> {
        let primary_words = (record.len() / 2) as u32;
        record.extend(name_linkage(name));
        let words = (record.len() / 2) as u32;
        write_u32(&mut record, 4, words);
        write_u32(&mut record, 8, primary_words);
        record
    }

    fn definition_header(id: u64, name: &str, children: u32, range: [f64; 6]) -> Vec<u8> {
        let mut record = shared_cell_primary(0x3000_0022, id, range, IDENTITY);
        write_u32(&mut record, 0x68, children);
        with_name(record, name)
    }

    fn named_instance(name: &str, range: [f64; 6], matrix: [f64; 9], origin: [f64; 3]) -> Vec<u8> {
        let mut record = shared_cell_primary(0x1000_0023, 800, range, matrix);
        for (index, value) in origin.iter().enumerate() {
            write_f64(&mut record, 0xe8 + index * 8, *value);
        }
        with_name(record, name)
    }

    /// A 2D line that is a component of the header before it.
    fn line_component(id: u64, from: [f64; 2], to: [f64; 2]) -> Vec<u8> {
        let mut record = vec![0; 0x88];
        write_u32(&mut record, 0, 0x5000_0003);
        write_u32(&mut record, 4, 0x44);
        write_u32(&mut record, 8, 0x44);
        write_u32(&mut record, 0x0c, 64);
        write_u64(&mut record, 0x10, id);
        write_u64(&mut record, 0x18, MODEL_ID);
        for (index, value) in from.iter().chain(to.iter()).enumerate() {
            write_f64(&mut record, 0x68 + index * 8, *value);
        }
        record
    }

    /// A named object that is no definition: it ends a run of components.
    fn other_named_object(id: u64) -> Vec<u8> {
        let mut record = vec![0; 0x20];
        write_u32(&mut record, 0, 0x1000_005f);
        write_u32(&mut record, 4, 0x10);
        write_u32(&mut record, 8, 0x10);
        write_u64(&mut record, 0x10, id);
        record
    }

    fn model_header_with_origin(origin: [f64; 3]) -> Vec<u8> {
        let mut stream = model_header();
        for (index, value) in origin.iter().enumerate() {
            write_f64(&mut stream[4..], 0xc8 + index * 8, *value);
        }
        stream
    }

    const SMALL: [f64; 6] = [0.0, 0.0, 0.0, 2000.0, 1000.0, 0.0];
    const LARGE: [f64; 6] = [0.0, 0.0, 0.0, 4000.0, 3000.0, 0.0];

    fn named_objects() -> Vec<Vec<u8>> {
        vec![
            other_named_object(10),
            definition_header(20, "CELL_A", 2, SMALL),
            line_component(21, [0.0, 0.0], [2000.0, 1000.0]),
            line_component(22, [2000.0, 0.0], [0.0, 1000.0]),
            definition_header(30, "CELL_A", 1, LARGE),
            line_component(31, [0.0, 0.0], [4000.0, 3000.0]),
            other_named_object(40),
            line_component(41, [1.0, 1.0], [2.0, 2.0]),
            definition_header(50, "EMPTY", 0, [0.0; 6]),
        ]
    }

    fn points(element: &V8Element) -> Vec<(f64, f64, f64, f64)> {
        let V8ElementData::Line { start, end } = &element.data else {
            panic!("expected a line, got {}", element.kind());
        };
        vec![
            (start.uor.x, start.uor.y, start.master.x, start.master.y),
            (end.uor.x, end.uor.y, end.master.x, end.master.y),
        ]
    }

    #[test]
    fn shared_cell_definitions_hold_their_components_in_their_own_space() {
        let quarter_turn = [0.0, -2.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let dgn = dgn_with_pages(
            &model_header_with_origin([500_000.0, 700_000.0, 0.0]),
            &[named_instance(
                "CELL_A",
                LARGE,
                quarter_turn,
                [9000.0, 100.0, 0.0],
            )],
            &[],
            &named_objects(),
        );
        let document = read_v8(&dgn, V8ScanOptions::default()).unwrap();
        // the model's own element is placed against the global origin
        let instance = &document.models[0].elements[0];
        let V8ElementData::SharedCellInstance {
            origin, transform, ..
        } = &instance.data
        else {
            panic!("expected an instance, got {}", instance.kind());
        };
        assert_eq!((origin.uor.x, origin.master.x), (9000.0, -491.0));

        let definitions = document
            .shared_cell_definitions(0, V8ScanOptions::default())
            .unwrap();
        assert_eq!(
            definitions
                .iter()
                .map(|definition| (definition.name.as_str(), definition.elements.len()))
                .collect::<Vec<_>>(),
            [("CELL_A", 3), ("CELL_A", 2), ("EMPTY", 1)]
        );
        let small = &definitions[0];
        let header = small.header().unwrap();
        assert_eq!(header.kind(), "SHARED_CELL_DEFINITION");
        assert_eq!(header.common.element_id, 20);
        assert_eq!((header.index, header.parent_index), (0, None));
        assert_eq!(header.child_indices, [1, 2]);
        assert_eq!(small.range_values(), Some(&SMALL[..]));
        assert_eq!(
            small.elements[1..]
                .iter()
                .map(|element| (
                    element.index,
                    element.parent_index,
                    element.common.element_id
                ))
                .collect::<Vec<_>>(),
            [(1, Some(0), 21), (2, Some(0), 22)]
        );
        // definition space: master is UOR over the model scale, no global origin
        assert_eq!(
            points(&small.elements[1]),
            [(0.0, 0.0, 0.0, 0.0), (2000.0, 1000.0, 2.0, 1.0)]
        );
        assert_eq!(definitions[2].range_values(), Some(&[0.0; 6][..]));

        // two definitions share the name; the six range values pick one
        assert!(!definitions[0].matches(&instance.data));
        assert!(definitions[1].matches(&instance.data));
        assert!(!definitions[2].matches(&instance.data));
        assert!(!definitions[1].matches(&definitions[1].elements[1].data));

        // the match is bit for bit: a zero with the other sign is another
        // value, and so is a value one step away
        let changed = |index: usize, value: f64| {
            let mut data = instance.data.clone();
            let V8ElementData::SharedCellInstance { transform, .. } = &mut data else {
                unreachable!();
            };
            transform[index] = value;
            data
        };
        assert!(definitions[1].matches(&changed(0, 0.0)));
        assert!(!definitions[1].matches(&changed(0, -0.0)));
        assert!(!definitions[1].matches(&changed(3, f64::from_bits(4000.0_f64.to_bits() + 1))));
        // the matrix is not part of the match
        assert!(definitions[1].matches(&changed(6, 7.0)));
        // an instance without a name, or with another number of values
        let mut unnamed = instance.data.clone();
        if let V8ElementData::SharedCellInstance { name, .. } = &mut unnamed {
            *name = None;
        }
        assert!(!definitions[1].matches(&unnamed));
        let mut short = instance.data.clone();
        if let V8ElementData::SharedCellInstance { transform, .. } = &mut short {
            transform.pop();
        }
        assert!(!definitions[1].matches(&short));

        // Placement: origin + M * p, with M the last nine values by rows.
        // This matrix is not symmetric: read by columns it would turn the
        // line the other way and put its far end at another point.
        assert_eq!(transform[6..], quarter_turn);
        let placed = points(&definitions[1].elements[1])
            .iter()
            .map(|(x, y, _, _)| {
                let point = instance
                    .data
                    .place_definition_point(V8Point3::new(*x, *y, 0.0))
                    .unwrap();
                (point.x, point.y, point.z)
            })
            .collect::<Vec<_>>();
        assert_eq!(placed, [(9000.0, 100.0, 0.0), (3000.0, 8100.0, 0.0)]);
        // only an instance with its fifteen values places a point
        let point = V8Point3::new(1.0, 1.0, 1.0);
        assert!(definitions[1].elements[0]
            .data
            .place_definition_point(point)
            .is_none());
        let mut short = instance.data.clone();
        if let V8ElementData::SharedCellInstance { transform, .. } = &mut short {
            transform.pop();
        }
        assert!(short.place_definition_point(point).is_none());

        assert_eq!(
            read_v8_shared_cell_definitions(&dgn, V8ScanOptions::default(), 0)
                .unwrap()
                .len(),
            3
        );
        for result in [
            document.shared_cell_definitions(1, V8ScanOptions::default()),
            read_v8_shared_cell_definitions(&dgn, V8ScanOptions::default(), 1),
        ] {
            // a wrong argument, not a malformed file
            let error = result.unwrap_err();
            assert_eq!(
                error,
                DgnError::V8ModelIndexOutOfRange { index: 1, count: 1 }
            );
            assert!(error.is_argument_error());
        }
        assert!(matches!(
            missing_model(0, 0),
            DgnError::InvalidV8ModelIndex { .. }
        ));
        assert!(!missing_model(0, 0).is_argument_error());
    }

    #[test]
    fn item_data_on_a_definition_component_is_read_with_each_definitions_call() {
        let pump = item_data(900, "Pump", &ITEM_ATTRIBUTES, &[("Tag", "P-101")]);
        let record = |element_id: u64| {
            auxiliary_record(
                element_id,
                1,
                &payload(&[entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, true, &pump)]),
            )
        };
        // Records of the named auxiliary pages: one for a component of the
        // first definition, one for the header of the second, and one for
        // an element ID that no definition holds.
        let dgn = dgn_with_named_pages(
            &model_header(),
            &[shared_cell_record(0, [1.0, 2.0, 0.0])],
            &named_objects(),
            &[record(21), record(30), record(999)],
        );
        let document = read_v8(&dgn, V8ScanOptions::default()).unwrap();
        // a plain read gives the element of the model none of them
        assert_eq!(document.models[0].elements.len(), 1);
        assert!(document.models[0].elements[0].auxiliary_records.is_empty());

        let items = |definitions: &[V8SharedCellDefinition]| {
            definitions
                .iter()
                .flat_map(|definition| &definition.elements)
                .map(|element| (element.common.element_id, element.item_instances().len()))
                .collect::<Vec<_>>()
        };
        let first = document
            .shared_cell_definitions(0, V8ScanOptions::default())
            .unwrap();
        assert_eq!(
            items(&first),
            [(20, 0), (21, 1), (22, 0), (30, 1), (31, 0), (50, 0)]
        );
        let component = &first[0].elements[1];
        assert_eq!(component.kind(), "LINE");
        let instance = &component.item_instances()[0];
        assert_eq!(&*instance.class_name, "Pump");
        assert_eq!(instance.property("Tag"), Some("P-101"));

        // Each call builds a new result that holds the records again: a
        // call uses nothing up, and two kept results hold the item data
        // twice.
        let second = document
            .shared_cell_definitions(0, V8ScanOptions::default())
            .unwrap();
        assert_eq!(items(&second), items(&first));
        assert_eq!(
            second[0].elements[1].item_instances(),
            first[0].elements[1].item_instances()
        );
        let again = read_v8_shared_cell_definitions(&dgn, V8ScanOptions::default(), 0).unwrap();
        assert_eq!(items(&again), items(&first));
    }

    #[test]
    fn a_definition_header_in_a_model_stays_an_unknown_complex_element() {
        // The same two objects in the graphical page of the model and in the
        // named page: a definition header with one line.
        let objects = |header: Vec<u8>| vec![header, line_component(21, [0.0, 0.0], [1.0, 1.0])];
        let good = definition_header(20, "CELL_A", 1, SMALL);
        let mut not_finite = good.clone();
        write_f64(&mut not_finite, 0x70, f64::NAN);
        write_f64(&mut not_finite, 0xa0, f64::INFINITY);

        for (case, header, definitions) in [("finite", good, 1), ("not finite", not_finite, 0)] {
            let run = objects(header);
            let dgn = dgn_with_pages(&model_header(), &run, &[], &run);
            // read_v8 reads the model as it did before definitions were
            // read: an unknown complex header with its child count, whatever
            // its values are
            let document = read_v8(&dgn, V8ScanOptions::default()).unwrap();
            let elements = &document.models[0].elements;
            assert_eq!(
                elements[0].data,
                V8ElementData::UnknownComplex { child_count: 1 },
                "{case}"
            );
            assert_eq!(elements[0].kind(), "UNKNOWN_COMPLEX", "{case}");
            assert_eq!(elements[0].child_indices, [1], "{case}");
            assert_eq!(elements[1].kind(), "LINE", "{case}");
            assert_eq!(elements[1].parent_index, Some(0), "{case}");
            // the definitions path reads the named copy, and a value that
            // is not finite leaves that run raw without an error
            let found = document
                .shared_cell_definitions(0, V8ScanOptions::default())
                .unwrap();
            assert_eq!(found.len(), definitions, "{case}");
        }
    }

    #[test]
    fn an_instance_places_a_definition_point_with_its_matrix_by_rows() {
        // Nine different matrix values and a point with three different
        // coordinates: each result is one row of the matrix times the point.
        let matrix = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let dgn = dgn_with_objects(
            &model_header(),
            &[named_instance("CELL_A", LARGE, matrix, [10.0, 20.0, 30.0])],
        );
        let document = read_v8(&dgn, V8ScanOptions::default()).unwrap();
        let instance = &document.models[0].elements[0].data;
        let placed = instance
            .place_definition_point(V8Point3::new(1.0, 10.0, 100.0))
            .unwrap();
        // x: 10 + 1 + 20 + 300; y: 20 + 4 + 50 + 600; z: 30 + 7 + 80 + 900.
        // By columns x would be 10 + 1 + 40 + 700.
        assert_eq!((placed.x, placed.y, placed.z), (331.0, 674.0, 1017.0));
        let origin = instance
            .place_definition_point(V8Point3::new(0.0, 0.0, 0.0))
            .unwrap();
        assert_eq!((origin.x, origin.y, origin.z), (10.0, 20.0, 30.0));
    }

    #[test]
    fn a_definition_can_hold_an_instance_of_another_definition() {
        // the nested instance is a component: the same record with that role
        let mut nested = named_instance("INNER", SMALL, IDENTITY, [100.0, 50.0, 0.0]);
        write_u32(&mut nested, 0, 0x5000_0023);
        let named = [
            definition_header(20, "OUTER", 2, LARGE),
            line_component(21, [0.0, 0.0], [4000.0, 3000.0]),
            nested,
            definition_header(30, "INNER", 1, SMALL),
            line_component(31, [0.0, 0.0], [2000.0, 1000.0]),
        ];
        let dgn = dgn_with_pages(&model_header(), &[], &[], &named);
        let definitions =
            read_v8_shared_cell_definitions(&dgn, V8ScanOptions::default(), 0).unwrap();
        assert_eq!(definitions.len(), 2);
        let outer = &definitions[0];
        assert_eq!(
            outer
                .elements
                .iter()
                .map(|element| (element.kind(), element.parent_index))
                .collect::<Vec<_>>(),
            [
                ("SHARED_CELL_DEFINITION", None),
                ("LINE", Some(0)),
                ("SHARED_CELL_INSTANCE", Some(0))
            ]
        );
        // The nested instance finds its own definition and places a point
        // of it in the space of the outer definition. The reader does not
        // expand it.
        let instance = &outer.elements[2].data;
        assert!(definitions[1].matches(instance));
        assert!(!outer.matches(instance));
        let placed = instance
            .place_definition_point(V8Point3::new(2000.0, 1000.0, 0.0))
            .unwrap();
        assert_eq!((placed.x, placed.y, placed.z), (2100.0, 1050.0, 0.0));
    }

    #[test]
    fn components_after_the_declared_count_are_not_part_of_the_definition() {
        // A text node header that is itself a component, with two children.
        let nested_header = |id: u64, children: u32| {
            let mut record = vec![0; 0x80];
            write_u32(&mut record, 0, 0x7000_0007);
            write_u32(&mut record, 4, 0x40);
            write_u32(&mut record, 8, 0x40);
            write_u32(&mut record, 0x0c, 64);
            write_u64(&mut record, 0x10, id);
            write_u64(&mut record, 0x18, MODEL_ID);
            write_u32(&mut record, 0x68, children);
            record
        };
        let line = |id: u64| line_component(id, [0.0, 0.0], [1.0, 1.0]);
        let named = [
            // one child declared, three components stored
            definition_header(20, "ONE", 1, SMALL),
            line(21),
            line(22),
            line(23),
            other_named_object(30),
            // two children declared: a line and a header with its own two
            // children; then a header and a line that no count covers
            definition_header(40, "NESTED", 2, SMALL),
            line(41),
            nested_header(42, 2),
            line(43),
            line(44),
            nested_header(45, 1),
            line(46),
            // the next definition is not affected
            definition_header(50, "NEXT", 1, SMALL),
            line(51),
        ];
        let dgn = dgn_with_pages(&model_header(), &[], &[], &named);
        let definitions =
            read_v8_shared_cell_definitions(&dgn, V8ScanOptions::default(), 0).unwrap();
        let members = |definition: &V8SharedCellDefinition| {
            definition
                .elements
                .iter()
                .map(|element| (element.common.element_id, element.parent_index))
                .collect::<Vec<_>>()
        };
        assert_eq!(definitions.len(), 3);
        assert_eq!(members(&definitions[0]), [(20, None), (21, Some(0))]);
        assert_eq!(
            members(&definitions[1]),
            [
                (40, None),
                (41, Some(0)),
                (42, Some(0)),
                (43, Some(2)),
                (44, Some(2))
            ]
        );
        assert_eq!(definitions[1].elements[2].child_indices, [3, 4]);
        assert_eq!(members(&definitions[2]), [(50, None), (51, Some(0))]);
        // every listed element but the header has a parent in the list
        for definition in &definitions {
            assert!(definition.elements[1..]
                .iter()
                .all(|element| element.parent_index.is_some()));
        }
    }

    #[test]
    fn runs_that_are_not_definitions_stay_raw() {
        let read = |named: &[Vec<u8>]| {
            let dgn = dgn_with_pages(&model_header(), &[], &[], named);
            read_v8_shared_cell_definitions(&dgn, V8ScanOptions::default(), 0)
                .unwrap()
                .iter()
                .map(|definition| {
                    (
                        definition.header().unwrap().common.element_id,
                        definition.elements[1..]
                            .iter()
                            .map(V8Element::kind)
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert!(read(&[]).is_empty());

        // a header with another primary size is not read as a definition
        let mut short = shared_cell_primary(0x3000_0022, 60, SMALL, IDENTITY);
        short.truncate(0xf8);
        write_u32(&mut short, 0x68, 1);
        // a type-34 object that is not a header
        let standalone = with_name(
            shared_cell_primary(0x1000_0022, 70, SMALL, IDENTITY),
            "LOOSE",
        );
        // a component that is too short to be a line stays an unknown child
        let mut stub = line_component(81, [0.0, 0.0], [1.0, 1.0]);
        stub.truncate(0x70);
        write_u32(&mut stub, 4, 0x38);
        write_u32(&mut stub, 8, 0x38);
        let named = [
            with_name(short, "SHORT"),
            line_component(61, [0.0, 0.0], [1.0, 1.0]),
            standalone,
            definition_header(80, "STUB", 2, SMALL),
            stub,
            line_component(82, [0.0, 0.0], [1.0, 1.0]),
            definition_header(90, "MORE", 5, SMALL),
            line_component(91, [0.0, 0.0], [1.0, 1.0]),
        ];
        assert_eq!(
            read(&named),
            [(80, vec!["UNKNOWN", "LINE"]), (90, vec!["LINE"])]
        );
    }

    #[test]
    fn every_prefix_and_mutation_of_a_file_with_definitions_is_panic_free() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let named = named_objects();
        for object in 0..named.len() {
            for offset in (0..named[object].len()).step_by(3) {
                for value in [0x00, 0x7f, 0xff] {
                    let mut mutated = named.clone();
                    mutated[object][offset] = value;
                    let dgn = dgn_with_pages(&model_header(), &[], &[], &mutated);
                    let outcome = catch_unwind(AssertUnwindSafe(|| {
                        let _ = read_v8_shared_cell_definitions(&dgn, V8ScanOptions::default(), 0);
                    }));
                    assert!(outcome.is_ok(), "panic at object {object} offset {offset}");
                }
            }
        }
        let dgn = dgn_with_pages(&model_header(), &[], &[], &named);
        for end in (0..=dgn.len()).step_by(7) {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let _ = read_v8_shared_cell_definitions(&dgn[..end], V8ScanOptions::default(), 0);
            }));
            assert!(outcome.is_ok(), "panic at prefix {end}");
        }
        // a nesting limit is an error, not a skipped definition
        let options = V8ScanOptions {
            max_hierarchy_depth: 0,
            ..V8ScanOptions::default()
        };
        assert!(matches!(
            read_v8_shared_cell_definitions(&dgn, options, 0),
            Err(DgnError::V8HierarchyDepthLimitExceeded { .. })
        ));
    }

    #[test]
    fn demotes_one_malformed_geometry_instead_of_rejecting_the_model() {
        // 本番の V8 図面: 頂点1個の線列(型4)が1つあるだけでモデル全体が読めなかった
        let fixture = include_bytes!("../../../../tests/data/dgn/v8/test_dgnv8.dgn");
        let options = V8ScanOptions::default();
        let mut raw = crate::v8::scan_v8_objects(fixture, options).unwrap();
        let model = &mut raw.models[0];
        let (page_index, object_index) = model
            .graphical_pages
            .iter()
            .enumerate()
            .find_map(|(page_index, page)| {
                page.objects
                    .iter()
                    .position(|object| object.element_type == 4 && !object.role.is_component())
                    .map(|object_index| (page_index, object_index))
            })
            .expect("fixture has a line string");
        let object = &model.graphical_pages[page_index].objects[object_index];
        let element_id = object.element_id;
        let mut bytes = object.as_bytes().to_vec();
        bytes[COMMON_HEADER_BYTES..COMMON_HEADER_BYTES + 4].copy_from_slice(&1_u32.to_le_bytes());
        let broken = object.with_object_bytes(bytes);
        model.graphical_pages[page_index].objects[object_index] = broken;

        let metadata = crate::v8::metadata::decode_model_metadata(model, options).unwrap();
        let elements = decode_model_elements(model, &metadata, options).unwrap();
        let demoted = elements
            .iter()
            .find(|element| element.raw.element_id == element_id)
            .unwrap();
        assert!(matches!(demoted.data, V8ElementData::Unknown));
        assert!(elements
            .iter()
            .any(|element| matches!(element.data, V8ElementData::LineString { .. })));
    }

    fn fixture_model() -> (crate::v8::V8RawModel, V8ModelMetadata) {
        let fixture = include_bytes!("../../../../tests/data/dgn/v8/test_dgnv8.dgn");
        let options = V8ScanOptions::default();
        let raw = crate::v8::scan_v8_objects(fixture, options).unwrap();
        let model = raw.models[0].clone();
        let metadata = crate::v8::metadata::decode_model_metadata(&model, options).unwrap();
        (model, metadata)
    }

    fn matrix_object(template: &V8RawObject, count: u32, values: &[u8]) -> V8RawObject {
        let mut bytes = vec![0_u8; MATRIX_VALUES_OFFSET];
        bytes[0x68..0x6c].copy_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(values);
        let mut object = template.with_object_bytes(bytes);
        object.attribute_words = u32::try_from(object.as_bytes().len() / 2).unwrap();
        object
    }

    fn read_double(bytes: &[u8], offset: usize) -> Option<Result<f64, DgnError>> {
        read_f64(bytes, offset).map(Ok)
    }

    #[test]
    fn decodes_matrix_values_and_rejects_truncated_payloads() {
        let (model, _) = fixture_model();
        let template = model.graphical_objects().next().unwrap().clone();
        let doubles: Vec<u8> = [1.5_f64, -2.0, 3.25]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let options = V8ScanOptions::default();
        let object = matrix_object(&template, 3, &doubles);
        let values =
            decode_matrix_values(&object, options, MATRIX_DOUBLE_BYTES, read_double).unwrap();
        assert_eq!(values, vec![1.5, -2.0, 3.25]);
        let truncated = matrix_object(&template, 4, &doubles);
        assert!(
            decode_matrix_values(&truncated, options, MATRIX_DOUBLE_BYTES, read_double).is_err()
        );
    }

    fn mesh_elements(
        template: &V8Element,
        style: u32,
        coordinates: Vec<f64>,
        indices: Vec<i32>,
    ) -> Vec<V8Element> {
        let child = |index: usize, data: V8ElementData| V8Element {
            index,
            data,
            parent_index: Some(0),
            child_indices: Vec::new(),
            ..template.clone()
        };
        vec![
            V8Element {
                index: 0,
                data: V8ElementData::Mesh {
                    child_count: 4,
                    style,
                    points: Vec::new(),
                    faces: Vec::new(),
                },
                parent_index: None,
                child_indices: vec![1, 2, 3, 4],
                ..template.clone()
            },
            child(
                1,
                V8ElementData::MatrixHeader {
                    child_count: 0,
                    columns: 3,
                },
            ),
            child(
                2,
                V8ElementData::MatrixDoubleData {
                    values: coordinates,
                },
            ),
            child(
                3,
                V8ElementData::MatrixHeader {
                    child_count: 0,
                    columns: 1,
                },
            ),
            child(4, V8ElementData::MatrixIntegerData { values: indices }),
        ]
    }

    #[test]
    fn resolves_indexed_face_loop_meshes_from_their_matrix_children() {
        let (model, metadata) = fixture_model();
        let decoded = decode_model_elements(&model, &metadata, V8ScanOptions::default()).unwrap();
        let tetrahedron = vec![
            0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0,
        ];
        let mut mesh = mesh_elements(
            &decoded[0],
            1,
            tetrahedron.clone(),
            vec![1, 2, -3, 0, 0, 1, 4, 2],
        );
        resolve_meshes(&mut mesh, &metadata, V8ScanOptions::default().max_vertices).unwrap();
        let V8ElementData::Mesh { points, faces, .. } = &mesh[0].data else {
            panic!("mesh was not resolved: {:?}", mesh[0].data);
        };
        assert_eq!(points.len(), 4);
        assert_eq!(points[1].uor, V8Point3::new(10.0, 0.0, 0.0));
        assert_eq!(points[1].master, metadata.to_master(points[1].uor));
        assert_eq!(
            faces,
            &vec![
                V8MeshFace {
                    point_indices: vec![0, 1, 2],
                    visible_edges: vec![true, true, false],
                },
                V8MeshFace {
                    point_indices: vec![0, 3, 1],
                    visible_edges: vec![true, true, true],
                },
            ]
        );

        let mut out_of_range = mesh_elements(&decoded[0], 1, tetrahedron.clone(), vec![1, 5, 2, 0]);
        resolve_meshes(
            &mut out_of_range,
            &metadata,
            V8ScanOptions::default().max_vertices,
        )
        .unwrap();
        assert!(matches!(
            out_of_range[0].data,
            V8ElementData::UnknownComplex { child_count: 4 }
        ));

        let mut other_style = mesh_elements(&decoded[0], 2, tetrahedron, vec![1, 2, 3, 0]);
        resolve_meshes(
            &mut other_style,
            &metadata,
            V8ScanOptions::default().max_vertices,
        )
        .unwrap();
        assert!(matches!(
            other_style[0].data,
            V8ElementData::UnknownComplex { child_count: 4 }
        ));
    }

    fn mesh_records() -> Vec<Vec<u8>> {
        let record = |element_type: u32, id: u64, size: usize| {
            let mut bytes = vec![0; size];
            write_u32(&mut bytes, 0, element_type);
            write_u32(&mut bytes, 4, (size / 2) as u32);
            write_u32(&mut bytes, 8, (size / 2) as u32);
            write_u64(&mut bytes, 0x10, id);
            write_u64(&mut bytes, 0x18, MODEL_ID);
            write_u32(&mut bytes, 0x28, ELEMENT_3D_FLAG);
            bytes
        };
        let mut mesh = record(0x3000_0069, 1, 0x70);
        write_u32(&mut mesh, 0x68, 4);
        write_u32(&mut mesh, 0x6c, 1);
        let mut point_header = record(0x7000_0065, 2, 0x70);
        write_u32(&mut point_header, 0x6c, 3);
        let coordinates = [
            0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, 0.0, 1000.0,
        ];
        let mut points = record(0x5000_0067, 3, 0x78 + coordinates.len() * 8);
        write_u32(&mut points, 0x68, coordinates.len() as u32);
        write_u32(&mut points, 0x6c, coordinates.len() as u32);
        write_u32(&mut points, 0x70, 49);
        for (index, value) in coordinates.iter().enumerate() {
            write_f64(&mut points, 0x78 + index * 8, *value);
        }
        let mut index_header = record(0x7000_0065, 4, 0x70);
        write_u32(&mut index_header, 0x6c, 1);
        let indices = [1_i32, 2, -3, 0, 1, 4, 2, 0];
        let mut faces = record(0x5000_0066, 5, 0x78 + indices.len() * 4);
        write_u32(&mut faces, 0x68, indices.len() as u32);
        write_u32(&mut faces, 0x6c, indices.len() as u32);
        write_u32(&mut faces, 0x70, 4386);
        for (index, value) in indices.iter().enumerate() {
            faces[0x78 + index * 4..0x7c + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        vec![mesh, point_header, points, index_header, faces]
    }

    fn mesh_dgn() -> Vec<u8> {
        dgn_with_objects(&model_header(), &mesh_records())
    }

    #[test]
    fn malformed_mesh_payloads_preserve_the_model_and_raw_children() {
        // Truncate a mesh header, matrix header, or point payload while
        // keeping each raw record's framing valid.
        for (index, size) in [(0, 0x6c), (1, 0x6c), (2, 0x80)] {
            let mut records = mesh_records();
            records[index].truncate(size);
            write_u32(&mut records[index], 4, (size / 2) as u32);
            write_u32(&mut records[index], 8, (size / 2) as u32);
            let bytes = dgn_with_objects(&model_header(), &records);
            let document = read_v8(&bytes, V8ScanOptions::default()).unwrap();
            let elements = &document.models[0].elements;
            assert_eq!(elements.len(), 5);
            assert_eq!(elements[0].child_indices, vec![1, 2, 3, 4]);
            assert!(matches!(
                elements[0].data,
                V8ElementData::UnknownComplex { .. }
            ));
            assert_eq!(elements[index].raw.as_bytes(), records[index]);
        }
    }

    #[test]
    fn reads_mesh_records_through_the_public_document_api() {
        let document = read_v8(&mesh_dgn(), V8ScanOptions::default()).unwrap();
        let elements = &document.models[0].elements;
        let V8ElementData::Mesh { points, faces, .. } = &elements[0].data else {
            panic!("mesh did not survive raw decoding and hierarchy construction");
        };
        assert_eq!(elements[0].child_indices, vec![1, 2, 3, 4]);
        assert_eq!(points[1].master, V8Point3::new(1.0, 0.0, 0.0));
        assert_eq!(faces[0].visible_edges, vec![true, true, false]);
        assert_eq!(faces[1].point_indices, vec![0, 3, 1]);
        assert!(elements[0].data.is_drawable());
        assert!(elements[1..].iter().all(|child| !child.data.is_drawable()));
    }

    #[test]
    fn mesh_points_obey_the_configured_vertex_limit() {
        let options = V8ScanOptions {
            max_vertices: 3,
            ..V8ScanOptions::default()
        };
        assert!(matches!(
            read_v8(&mesh_dgn(), options),
            Err(DgnError::V8VertexLimitExceeded {
                actual: 4,
                limit: 3,
                ..
            })
        ));
    }

    #[test]
    fn invalid_mesh_matrices_and_faces_remain_unknown() {
        let (model, metadata) = fixture_model();
        let decoded = decode_model_elements(&model, &metadata, V8ScanOptions::default()).unwrap();
        let coordinates = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        for indices in [
            vec![],
            vec![0],
            vec![1, 0],
            vec![1, 2, 0],
            vec![i32::MIN, 2, 3],
        ] {
            let mut mesh = mesh_elements(&decoded[0], 1, coordinates.clone(), indices);
            resolve_meshes(&mut mesh, &metadata, V8ScanOptions::default().max_vertices).unwrap();
            assert!(matches!(mesh[0].data, V8ElementData::UnknownComplex { .. }));
        }
        let mut mesh = mesh_elements(&decoded[0], 1, coordinates.clone(), vec![1, 2, 3, 0]);
        mesh[1].data = V8ElementData::MatrixHeader {
            child_count: 0,
            columns: 2,
        };
        resolve_meshes(&mut mesh, &metadata, V8ScanOptions::default().max_vertices).unwrap();
        assert!(matches!(mesh[0].data, V8ElementData::UnknownComplex { .. }));
        let mut mesh = mesh_elements(&decoded[0], 1, coordinates, vec![1, 2, 3, 0]);
        mesh[0].child_indices = vec![2, 4];
        resolve_meshes(&mut mesh, &metadata, V8ScanOptions::default().max_vertices).unwrap();
        assert!(matches!(mesh[0].data, V8ElementData::UnknownComplex { .. }));
    }

    #[test]
    fn range_high_is_the_low_corner_plus_the_stored_extent() {
        let fixture = include_bytes!("../../../../tests/data/dgn/v8/test_dgnv8.dgn");
        let options = V8ScanOptions::default();
        let raw = crate::v8::scan_v8_objects(fixture, options).unwrap();
        let metadata = crate::v8::metadata::decode_model_metadata(&raw.models[0], options).unwrap();
        let elements = decode_model_elements(&raw.models[0], &metadata, options).unwrap();
        let mut checked = 0;
        for element in &elements {
            let vertices: Vec<V8Point3> = match &element.data {
                V8ElementData::Line { start, end } => vec![start.uor, end.uor],
                V8ElementData::LineString { vertices } | V8ElementData::Shape { vertices } => {
                    vertices.iter().map(|vertex| vertex.uor).collect()
                }
                _ => continue,
            };
            let max_x = vertices
                .iter()
                .map(|point| point.x)
                .fold(f64::MIN, f64::max);
            let max_y = vertices
                .iter()
                .map(|point| point.y)
                .fold(f64::MIN, f64::max);
            let high = element.common.range_uor.high;
            assert!((high[0] as f64 - max_x).abs() <= 2.0, "{high:?} vs {max_x}");
            assert!((high[1] as f64 - max_y).abs() <= 2.0, "{high:?} vs {max_y}");
            checked += 1;
        }
        assert!(checked > 10);
    }

    #[test]
    fn decodes_text_escape_marker_without_losing_source_bytes() {
        let raw = [0xff, 0xfe, 1, 0, b'm', b'y', b'T', 0xe9, b'x', b't'];
        let decoded = decode_windows_1252(&raw[4..]);
        assert_eq!(decoded, "myTéxt");
    }

    #[test]
    fn shared_cell_instance_origin_keeps_three_components_without_the_3d_flag() {
        let element = read_shared_cell(0, [1000.0, 2000.0, 0.0]);
        assert_eq!(element.common.stored_dimension, V8Dimension::Two);
        let V8ElementData::SharedCellInstance {
            origin,
            transform,
            name,
        } = &element.data
        else {
            panic!("expected a shared-cell instance, got {:?}", element.data);
        };
        assert_eq!(origin.uor.as_array(), [1000.0, 2000.0, 0.0]);
        assert_eq!(origin.master.as_array(), [1.0, 2.0, 0.0]);
        assert_eq!(transform.as_slice(), &SHARED_CELL_TRANSFORM_VALUES);
        assert_eq!(name, &None);
    }

    #[test]
    fn shared_cell_instance_origin_reads_the_same_bytes_with_the_3d_flag() {
        let element = read_shared_cell(ELEMENT_3D_FLAG, [1000.0, 2000.0, 3000.0]);
        assert_eq!(element.common.stored_dimension, V8Dimension::Three);
        let V8ElementData::SharedCellInstance {
            origin, transform, ..
        } = &element.data
        else {
            panic!("expected a shared-cell instance, got {:?}", element.data);
        };
        assert_eq!(origin.uor.as_array(), [1000.0, 2000.0, 3000.0]);
        assert_eq!(transform.len(), SHARED_CELL_TRANSFORM_VALUES.len());
    }
}

#[cfg(test)]
mod utf16_text_tests {
    use super::{decode_utf16_payload, utf16_byte_order, V8TextEncoding};

    fn marked(mark: [u8; 2], text: &str, big_endian: bool) -> Vec<u8> {
        let mut out = mark.to_vec();
        for unit in text.encode_utf16() {
            out.extend(if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        out
    }

    #[test]
    fn byte_order_marked_utf16_is_recognised() {
        let latvian = marked([0xff, 0xfe], "Pārskata plāns", false);
        assert_eq!(utf16_byte_order(&latvian), Some(V8TextEncoding::Utf16Le));
        assert_eq!(
            decode_utf16_payload(&latvian[2..], V8TextEncoding::Utf16Le).as_deref(),
            Some("Pārskata plāns")
        );
        let mut czech = marked([0xff, 0xfe], "Čerpací šachta", false);
        czech.push(0); // a trailing NUL is tolerated
        assert_eq!(utf16_byte_order(&czech), Some(V8TextEncoding::Utf16Le));
        assert_eq!(
            decode_utf16_payload(&czech[2..], V8TextEncoding::Utf16Le).as_deref(),
            Some("Čerpací šachta")
        );
        let plan = marked([0xfe, 0xff], "Plan", true);
        assert_eq!(utf16_byte_order(&plan), Some(V8TextEncoding::Utf16Be));
        assert_eq!(
            decode_utf16_payload(&plan[2..], V8TextEncoding::Utf16Be).as_deref(),
            Some("Plan")
        );
    }

    #[test]
    fn code_page_bytes_that_merely_start_with_ff_fe_are_not_utf16() {
        assert_eq!(
            utf16_byte_order(&[0xff, 0xfe, 0xe4, 0x04, b'A', b'B', b'C']),
            None
        ); // odd
        assert_eq!(
            utf16_byte_order(&[0xff, 0xfe, b'A', b'B', 0x07, 0x00]),
            None
        ); // control
        assert_eq!(utf16_byte_order(&[0xff, 0xfe, 1, 0, b'm', b'y']), None); // escaped layout
        assert_eq!(utf16_byte_order(b"plain"), None);
        assert_eq!(utf16_byte_order(&[0xff]), None);
    }
}
