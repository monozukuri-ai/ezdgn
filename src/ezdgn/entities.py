"""High-level, lossless V7 2D CAD object model."""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from typing import ClassVar, Iterator, TypeAlias, cast

from ._core import (
    DEFAULT_MAX_FILE_SIZE_BYTES,
    DEFAULT_MAX_RECORDS,
    MAX_V7_RECORD_SIZE_BYTES,
)
from ._core import read_v7_2d_primitives as _read_v7_2d_primitives
from .metadata import (
    CommonElementHeader,
    DesignSettings,
    ElementMetadata,
    HeaderScanRow,
    _header_scan_from_core,
    _load_data,
)
from .raw import DgnSource, PathSource, RawElement, RawScan

Point2Uor: TypeAlias = tuple[int, int]
Point2Raw: TypeAlias = tuple[float, float]
Point2Master: TypeAlias = tuple[float, float]
RgbColor: TypeAlias = tuple[int, int, int]

LineRow: TypeAlias = tuple[
    int,
    Point2Uor,
    Point2Uor,
    tuple[Point2Raw, Point2Raw],
    tuple[Point2Master, Point2Master] | None,
]
MultiPointRow: TypeAlias = tuple[
    int,
    list[Point2Uor],
    list[Point2Raw],
    list[Point2Master] | None,
]
EllipseRow: TypeAlias = tuple[
    int,
    Point2Raw,
    Point2Master | None,
    tuple[float, float],
    tuple[float, float] | None,
    tuple[int, float],
]
ArcRow: TypeAlias = tuple[
    int,
    Point2Raw,
    Point2Master | None,
    tuple[float, float],
    tuple[float, float] | None,
    tuple[int, float],
    tuple[int, float],
    tuple[int, float],
]
TextRow: TypeAlias = tuple[
    int,
    tuple[int, int],
    tuple[int, int],
    tuple[float, float] | None,
    tuple[int, float],
    Point2Uor,
    Point2Master | None,
    tuple[int, int, int],
]
ColorTableRow: TypeAlias = tuple[int, int, list[RgbColor]]
CellRow: TypeAlias = tuple[
    int,
    tuple[int, tuple[int, int], str],
    tuple[int, tuple[int, int, int, int]],
    tuple[Point2Uor, Point2Uor],
    tuple[Point2Master, Point2Master] | None,
    tuple[tuple[int, int], tuple[int, int]],
    tuple[tuple[float, float], tuple[float, float]],
    Point2Uor,
    Point2Master | None,
]
TextNodeRow: TypeAlias = tuple[
    int,
    tuple[int, int, int],
    tuple[int, int, int, int],
    tuple[int, float | None],
    tuple[int, int],
    tuple[float, float] | None,
    tuple[int, float],
    Point2Uor,
    Point2Master | None,
]
ComplexRow: TypeAlias = tuple[int, int, int]
BSplineSurfaceRow: TypeAlias = tuple[
    int,
    tuple[int, int],
    tuple[int, int, int, int, int],
    tuple[int, int, int, int, int],
    int,
]
BSplineBoundaryRow: TypeAlias = tuple[
    int,
    int,
    list[Point2Uor],
    list[Point2Raw],
    list[Point2Raw],
]
BSplineScalarRow: TypeAlias = tuple[int, list[int], list[float]]
BSplineCurveRow: TypeAlias = tuple[int, int, int, int, int, int, int]
BSplineRows: TypeAlias = tuple[
    list[MultiPointRow],
    list[BSplineSurfaceRow],
    list[BSplineBoundaryRow],
    list[BSplineScalarRow],
    list[BSplineCurveRow],
    list[BSplineScalarRow],
]
HierarchyRow: TypeAlias = tuple[int | None, list[int]]
HighPrecisionRow: TypeAlias = tuple[int, list[tuple[int, int]], bool]
LinkageRow: TypeAlias = tuple[
    int,
    int,
    int | None,
    int | None,
    str,
    int | None,
    int | None,
    int | None,
    int | None,
    HighPrecisionRow | None,
]
Phase4Row: TypeAlias = tuple[
    list[MultiPointRow],
    list[CellRow],
    list[TextNodeRow],
    list[ComplexRow],
    list[ComplexRow],
    BSplineRows,
    list[HierarchyRow],
    list[list[LinkageRow]],
]
PrimitiveScanRow: TypeAlias = tuple[
    HeaderScanRow,
    list[LineRow],
    list[MultiPointRow],
    list[MultiPointRow],
    list[EllipseRow],
    list[ArcRow],
    list[TextRow],
    list[ColorTableRow],
    int | None,
    Phase4Row,
]


@dataclass(frozen=True, slots=True)
class BasicStyle:
    """Common V7 symbology plus an optional resolved RGB color."""

    color_index: int
    line_style: int
    line_weight: int
    rgb: RgbColor | None
    fill_color_index: int | None = None
    fill_rgb: RgbColor | None = None


_LINKAGE_TYPE_NAMES = {
    0x0000: "DMRS",
    0x0041: "SHAPE_FILL",
    0x1971: "XBASE",
    0x3848: "INFORMIX",
    0x4F58: "SYBASE",
    0x51A9: "HIGH_PRECISION",
    0x5E62: "ODBC",
    0x6091: "ORACLE",
    0x71FB: "RIS",
    0x7D2F: "ASSOCIATION_ID",
}


@dataclass(frozen=True, slots=True)
class AttributeLinkage:
    """One bounded attribute linkage with typed fields and exact raw bytes."""

    offset: int
    declared_size: int | None
    linkage_type: int | None
    kind: str
    entity_number: int | None
    mslink: int | None
    color_index: int | None
    association_id: int | None
    delta_words: int | None
    deltas: tuple[tuple[int, int], ...]
    is_complete: bool
    _raw_view: memoryview = field(repr=False)

    @property
    def linkage_type_name(self) -> str | None:
        if self.linkage_type is None:
            return None
        return _LINKAGE_TYPE_NAMES.get(
            self.linkage_type, f"USER_0x{self.linkage_type:04X}"
        )

    @property
    def raw_view(self) -> memoryview:
        return self._raw_view

    @property
    def raw_bytes(self) -> bytes:
        return self._raw_view.tobytes()


@dataclass(frozen=True, slots=True)
class DgnElement:
    """Base record shared by supported and unsupported semantic elements."""

    record: RawElement
    common_header: CommonElementHeader | None
    style: BasicStyle | None
    parent_index: int | None = field(default=None, init=False)
    child_indices: tuple[int, ...] = field(default=(), init=False)
    linkages: tuple[AttributeLinkage, ...] = field(
        default=(), init=False, repr=False
    )

    KIND: ClassVar[str] = "ELEMENT"

    @property
    def kind(self) -> str:
        return type(self).KIND

    def dxftype(self) -> str:
        """Return an ezdxf-style stable type name for querying."""

        return self.kind

    @property
    def is_component(self) -> bool:
        return self.parent_index is not None

    @property
    def has_children(self) -> bool:
        return bool(self.child_indices)

    @property
    def association_ids(self) -> tuple[int, ...]:
        return tuple(
            linkage.association_id
            for linkage in self.linkages
            if linkage.association_id is not None
        )

    @property
    def attribute_view(self) -> memoryview | None:
        """Zero-copy attribute bytes, when the common A-bit is set."""

        header = self.common_header
        if header is None or header.attribute_offset is None:
            return None
        start = header.attribute_offset
        return self.record.raw_view[start : start + header.attribute_length]


@dataclass(frozen=True, slots=True)
class Cell(DgnElement):
    total_length_words: int
    name_words: tuple[int, int]
    name: str
    cell_class: int
    levels: tuple[int, int, int, int]
    range_low_uor: Point2Uor
    range_high_uor: Point2Uor
    range_low_master: Point2Master | None
    range_high_master: Point2Master | None
    transform_raw: tuple[tuple[int, int], tuple[int, int]]
    transform: tuple[tuple[float, float], tuple[float, float]]
    origin_uor: Point2Uor
    origin_master: Point2Master | None

    KIND: ClassVar[str] = "CELL"


@dataclass(frozen=True, slots=True)
class Line(DgnElement):
    start_uor: Point2Uor
    end_uor: Point2Uor
    start_uor_precise: Point2Raw
    end_uor_precise: Point2Raw
    start_master: Point2Master | None
    end_master: Point2Master | None

    KIND: ClassVar[str] = "LINE"


@dataclass(frozen=True, slots=True)
class LineString(DgnElement):
    vertices_uor: tuple[Point2Uor, ...]
    vertices_uor_precise: tuple[Point2Raw, ...]
    vertices_master: tuple[Point2Master, ...] | None

    KIND: ClassVar[str] = "LINE_STRING"


@dataclass(frozen=True, slots=True)
class Shape(DgnElement):
    vertices_uor: tuple[Point2Uor, ...]
    vertices_uor_precise: tuple[Point2Raw, ...]
    vertices_master: tuple[Point2Master, ...] | None

    KIND: ClassVar[str] = "SHAPE"

    @property
    def is_closed(self) -> bool:
        """A shape is closed by element semantics, without adding a vertex."""

        return True

    @property
    def has_repeated_closing_vertex(self) -> bool:
        return bool(self.vertices_uor) and (
            self.vertices_uor[0] == self.vertices_uor[-1]
        )


@dataclass(frozen=True, slots=True)
class TextNode(DgnElement):
    total_length_words: int
    num_text_strings: int
    node_number: int
    max_length: int
    max_used: int
    font_id: int
    justification: int
    line_spacing_raw: int
    line_spacing_master: float | None
    length_multiplier_raw: int
    height_multiplier_raw: int
    length_multiplier_master: float | None
    height_multiplier_master: float | None
    rotation_raw: int
    rotation_degrees: float
    origin_uor: Point2Uor
    origin_master: Point2Master | None

    KIND: ClassVar[str] = "TEXT_NODE"


@dataclass(frozen=True, slots=True)
class Curve(DgnElement):
    """Native type-11 parametric curve control sequence."""

    vertices_uor: tuple[Point2Uor, ...]
    vertices_uor_precise: tuple[Point2Raw, ...]
    vertices_master: tuple[Point2Master, ...] | None

    KIND: ClassVar[str] = "CURVE"


@dataclass(frozen=True, slots=True)
class ComplexElement(DgnElement):
    total_length_words: int
    num_elements: int


@dataclass(frozen=True, slots=True)
class ComplexChain(ComplexElement):
    KIND: ClassVar[str] = "COMPLEX_CHAIN"


@dataclass(frozen=True, slots=True)
class ComplexShape(ComplexElement):
    KIND: ClassVar[str] = "COMPLEX_SHAPE"


@dataclass(frozen=True, slots=True)
class Ellipse(DgnElement):
    center_uor: Point2Raw
    center_master: Point2Master | None
    primary_axis_uor: float
    secondary_axis_uor: float
    primary_axis_master: float | None
    secondary_axis_master: float | None
    rotation_raw: int
    rotation_degrees: float

    KIND: ClassVar[str] = "ELLIPSE"


@dataclass(frozen=True, slots=True)
class Arc(DgnElement):
    center_uor: Point2Raw
    center_master: Point2Master | None
    primary_axis_uor: float
    secondary_axis_uor: float
    primary_axis_master: float | None
    secondary_axis_master: float | None
    rotation_raw: int
    rotation_degrees: float
    start_angle_raw: int
    start_angle_degrees: float
    sweep_angle_raw: int
    sweep_angle_degrees: float

    KIND: ClassVar[str] = "ARC"


@dataclass(frozen=True, slots=True)
class Text(DgnElement):
    font_id: int
    justification: int
    length_multiplier_raw: int
    height_multiplier_raw: int
    length_multiplier_master: float | None
    height_multiplier_master: float | None
    rotation_raw: int
    rotation_degrees: float
    origin_uor: Point2Uor
    origin_master: Point2Master | None
    editable_fields: int
    _text_view: memoryview = field(repr=False)

    KIND: ClassVar[str] = "TEXT"

    @property
    def text_view(self) -> memoryview:
        """Read-only zero-copy view of the encoded text payload."""

        return self._text_view

    @property
    def text_bytes(self) -> bytes:
        return self._text_view.tobytes()

    def decode_text(self, encoding: str, errors: str = "strict") -> str:
        """Decode text using an encoding explicitly selected by the caller."""

        return self.text_bytes.decode(encoding, errors)


@dataclass(frozen=True, slots=True)
class BSplinePole(DgnElement):
    vertices_uor: tuple[Point2Uor, ...]
    vertices_uor_precise: tuple[Point2Raw, ...]
    vertices_master: tuple[Point2Master, ...] | None

    KIND: ClassVar[str] = "BSPLINE_POLE"


@dataclass(frozen=True, slots=True)
class BSplineSurface(DgnElement):
    description_words: int
    curve_type: int
    u_order: int
    u_properties: int
    num_poles_u: int
    num_knots_u: int
    rule_lines_u: int
    v_order: int
    v_properties: int
    num_poles_v: int
    num_knots_v: int
    rule_lines_v: int
    num_bounds: int

    KIND: ClassVar[str] = "BSPLINE_SURFACE"

    @property
    def is_rational(self) -> bool:
        return bool(self.u_properties & 0x40)

    @property
    def is_u_closed(self) -> bool:
        return bool(self.u_properties & 0x80)

    @property
    def is_v_closed(self) -> bool:
        return bool(self.v_properties & 0x80)


@dataclass(frozen=True, slots=True)
class BSplineSurfaceBoundary(DgnElement):
    number: int
    vertices_raw: tuple[Point2Uor, ...]
    vertices_raw_precise: tuple[Point2Raw, ...]
    vertices_uv: tuple[Point2Raw, ...]

    KIND: ClassVar[str] = "BSPLINE_SURFACE_BOUNDARY"


@dataclass(frozen=True, slots=True)
class BSplineKnot(DgnElement):
    values_raw: tuple[int, ...]
    values: tuple[float, ...]

    KIND: ClassVar[str] = "BSPLINE_KNOT"


@dataclass(frozen=True, slots=True)
class BSplineCurve(DgnElement):
    description_words: int
    order: int
    properties: int
    curve_type: int
    num_poles: int
    num_knots: int

    KIND: ClassVar[str] = "BSPLINE_CURVE"

    @property
    def curve_display(self) -> bool:
        return bool(self.properties & 0x10)

    @property
    def polygon_display(self) -> bool:
        return bool(self.properties & 0x20)

    @property
    def is_rational(self) -> bool:
        return bool(self.properties & 0x40)

    @property
    def is_closed(self) -> bool:
        return bool(self.properties & 0x80)


@dataclass(frozen=True, slots=True)
class BSplineWeight(DgnElement):
    values_raw: tuple[int, ...]
    values: tuple[float, ...]

    KIND: ClassVar[str] = "BSPLINE_WEIGHT"


@dataclass(frozen=True, slots=True)
class ColorTable(DgnElement):
    screen_flag: int
    colors: tuple[RgbColor, ...]

    KIND: ClassVar[str] = "COLOR_TABLE"

    def color(self, index: int) -> RgbColor:
        if not 0 <= index < 256:
            raise IndexError("DGN color index must be between 0 and 255")
        return self.colors[index]

    def __getitem__(self, index: int) -> RgbColor:
        return self.color(index)


@dataclass(frozen=True, slots=True)
class UnsupportedElement(DgnElement):
    """A control or not-yet-decoded record with raw bytes intact."""

    KIND: ClassVar[str] = "UNSUPPORTED"


GraphicElement: TypeAlias = (
    Cell
    | Line
    | LineString
    | Shape
    | TextNode
    | Curve
    | ComplexChain
    | ComplexShape
    | Ellipse
    | Arc
    | Text
    | BSplineSurface
    | BSplineCurve
)

_GRAPHIC_TYPES = (
    Cell,
    Line,
    LineString,
    Shape,
    TextNode,
    Curve,
    ComplexChain,
    ComplexShape,
    Ellipse,
    Arc,
    Text,
    BSplineSurface,
    BSplineCurve,
)


@dataclass(frozen=True, slots=True)
class Drawing:
    """Ordered V7 2D document returned by :func:`read` and `readfile`."""

    raw_scan: RawScan
    design_settings: DesignSettings
    elements: tuple[DgnElement, ...]
    active_color_table_index: int | None

    @property
    def entities(self) -> tuple[GraphicElement, ...]:
        """Top-level drawable entities without flattening complex children."""

        return tuple(
            cast(GraphicElement, element)
            for element in self.elements
            if element.parent_index is None
            and isinstance(element, _GRAPHIC_TYPES)
        )

    @property
    def all_entities(self) -> tuple[GraphicElement, ...]:
        """All drawable records, including components nested in containers."""

        return tuple(
            cast(GraphicElement, element)
            for element in self.elements
            if isinstance(element, _GRAPHIC_TYPES)
        )

    @property
    def root_elements(self) -> tuple[DgnElement, ...]:
        return tuple(
            element
            for element in self.elements
            if element.parent_index is None
        )

    @property
    def unsupported_elements(self) -> tuple[UnsupportedElement, ...]:
        return tuple(
            element
            for element in self.elements
            if isinstance(element, UnsupportedElement)
        )

    @property
    def color_table(self) -> ColorTable | None:
        index = self.active_color_table_index
        if index is None:
            return None
        element = self.elements[index]
        if not isinstance(element, ColorTable):
            raise RuntimeError("native reader returned an invalid color-table index")
        return element

    def resolve_color(self, index: int) -> RgbColor | None:
        table = self.color_table
        return None if table is None else table.color(index)

    def query(self, kind: str) -> tuple[DgnElement, ...]:
        """Return elements matching a stable type name such as ``LINE``."""

        normalized = kind.strip().upper().replace("-", "_").replace(" ", "_")
        return tuple(element for element in self.elements if element.kind == normalized)

    def parent(self, element: DgnElement | int) -> DgnElement | None:
        """Return the direct complex parent for an element or record index."""

        resolved = self._resolve_element(element)
        index = resolved.parent_index
        return None if index is None else self.elements[index]

    def children(self, element: DgnElement | int) -> tuple[DgnElement, ...]:
        """Return direct children in original file order."""

        resolved = self._resolve_element(element)
        return tuple(self.elements[index] for index in resolved.child_indices)

    def descendants(self, element: DgnElement | int) -> tuple[DgnElement, ...]:
        """Return all descendants in depth-first file order."""

        result: list[DgnElement] = []
        pending = list(reversed(self._resolve_element(element).child_indices))
        while pending:
            index = pending.pop()
            child = self.elements[index]
            result.append(child)
            pending.extend(reversed(child.child_indices))
        return tuple(result)

    def _resolve_element(self, element: DgnElement | int) -> DgnElement:
        index = element if isinstance(element, int) else element.record.index
        try:
            resolved = self.elements[index]
        except (IndexError, TypeError) as error:
            raise IndexError(f"element index out of range: {index}") from error
        if not isinstance(element, int) and resolved is not element:
            raise ValueError("element does not belong to this drawing")
        return resolved

    def __iter__(self) -> Iterator[GraphicElement]:
        return iter(self.entities)


def read(
    source: DgnSource,
    *,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
    max_records: int = DEFAULT_MAX_RECORDS,
    max_record_size: int = MAX_V7_RECORD_SIZE_BYTES,
) -> Drawing:
    """Read a V7 2D source while retaining every ordered raw record."""

    data = _load_data(source, max_file_size, max_records, max_record_size)
    row = cast(
        PrimitiveScanRow,
        _read_v7_2d_primitives(
            data,
            max_file_size,
            max_records,
            max_record_size,
        ),
    )
    return _drawing_from_core(data, row)


def readfile(
    path: PathSource,
    *,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
    max_records: int = DEFAULT_MAX_RECORDS,
    max_record_size: int = MAX_V7_RECORD_SIZE_BYTES,
) -> Drawing:
    """Read a filesystem path using the high-level 2D object model."""

    if not isinstance(path, (str, os.PathLike)):
        raise TypeError("readfile() requires a filesystem path")
    return read(
        path,
        max_file_size=max_file_size,
        max_records=max_records,
        max_record_size=max_record_size,
    )


@dataclass(slots=True)
class _SemanticRows:
    cells: dict[int, CellRow]
    lines: dict[int, LineRow]
    line_strings: dict[int, MultiPointRow]
    shapes: dict[int, MultiPointRow]
    text_nodes: dict[int, TextNodeRow]
    curves: dict[int, MultiPointRow]
    complex_chains: dict[int, ComplexRow]
    complex_shapes: dict[int, ComplexRow]
    ellipses: dict[int, EllipseRow]
    arcs: dict[int, ArcRow]
    texts: dict[int, TextRow]
    bspline_poles: dict[int, MultiPointRow]
    bspline_surfaces: dict[int, BSplineSurfaceRow]
    bspline_boundaries: dict[int, BSplineBoundaryRow]
    bspline_knots: dict[int, BSplineScalarRow]
    bspline_curves: dict[int, BSplineCurveRow]
    bspline_weights: dict[int, BSplineScalarRow]
    color_tables: dict[int, ColorTableRow]

    def dictionaries(self) -> tuple[dict[int, object], ...]:
        return cast(
            tuple[dict[int, object], ...],
            tuple(getattr(self, name) for name in self.__slots__),
        )


def _drawing_from_core(data: bytes, row: PrimitiveScanRow) -> Drawing:
    (
        header_row,
        line_rows,
        line_string_rows,
        shape_rows,
        ellipse_rows,
        arc_rows,
        text_rows,
        color_table_rows,
        active_color_table_index,
        phase4,
    ) = row
    (
        curve_rows,
        cell_rows,
        text_node_rows,
        complex_chain_rows,
        complex_shape_rows,
        bspline_rows,
        hierarchy_rows,
        linkage_rows,
    ) = phase4
    (
        bspline_pole_rows,
        bspline_surface_rows,
        bspline_boundary_rows,
        bspline_knot_rows,
        bspline_curve_rows,
        bspline_weight_rows,
    ) = bspline_rows
    headers = _header_scan_from_core(data, header_row)

    rows = _SemanticRows(
        cells={item[0]: item for item in cell_rows},
        lines={item[0]: item for item in line_rows},
        line_strings={item[0]: item for item in line_string_rows},
        shapes={item[0]: item for item in shape_rows},
        text_nodes={item[0]: item for item in text_node_rows},
        curves={item[0]: item for item in curve_rows},
        complex_chains={item[0]: item for item in complex_chain_rows},
        complex_shapes={item[0]: item for item in complex_shape_rows},
        ellipses={item[0]: item for item in ellipse_rows},
        arcs={item[0]: item for item in arc_rows},
        texts={item[0]: item for item in text_rows},
        bspline_poles={item[0]: item for item in bspline_pole_rows},
        bspline_surfaces={item[0]: item for item in bspline_surface_rows},
        bspline_boundaries={item[0]: item for item in bspline_boundary_rows},
        bspline_knots={item[0]: item for item in bspline_knot_rows},
        bspline_curves={item[0]: item for item in bspline_curve_rows},
        bspline_weights={item[0]: item for item in bspline_weight_rows},
        color_tables={item[0]: item for item in color_table_rows},
    )
    semantic_indices: set[int] = set()
    expected_semantic_count = 0
    for items in rows.dictionaries():
        semantic_indices.update(items)
        expected_semantic_count += len(items)
    if len(semantic_indices) != expected_semantic_count:
        raise RuntimeError("native entity scan returned duplicate element indices")

    active_colors: tuple[RgbColor, ...] | None = None
    if active_color_table_index is not None:
        try:
            active_row = rows.color_tables[active_color_table_index]
        except KeyError as error:
            raise RuntimeError(
                "native reader returned an invalid color-table index"
            ) from error
        active_colors = tuple(active_row[2])

    record_count = len(headers.elements)
    if len(hierarchy_rows) != record_count or len(linkage_rows) != record_count:
        raise RuntimeError("native hierarchy/linkage rows are not record-aligned")
    linkages = tuple(
        _linkages_from_rows(metadata.record, native_rows)
        for metadata, native_rows in zip(
            headers.elements, linkage_rows, strict=True
        )
    )
    elements = tuple(
        _element_from_rows(metadata, active_colors, linkages[index], rows)
        for index, metadata in enumerate(headers.elements)
    )
    for index, (parent_index, child_indices) in enumerate(hierarchy_rows):
        if parent_index is not None and not 0 <= parent_index < record_count:
            raise RuntimeError("native hierarchy contains an invalid parent index")
        if any(not 0 <= child < record_count for child in child_indices):
            raise RuntimeError("native hierarchy contains an invalid child index")
        object.__setattr__(elements[index], "parent_index", parent_index)
        object.__setattr__(elements[index], "child_indices", tuple(child_indices))
        object.__setattr__(elements[index], "linkages", linkages[index])

    return Drawing(
        raw_scan=headers.raw_scan,
        design_settings=headers.design_settings,
        elements=elements,
        active_color_table_index=active_color_table_index,
    )


def _linkages_from_rows(
    record: RawElement, rows: list[LinkageRow]
) -> tuple[AttributeLinkage, ...]:
    result: list[AttributeLinkage] = []
    for row in rows:
        (
            offset,
            raw_size,
            declared_size,
            linkage_type,
            kind,
            entity_number,
            mslink,
            color_index,
            association_id,
            high_precision,
        ) = row
        if offset < 0 or raw_size < 0 or offset + raw_size > record.size_bytes:
            raise RuntimeError("native linkage row exceeds its element record")
        delta_words = None
        deltas: tuple[tuple[int, int], ...] = ()
        is_complete = kind != "UNPARSED"
        if high_precision is not None:
            delta_words, native_deltas, is_complete = high_precision
            deltas = tuple(native_deltas)
        result.append(
            AttributeLinkage(
                offset=offset,
                declared_size=declared_size,
                linkage_type=linkage_type,
                kind=kind,
                entity_number=entity_number,
                mslink=mslink,
                color_index=color_index,
                association_id=association_id,
                delta_words=delta_words,
                deltas=deltas,
                is_complete=is_complete,
                _raw_view=record.raw_view[offset : offset + raw_size],
            )
        )
    return tuple(result)


def _element_from_rows(
    metadata: ElementMetadata,
    active_colors: tuple[RgbColor, ...] | None,
    linkages: tuple[AttributeLinkage, ...],
    rows: _SemanticRows,
) -> DgnElement:
    index = metadata.record.index
    base = (metadata.record, metadata.common_header)
    style = _basic_style(metadata.common_header, active_colors, linkages)

    if index in rows.cells:
        (
            _,
            description,
            class_levels,
            range_uor,
            range_master,
            transform_raw,
            transform,
            origin_uor,
            origin_master,
        ) = rows.cells[index]
        low_master, high_master = (
            (None, None) if range_master is None else range_master
        )
        return Cell(
            *base,
            style,
            description[0],
            description[1],
            description[2],
            class_levels[0],
            class_levels[1],
            range_uor[0],
            range_uor[1],
            low_master,
            high_master,
            transform_raw,
            transform,
            origin_uor,
            origin_master,
        )
    if index in rows.lines:
        _, start_uor, end_uor, precise, master = rows.lines[index]
        start_master, end_master = (None, None) if master is None else master
        return Line(
            *base,
            style,
            start_uor,
            end_uor,
            precise[0],
            precise[1],
            start_master,
            end_master,
        )
    if index in rows.line_strings:
        return _multipoint_entity(LineString, base, style, rows.line_strings[index])
    if index in rows.shapes:
        return _multipoint_entity(Shape, base, style, rows.shapes[index])
    if index in rows.text_nodes:
        (
            _,
            description,
            text_settings,
            line_spacing,
            multipliers_raw,
            multipliers_master,
            rotation,
            origin_uor,
            origin_master,
        ) = rows.text_nodes[index]
        length_master, height_master = (
            (None, None)
            if multipliers_master is None
            else multipliers_master
        )
        return TextNode(
            *base,
            style,
            description[0],
            description[1],
            description[2],
            text_settings[0],
            text_settings[1],
            text_settings[2],
            text_settings[3],
            line_spacing[0],
            line_spacing[1],
            multipliers_raw[0],
            multipliers_raw[1],
            length_master,
            height_master,
            rotation[0],
            rotation[1],
            origin_uor,
            origin_master,
        )
    if index in rows.curves:
        return _multipoint_entity(Curve, base, style, rows.curves[index])
    if index in rows.complex_chains:
        _, total, count = rows.complex_chains[index]
        return ComplexChain(*base, style, total, count)
    if index in rows.complex_shapes:
        _, total, count = rows.complex_shapes[index]
        return ComplexShape(*base, style, total, count)
    if index in rows.ellipses:
        _, center_uor, center_master, axes_uor, axes_master, rotation = rows.ellipses[
            index
        ]
        primary_master, secondary_master = (
            (None, None) if axes_master is None else axes_master
        )
        return Ellipse(
            *base,
            style,
            center_uor,
            center_master,
            axes_uor[0],
            axes_uor[1],
            primary_master,
            secondary_master,
            rotation[0],
            rotation[1],
        )
    if index in rows.arcs:
        (
            _,
            center_uor,
            center_master,
            axes_uor,
            axes_master,
            rotation,
            start_angle,
            sweep_angle,
        ) = rows.arcs[index]
        primary_master, secondary_master = (
            (None, None) if axes_master is None else axes_master
        )
        return Arc(
            *base,
            style,
            center_uor,
            center_master,
            axes_uor[0],
            axes_uor[1],
            primary_master,
            secondary_master,
            rotation[0],
            rotation[1],
            start_angle[0],
            start_angle[1],
            sweep_angle[0],
            sweep_angle[1],
        )
    if index in rows.texts:
        (
            _,
            font,
            multipliers_raw,
            multipliers_master,
            rotation,
            origin_uor,
            origin_master,
            text_location,
        ) = rows.texts[index]
        length_master, height_master = (
            (None, None)
            if multipliers_master is None
            else multipliers_master
        )
        text_offset, text_length, editable_fields = text_location
        return Text(
            *base,
            style,
            font[0],
            font[1],
            multipliers_raw[0],
            multipliers_raw[1],
            length_master,
            height_master,
            rotation[0],
            rotation[1],
            origin_uor,
            origin_master,
            editable_fields,
            metadata.record.raw_view[text_offset : text_offset + text_length],
        )
    if index in rows.bspline_poles:
        return _multipoint_entity(
            BSplinePole, base, style, rows.bspline_poles[index]
        )
    if index in rows.bspline_surfaces:
        _, description, u_data, v_data, num_bounds = rows.bspline_surfaces[index]
        return BSplineSurface(
            *base,
            style,
            description[0],
            description[1],
            u_data[0],
            u_data[1],
            u_data[2],
            u_data[3],
            u_data[4],
            v_data[0],
            v_data[1],
            v_data[2],
            v_data[3],
            v_data[4],
            num_bounds,
        )
    if index in rows.bspline_boundaries:
        _, number, vertices_raw, vertices_raw_precise, vertices_uv = (
            rows.bspline_boundaries[index]
        )
        return BSplineSurfaceBoundary(
            *base,
            style,
            number,
            tuple(vertices_raw),
            tuple(vertices_raw_precise),
            tuple(vertices_uv),
        )
    if index in rows.bspline_knots:
        _, values_raw, values = rows.bspline_knots[index]
        return BSplineKnot(*base, style, tuple(values_raw), tuple(values))
    if index in rows.bspline_curves:
        _, description, order, properties, curve_type, poles, knots = (
            rows.bspline_curves[index]
        )
        return BSplineCurve(
            *base,
            style,
            description,
            order,
            properties,
            curve_type,
            poles,
            knots,
        )
    if index in rows.bspline_weights:
        _, values_raw, values = rows.bspline_weights[index]
        return BSplineWeight(*base, style, tuple(values_raw), tuple(values))
    if index in rows.color_tables:
        _, screen_flag, colors = rows.color_tables[index]
        if len(colors) != 256:
            raise RuntimeError("native color table does not contain 256 entries")
        return ColorTable(*base, None, screen_flag, tuple(colors))
    return UnsupportedElement(*base, style)


def _multipoint_entity(
    entity_type: type[LineString]
    | type[Shape]
    | type[Curve]
    | type[BSplinePole],
    base: tuple[RawElement, CommonElementHeader | None],
    style: BasicStyle | None,
    row: MultiPointRow,
) -> LineString | Shape | Curve | BSplinePole:
    _, vertices_uor, vertices_uor_precise, vertices_master = row
    return entity_type(
        *base,
        style,
        tuple(vertices_uor),
        tuple(vertices_uor_precise),
        None if vertices_master is None else tuple(vertices_master),
    )


def _basic_style(
    header: CommonElementHeader | None,
    colors: tuple[RgbColor, ...] | None,
    linkages: tuple[AttributeLinkage, ...],
) -> BasicStyle | None:
    if header is None:
        return None
    symbology = header.symbology
    rgb = None if colors is None else colors[symbology.color]
    fill_color_index = next(
        (
            linkage.color_index
            for linkage in linkages
            if linkage.kind == "SHAPE_FILL"
        ),
        None,
    )
    fill_rgb = (
        None
        if colors is None or fill_color_index is None
        else colors[fill_color_index]
    )
    return BasicStyle(
        color_index=symbology.color,
        line_style=symbology.style,
        line_weight=symbology.weight,
        rgb=rgb,
        fill_color_index=fill_color_index,
        fill_rgb=fill_rgb,
    )
