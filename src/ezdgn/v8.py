"""Native, bounded DGN V8 container, raw-object, and semantic APIs."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Any, Iterator, Literal, TypeAlias, cast

from ._core import (
    DEFAULT_MAX_CFB_ENTRIES,
    DEFAULT_MAX_CFB_STREAM_SIZE_BYTES,
    DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES,
    DEFAULT_MAX_V8_HIERARCHY_DEPTH,
    DEFAULT_MAX_V8_INFLATED_STREAM_BYTES,
    DEFAULT_MAX_V8_MODELS,
    DEFAULT_MAX_V8_OBJECT_SIZE_BYTES,
    DEFAULT_MAX_V8_OBJECTS,
    DEFAULT_MAX_V8_PAGES,
    DEFAULT_MAX_V8_STRING_BYTES,
    DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES,
    DEFAULT_MAX_V8_VERTICES,
    DgnLimitError,
)
from ._core import inspect_v8_cfb as _inspect_v8_cfb
from ._core import read_v8_document as _read_v8_document
from ._core import scan_v8_object_records as _scan_v8_object_records
from .raw import DEFAULT_MAX_FILE_SIZE_BYTES, DgnSource, _read_all, _validate_limit

Point3: TypeAlias = tuple[float, float, float]
Point3I64: TypeAlias = tuple[int, int, int]
Range3: TypeAlias = tuple[Point3, Point3]
Range3I64: TypeAlias = tuple[Point3I64, Point3I64]
V8CfbEntryKind: TypeAlias = Literal["storage", "stream"]
V8ObjectFamily: TypeAlias = Literal["graphical", "control", "named"]
V8ObjectRole: TypeAlias = Literal[
    "standalone", "header", "component", "header_component"
]


@dataclass(frozen=True, slots=True)
class V8ScanLimits:
    """All resource limits applied before and during native V8 decoding."""

    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES
    max_cfb_entries: int = DEFAULT_MAX_CFB_ENTRIES
    max_stream_size: int = DEFAULT_MAX_CFB_STREAM_SIZE_BYTES
    max_total_stream_bytes: int = DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES
    max_pages: int = DEFAULT_MAX_V8_PAGES
    max_objects: int = DEFAULT_MAX_V8_OBJECTS
    max_object_size: int = DEFAULT_MAX_V8_OBJECT_SIZE_BYTES
    max_inflated_stream_size: int = DEFAULT_MAX_V8_INFLATED_STREAM_BYTES
    max_total_inflated_bytes: int = DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES
    max_models: int = DEFAULT_MAX_V8_MODELS
    max_string_bytes: int = DEFAULT_MAX_V8_STRING_BYTES
    max_vertices: int = DEFAULT_MAX_V8_VERTICES
    max_hierarchy_depth: int = DEFAULT_MAX_V8_HIERARCHY_DEPTH

    def _native_row(self) -> list[int]:
        values = [
            self.max_file_size,
            self.max_cfb_entries,
            self.max_stream_size,
            self.max_total_stream_bytes,
            self.max_pages,
            self.max_objects,
            self.max_object_size,
            self.max_inflated_stream_size,
            self.max_total_inflated_bytes,
            self.max_models,
            self.max_string_bytes,
            self.max_vertices,
            self.max_hierarchy_depth,
        ]
        for name, value in zip(self.__dataclass_fields__, values, strict=True):
            _validate_limit(name, value)
        return values


@dataclass(frozen=True, slots=True)
class V8CfbEntry:
    """One non-root storage or stream in a V8 CFB directory."""

    path: str
    kind: V8CfbEntryKind
    size_bytes: int | None


@dataclass(frozen=True, slots=True)
class V8ContainerInfo:
    """Bounded CFB directory metadata and DGN marker validation."""

    cfb_version: Literal[3, 4]
    has_dgn_v8_markers: bool
    missing_markers: tuple[str, ...]
    model_storage_paths: tuple[str, ...]
    entries: tuple[V8CfbEntry, ...]

    @property
    def stream_count(self) -> int:
        return sum(entry.kind == "stream" for entry in self.entries)

    @property
    def storage_count(self) -> int:
        return sum(entry.kind == "storage" for entry in self.entries)


@dataclass(frozen=True, slots=True)
class V8PageHeader:
    record_count: int
    format_version: int
    page_number: int
    population: int


@dataclass(frozen=True, slots=True)
class V8RawObject:
    """One word-bounded V8 object with its complete inflated bytes retained."""

    index: int
    page_index: int
    family: V8ObjectFamily
    stream_path: str
    inflated_offset: int
    framing_prefix: int
    type_and_flags: int
    element_type: int
    role: V8ObjectRole
    words: int
    attribute_words: int
    level: int | None
    element_id: int | None
    model_id: int | None
    raw_bytes: bytes

    @property
    def primary_bytes(self) -> bytes:
        return self.raw_bytes[: self.attribute_words * 2]

    @property
    def attribute_bytes(self) -> bytes:
        return self.raw_bytes[self.attribute_words * 2 :]

    @property
    def size_bytes(self) -> int:
        return len(self.raw_bytes)


@dataclass(frozen=True, slots=True)
class V8AuxiliaryRecord:
    """One exact V8 auxiliary/XAttribute record."""

    index: int
    stream_path: str
    inflated_offset: int
    magic: int
    kind: int
    reserved: int
    element_id: int
    flags: int
    raw_bytes: bytes
    payload: bytes


@dataclass(frozen=True, slots=True)
class V8ObjectPage:
    stream_path: str
    family: V8ObjectFamily
    header: V8PageHeader
    inflated_size: int
    objects: tuple[V8RawObject, ...]


@dataclass(frozen=True, slots=True)
class V8AuxiliaryPage:
    stream_path: str
    header: V8PageHeader
    inflated_size: int
    records: tuple[V8AuxiliaryRecord, ...]


@dataclass(frozen=True, slots=True)
class V8ModelIndexEntry:
    index: int
    raw_number: int
    storage_index: int
    model_number: int
    flags: int
    model_id: int
    name: str
    description: str
    raw_bytes: bytes


@dataclass(frozen=True, slots=True)
class V8RawModel:
    index: V8ModelIndexEntry
    storage_path: str
    model_header_stream_path: str
    model_header_stream_bytes: bytes
    model_header_bytes: bytes
    graphical_pages: tuple[V8ObjectPage, ...]
    graphical_auxiliary_pages: tuple[V8AuxiliaryPage, ...]
    control_pages: tuple[V8ObjectPage, ...]
    control_auxiliary_pages: tuple[V8AuxiliaryPage, ...]

    @property
    def graphical_objects(self) -> tuple[V8RawObject, ...]:
        return tuple(item for page in self.graphical_pages for item in page.objects)

    @property
    def control_objects(self) -> tuple[V8RawObject, ...]:
        return tuple(item for page in self.control_pages for item in page.objects)


@dataclass(frozen=True, slots=True)
class V8RawDocument:
    container: V8ContainerInfo
    models: tuple[V8RawModel, ...]
    named_pages: tuple[V8ObjectPage, ...]
    named_auxiliary_pages: tuple[V8AuxiliaryPage, ...]
    total_inflated_bytes: int
    graphical_object_count: int
    total_object_count: int


@dataclass(frozen=True, slots=True)
class V8Linkage:
    """One native word-framed V8 linkage, including unknown raw bytes."""

    offset: int
    words_to_follow: int | None
    kind_code: int | None
    property_id: int | None
    property_bytes: bytes | None
    property_text: str | None
    complete: bool
    raw_bytes: bytes


@dataclass(frozen=True, slots=True)
class V8Point:
    uor: Point3
    master: Point3


@dataclass(frozen=True, slots=True)
class V8CommonHeader:
    level: int
    element_id: int
    model_id: int
    graphic_group: int
    properties: int
    geometry_flags: int
    line_style: int
    line_weight: int
    color_index: int
    stored_dimension: Literal[2, 3]
    dimension: Literal[2, 3]
    range_uor: Range3I64
    range_master: Range3
    attribute_offset: int
    attribute_length: int


@dataclass(frozen=True, slots=True)
class V8ElementData:
    """Native parameters for a supported V8 element kind."""

    kind: str
    vertices: tuple[V8Point, ...] = ()
    orientations: tuple[tuple[float, float, float, float], ...] = ()
    origin: V8Point | None = None
    center: V8Point | None = None
    anchor: V8Point | None = None
    font_id: int | None = None
    justification: int | None = None
    width_multiplier_raw: float | None = None
    height_multiplier_raw: float | None = None
    width_uor: float | None = None
    height_uor: float | None = None
    width_master: float | None = None
    height_master: float | None = None
    rotation_radians: float | None = None
    orientation: tuple[float, ...] = ()
    editable_fields: int | None = None
    encoding: str | None = None
    text_bytes: bytes | None = None
    text: str | None = None
    primary_axis_uor: float | None = None
    secondary_axis_uor: float | None = None
    primary_axis_master: float | None = None
    secondary_axis_master: float | None = None
    start_angle_radians: float | None = None
    sweep_angle_radians: float | None = None
    child_count: int | None = None
    node_number: int | None = None
    boundary_count: int | None = None
    transform: tuple[float, ...] = ()
    name: str | None = None
    properties_raw: int | None = None
    declared_poles: int | None = None

    @property
    def rotation_degrees(self) -> float | None:
        if self.rotation_radians is None:
            return None
        return math.degrees(self.rotation_radians)

    @property
    def start_angle_degrees(self) -> float | None:
        if self.start_angle_radians is None:
            return None
        return math.degrees(self.start_angle_radians)

    @property
    def sweep_angle_degrees(self) -> float | None:
        if self.sweep_angle_radians is None:
            return None
        return math.degrees(self.sweep_angle_radians)


@dataclass(frozen=True, slots=True)
class V8Element:
    index: int
    raw: V8RawObject
    common: V8CommonHeader
    data: V8ElementData
    parent_index: int | None
    child_indices: tuple[int, ...]
    linkages: tuple[V8Linkage, ...]
    auxiliary_records: tuple[V8AuxiliaryRecord, ...]

    @property
    def kind(self) -> str:
        return self.data.kind

    @property
    def level(self) -> int:
        return self.common.level

    @property
    def is_top_level(self) -> bool:
        return self.parent_index is None

    @property
    def is_drawable(self) -> bool:
        return self.kind not in {
            "UNKNOWN",
            "UNKNOWN_COMPLEX",
            "BSPLINE_POLE",
            "TEXT_NODE",
        }

    def dxftype(self) -> str:
        """Return the stable uppercase entity label used by ``query`` and CLI."""

        return self.kind


@dataclass(frozen=True, slots=True)
class V8ModelMetadata:
    index: int
    storage_path: str
    storage_index: int
    model_number: int
    model_id: int
    index_model_id: int
    name: str
    description: str
    dimension: Literal[2, 3]
    type_and_flags: int
    model_flags: int
    uor_per_master: float
    scale: float
    global_origin_uor: Point3
    extents_uor: Range3I64
    extents_master: Range3
    master_unit: str | None
    sub_unit: str | None
    linkages: tuple[V8Linkage, ...]
    raw_header: bytes


@dataclass(frozen=True, slots=True)
class V8Model:
    metadata: V8ModelMetadata
    elements: tuple[V8Element, ...]

    @property
    def entities(self) -> tuple[V8Element, ...]:
        result = []
        for element in self.elements:
            if not element.is_drawable:
                continue
            if element.is_top_level:
                result.append(element)
                continue
            if (
                element.kind == "TEXT"
                and element.parent_index is not None
                and self.elements[element.parent_index].kind == "TEXT_NODE"
            ):
                result.append(element)
        return tuple(result)

    @property
    def all_entities(self) -> tuple[V8Element, ...]:
        return tuple(element for element in self.elements if element.is_drawable)

    @property
    def unknown_elements(self) -> tuple[V8Element, ...]:
        return tuple(
            element
            for element in self.elements
            if element.kind in {"UNKNOWN", "UNKNOWN_COMPLEX"}
        )

    def __iter__(self) -> Iterator[V8Element]:
        return iter(self.entities)

    def query(self, kind: str) -> tuple[V8Element, ...]:
        normalized = kind.strip().upper().replace("-", "_").replace(" ", "_")
        return tuple(element for element in self.entities if element.kind == normalized)

    def parent(self, element: V8Element) -> V8Element | None:
        if element.parent_index is None:
            return None
        return self.elements[element.parent_index]

    def children(self, element: V8Element) -> tuple[V8Element, ...]:
        return tuple(self.elements[index] for index in element.child_indices)

    def descendants(self, element: V8Element) -> Iterator[V8Element]:
        pending = list(reversed(element.child_indices))
        while pending:
            index = pending.pop()
            child = self.elements[index]
            yield child
            pending.extend(reversed(child.child_indices))


@dataclass(frozen=True, slots=True)
class V8Document:
    raw: V8RawDocument
    models: tuple[V8Model, ...]

    @property
    def entities(self) -> tuple[V8Element, ...]:
        return tuple(element for model in self.models for element in model.entities)

    def __iter__(self) -> Iterator[V8Model]:
        return iter(self.models)


def inspect_v8_container(
    source: DgnSource,
    *,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
    max_entries: int = DEFAULT_MAX_CFB_ENTRIES,
) -> V8ContainerInfo:
    """Inspect DGN markers and directory entries without inflating V8 streams."""

    _validate_limit("max_file_size", max_file_size)
    _validate_limit("max_entries", max_entries)
    data = _read_all(source, max_file_size=max_file_size)
    if len(data) > max_file_size:
        raise DgnLimitError(
            f"input size {len(data)} bytes exceeds configured limit {max_file_size} bytes"
        )
    cfb_version, has_markers, missing, storage_paths, entries = _inspect_v8_cfb(
        data, max_entries
    )
    return V8ContainerInfo(
        cfb_version=cast(Literal[3, 4], cfb_version),
        has_dgn_v8_markers=has_markers,
        missing_markers=tuple(missing),
        model_storage_paths=tuple(storage_paths),
        entries=tuple(
            V8CfbEntry(path, cast(V8CfbEntryKind, kind), size)
            for path, kind, size in entries
        ),
    )


def scan_v8_objects(
    source: DgnSource,
    *,
    limits: V8ScanLimits | None = None,
) -> V8RawDocument:
    """Scan every recognized V8 object family while preserving exact bytes."""

    actual_limits = limits or V8ScanLimits()
    limit_row = actual_limits._native_row()
    data = _read_all(source, max_file_size=actual_limits.max_file_size)
    return _raw_document_from_core(_scan_v8_object_records(data, limit_row))


def read_v8(
    source: DgnSource,
    *,
    limits: V8ScanLimits | None = None,
) -> V8Document:
    """Read supported native V8 models and retain the full raw object scan."""

    actual_limits = limits or V8ScanLimits()
    limit_row = actual_limits._native_row()
    data = _read_all(source, max_file_size=actual_limits.max_file_size)
    return _document_from_core(_read_v8_document(data, limit_row))


read_v8file = read_v8


def _container_from_core(row: dict[str, Any]) -> V8ContainerInfo:
    return V8ContainerInfo(
        cfb_version=cast(Literal[3, 4], row["cfb_version"]),
        has_dgn_v8_markers=row["has_dgn_v8_markers"],
        missing_markers=tuple(row["missing_markers"]),
        model_storage_paths=tuple(row["model_storage_paths"]),
        entries=tuple(
            V8CfbEntry(path, cast(V8CfbEntryKind, kind), size)
            for path, kind, size in row["entries"]
        ),
    )


def _page_header_from_core(row: dict[str, Any]) -> V8PageHeader:
    return V8PageHeader(
        record_count=row["record_count"],
        format_version=row["format_version"],
        page_number=row["page_number"],
        population=row["population"],
    )


def _raw_object_from_core(row: dict[str, Any]) -> V8RawObject:
    return V8RawObject(
        index=row["index"],
        page_index=row["page_index"],
        family=cast(V8ObjectFamily, row["family"]),
        stream_path=row["stream_path"],
        inflated_offset=row["inflated_offset"],
        framing_prefix=row["framing_prefix"],
        type_and_flags=row["type_and_flags"],
        element_type=row["element_type"],
        role=cast(V8ObjectRole, row["role"]),
        words=row["words"],
        attribute_words=row["attribute_words"],
        level=row["level"],
        element_id=row["element_id"],
        model_id=row["model_id"],
        raw_bytes=row["raw_bytes"],
    )


def _object_page_from_core(row: dict[str, Any]) -> V8ObjectPage:
    return V8ObjectPage(
        stream_path=row["stream_path"],
        family=cast(V8ObjectFamily, row["family"]),
        header=_page_header_from_core(row["header"]),
        inflated_size=row["inflated_size"],
        objects=tuple(_raw_object_from_core(item) for item in row["objects"]),
    )


def _aux_record_from_core(row: dict[str, Any]) -> V8AuxiliaryRecord:
    return V8AuxiliaryRecord(
        index=row["index"],
        stream_path=row["stream_path"],
        inflated_offset=row["inflated_offset"],
        magic=row["magic"],
        kind=row["kind"],
        reserved=row["reserved"],
        element_id=row["element_id"],
        flags=row["flags"],
        raw_bytes=row["raw_bytes"],
        payload=row["payload"],
    )


def _aux_page_from_core(row: dict[str, Any]) -> V8AuxiliaryPage:
    return V8AuxiliaryPage(
        stream_path=row["stream_path"],
        header=_page_header_from_core(row["header"]),
        inflated_size=row["inflated_size"],
        records=tuple(_aux_record_from_core(item) for item in row["records"]),
    )


def _model_index_from_core(row: dict[str, Any]) -> V8ModelIndexEntry:
    return V8ModelIndexEntry(
        index=row["index"],
        raw_number=row["raw_number"],
        storage_index=row["storage_index"],
        model_number=row["model_number"],
        flags=row["flags"],
        model_id=row["model_id"],
        name=row["name"],
        description=row["description"],
        raw_bytes=row["raw_bytes"],
    )


def _raw_model_from_core(row: dict[str, Any]) -> V8RawModel:
    return V8RawModel(
        index=_model_index_from_core(row["index"]),
        storage_path=row["storage_path"],
        model_header_stream_path=row["model_header_stream_path"],
        model_header_stream_bytes=row["model_header_stream_bytes"],
        model_header_bytes=row["model_header_bytes"],
        graphical_pages=tuple(
            _object_page_from_core(item) for item in row["graphical_pages"]
        ),
        graphical_auxiliary_pages=tuple(
            _aux_page_from_core(item) for item in row["graphical_auxiliary_pages"]
        ),
        control_pages=tuple(
            _object_page_from_core(item) for item in row["control_pages"]
        ),
        control_auxiliary_pages=tuple(
            _aux_page_from_core(item) for item in row["control_auxiliary_pages"]
        ),
    )


def _raw_document_from_core(row: dict[str, Any]) -> V8RawDocument:
    return V8RawDocument(
        container=_container_from_core(row["container"]),
        models=tuple(_raw_model_from_core(item) for item in row["models"]),
        named_pages=tuple(_object_page_from_core(item) for item in row["named_pages"]),
        named_auxiliary_pages=tuple(
            _aux_page_from_core(item) for item in row["named_auxiliary_pages"]
        ),
        total_inflated_bytes=row["total_inflated_bytes"],
        graphical_object_count=row["graphical_object_count"],
        total_object_count=row["total_object_count"],
    )


def _linkage_from_core(row: dict[str, Any]) -> V8Linkage:
    return V8Linkage(
        offset=row["offset"],
        words_to_follow=row["words_to_follow"],
        kind_code=row["kind_code"],
        property_id=row["property_id"],
        property_bytes=row["property_bytes"],
        property_text=row["property_text"],
        complete=row["complete"],
        raw_bytes=row["raw_bytes"],
    )


def _point_from_core(row: tuple[Point3, Point3]) -> V8Point:
    return V8Point(tuple(row[0]), tuple(row[1]))  # type: ignore[arg-type]


def _common_from_core(row: dict[str, Any]) -> V8CommonHeader:
    return V8CommonHeader(
        level=row["level"],
        element_id=row["element_id"],
        model_id=row["model_id"],
        graphic_group=row["graphic_group"],
        properties=row["properties"],
        geometry_flags=row["geometry_flags"],
        line_style=row["line_style"],
        line_weight=row["line_weight"],
        color_index=row["color_index"],
        stored_dimension=cast(Literal[2, 3], row["stored_dimension"]),
        dimension=cast(Literal[2, 3], row["dimension"]),
        range_uor=cast(Range3I64, tuple(map(tuple, row["range_uor"]))),
        range_master=cast(Range3, tuple(map(tuple, row["range_master"]))),
        attribute_offset=row["attribute_offset"],
        attribute_length=row["attribute_length"],
    )


def _element_data_from_core(row: dict[str, Any]) -> V8ElementData:
    return V8ElementData(
        kind=row["kind"],
        vertices=tuple(_point_from_core(item) for item in row.get("vertices", ())),
        orientations=tuple(tuple(item) for item in row.get("orientations", ())),
        origin=(
            _point_from_core(row["origin"]) if row.get("origin") is not None else None
        ),
        center=(
            _point_from_core(row["center"]) if row.get("center") is not None else None
        ),
        anchor=(
            _point_from_core(row["anchor"]) if row.get("anchor") is not None else None
        ),
        font_id=row.get("font_id"),
        justification=row.get("justification"),
        width_multiplier_raw=row.get("width_multiplier_raw"),
        height_multiplier_raw=row.get("height_multiplier_raw"),
        width_uor=row.get("width_uor"),
        height_uor=row.get("height_uor"),
        width_master=row.get("width_master"),
        height_master=row.get("height_master"),
        rotation_radians=row.get("rotation_radians"),
        orientation=tuple(row.get("orientation", ())),
        editable_fields=row.get("editable_fields"),
        encoding=row.get("encoding"),
        text_bytes=row.get("text_bytes"),
        text=row.get("text"),
        primary_axis_uor=row.get("primary_axis_uor"),
        secondary_axis_uor=row.get("secondary_axis_uor"),
        primary_axis_master=row.get("primary_axis_master"),
        secondary_axis_master=row.get("secondary_axis_master"),
        start_angle_radians=row.get("start_angle_radians"),
        sweep_angle_radians=row.get("sweep_angle_radians"),
        child_count=row.get("child_count"),
        node_number=row.get("node_number"),
        boundary_count=row.get("boundary_count"),
        transform=tuple(row.get("transform", ())),
        name=row.get("name"),
        properties_raw=row.get("properties_raw"),
        declared_poles=row.get("declared_poles"),
    )


def _metadata_from_core(row: dict[str, Any]) -> V8ModelMetadata:
    return V8ModelMetadata(
        index=row["index"],
        storage_path=row["storage_path"],
        storage_index=row["storage_index"],
        model_number=row["model_number"],
        model_id=row["model_id"],
        index_model_id=row["index_model_id"],
        name=row["name"],
        description=row["description"],
        dimension=cast(Literal[2, 3], row["dimension"]),
        type_and_flags=row["type_and_flags"],
        model_flags=row["model_flags"],
        uor_per_master=row["uor_per_master"],
        scale=row["scale"],
        global_origin_uor=cast(Point3, tuple(row["global_origin_uor"])),
        extents_uor=cast(Range3I64, tuple(map(tuple, row["extents_uor"]))),
        extents_master=cast(Range3, tuple(map(tuple, row["extents_master"]))),
        master_unit=row["master_unit"],
        sub_unit=row["sub_unit"],
        linkages=tuple(_linkage_from_core(item) for item in row["linkages"]),
        raw_header=row["raw_header"],
    )


def _document_from_core(row: dict[str, Any]) -> V8Document:
    raw = _raw_document_from_core(row["raw"])
    objects = {
        item.index: item
        for model in raw.models
        for page in (*model.graphical_pages, *model.control_pages)
        for item in page.objects
    }
    auxiliary = {
        record.index: record
        for model in raw.models
        for page in (
            *model.graphical_auxiliary_pages,
            *model.control_auxiliary_pages,
        )
        for record in page.records
    }
    models = []
    for model_row in row["models"]:
        elements = tuple(
            V8Element(
                index=element["index"],
                raw=objects[element["raw_object_index"]],
                common=_common_from_core(element["common"]),
                data=_element_data_from_core(element["data"]),
                parent_index=element["parent_index"],
                child_indices=tuple(element["child_indices"]),
                linkages=tuple(
                    _linkage_from_core(item) for item in element["linkages"]
                ),
                auxiliary_records=tuple(
                    auxiliary[index] for index in element["auxiliary_indices"]
                ),
            )
            for element in model_row["elements"]
        )
        models.append(
            V8Model(metadata=_metadata_from_core(model_row["metadata"]), elements=elements)
        )
    return V8Document(raw=raw, models=tuple(models))


__all__ = [
    "DEFAULT_MAX_CFB_ENTRIES",
    "DEFAULT_MAX_CFB_STREAM_SIZE_BYTES",
    "DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES",
    "DEFAULT_MAX_V8_HIERARCHY_DEPTH",
    "DEFAULT_MAX_V8_INFLATED_STREAM_BYTES",
    "DEFAULT_MAX_V8_MODELS",
    "DEFAULT_MAX_V8_OBJECT_SIZE_BYTES",
    "DEFAULT_MAX_V8_OBJECTS",
    "DEFAULT_MAX_V8_PAGES",
    "DEFAULT_MAX_V8_STRING_BYTES",
    "DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES",
    "DEFAULT_MAX_V8_VERTICES",
    "V8AuxiliaryPage",
    "V8AuxiliaryRecord",
    "V8CfbEntry",
    "V8CommonHeader",
    "V8ContainerInfo",
    "V8Document",
    "V8Element",
    "V8ElementData",
    "V8Linkage",
    "V8Model",
    "V8ModelIndexEntry",
    "V8ModelMetadata",
    "V8ObjectPage",
    "V8PageHeader",
    "V8Point",
    "V8RawDocument",
    "V8RawModel",
    "V8RawObject",
    "V8ScanLimits",
    "inspect_v8_container",
    "read_v8",
    "read_v8file",
    "scan_v8_objects",
]
