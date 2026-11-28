"""Native, bounded DGN V8 container, raw-object, and semantic APIs."""

from __future__ import annotations

import math
import operator
import struct
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from functools import lru_cache
from itertools import pairwise
from typing import Any, Iterator, Literal, TypeAlias, TypeVar, cast, overload

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
    V8_ITEM_INSTANCE_ENTRY_TYPE,
    V8_MAX_AUXILIARY_ENTRIES,
    V8_MAX_ITEM_BYTES,
    V8_MAX_TAG_SET_BYTES,
    V8_TAG_SET_CODE,
    V8_TAG_VALUE_INTEGER,
    V8_TAG_VALUE_TEXT,
    DgnLimitError,
)
from ._core import decode_v8_auxiliary_payload as _decode_v8_auxiliary_payload
from ._core import decode_v8_item_instance_data as _decode_v8_item_instance_data
from ._core import decode_v8_item_instances_data as _decode_v8_item_instances_data
from ._core import decode_v8_tag_set_data as _decode_v8_tag_set_data
from ._core import decode_v8_tag_sets_data as _decode_v8_tag_sets_data
from ._core import inspect_v8_cfb as _inspect_v8_cfb
from ._core import read_v8_document as _read_v8_document
from ._core import read_v8_shared_cells as _read_v8_shared_cells
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
class V8AuxiliaryEntry:
    """One framed entry of an auxiliary-record payload.

    ``type_id`` selects the meaning of ``data``. Its high half groups related
    entry types and its low half numbers the types of one group. ``size_word``
    is the stored size word: the low 31 bits are ``len(data)`` and bit 31 is a
    writer flag whose meaning is unresolved.
    """

    index: int
    offset: int
    type_id: int
    entry_id: int
    size_word: int
    reserved: int
    data: bytes

    @property
    def type_major(self) -> int:
        return self.type_id >> 16

    @property
    def type_minor(self) -> int:
        return self.type_id & 0xFFFF

    @property
    def size_flag(self) -> bool:
        return bool(self.size_word & 0x8000_0000)


@dataclass(frozen=True, slots=True)
class V8AuxiliaryRecord:
    """One exact V8 auxiliary/XAttribute record.

    ``flags`` keeps its historical name; it is the number of entries in the
    payload.
    """

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

    @property
    def entries(self) -> tuple[V8AuxiliaryEntry, ...] | None:
        """The entries of the payload, or ``None`` when the payload does not
        follow the entry layout exactly or declares more than
        ``V8_MAX_AUXILIARY_ENTRIES`` entries.

        The payload is split on each access and nothing is stored, so a
        document costs no more to read when the entries are not used.
        """

        return decode_auxiliary_entries(self.payload, self.flags)


@dataclass(frozen=True, slots=True)
class V8ItemInstance:
    """One item instance attached to an element.

    ``class_name`` is the item class, ``attributes`` are the attributes of the
    stored root element (``instanceId`` and ``xmlns`` in the observed files)
    and ``properties`` are the property names with their text values, all in
    stored order and exactly as stored. ``schema_element_id`` is the element ID
    of the named object that holds the schema text.

    The stored instance has a string table, and its names and values are
    numbers of table strings. Each table string is one ``str`` object here,
    and every use of it is a reference to that object, not a copy.
    """

    entry_id: int
    schema_element_id: int
    class_name: str
    attributes: tuple[tuple[str, str], ...]
    properties: tuple[tuple[str, str], ...]

    @property
    def schema_reference(self) -> str | None:
        """The ``xmlns`` attribute: schema name and version of the class."""

        return self.attribute("xmlns")

    def attribute(self, name: str, default: str | None = None) -> str | None:
        """Return the value of the first root attribute called ``name``."""

        return next((value for key, value in self.attributes if key == name), default)

    def get(self, name: str, default: str | None = None) -> str | None:
        """Return the value of the first property called ``name``."""

        return next((value for key, value in self.properties if key == name), default)

    def as_dict(self) -> dict[str, str]:
        """Return the properties as a mapping; the first of equal names wins."""

        result: dict[str, str] = {}
        for key, value in self.properties:
            result.setdefault(key, value)
        return result


@dataclass(frozen=True, slots=True)
class V8ItemInstances:
    """The item instances of one call, and what the size limit left raw.

    ``over_limit`` is the number of item entries that were not decoded because
    their data was larger than what the call had left of
    ``V8_MAX_ITEM_BYTES``. They stay raw in their records.
    """

    instances: tuple[V8ItemInstance, ...]
    over_limit: int


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
    skipped_models: tuple[str, ...] = ()


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
class V8MeshFace:
    """One face loop of an indexed mesh.

    ``point_indices`` are zero-based indices into the mesh's ``vertices``;
    ``visible_edges[i]`` tells whether the edge from point ``i`` to the next
    point of the loop is drawn.
    """

    point_indices: tuple[int, ...]
    visible_edges: tuple[bool, ...]


_Item = TypeVar("_Item")
_POINT_STRUCT = struct.Struct("<3d")
_FACE_OFFSET_STRUCT = struct.Struct("<I")
_FACE_RANGE_STRUCT = struct.Struct("<2I")
_PACKED_VALUE_FORMATS = ("d", "i")
_ORIENTATION_VALUES = 4
_REPR_ITEMS = 3


def _as_bytes(data: bytes | bytearray | memoryview) -> bytes:
    return data if isinstance(data, bytes) else bytes(memoryview(data))


@lru_cache(maxsize=32)
def _value_struct(format: str, width: int) -> struct.Struct:
    return struct.Struct(f"<{width}{format}")


def _hash_value(value: Any) -> Any:
    # Unpacking creates a new float. Python hashes NaNs by identity, so
    # identical buffers need a shared NaN when computing their tuple hash.
    if isinstance(value, float) and math.isnan(value):
        return math.nan
    if isinstance(value, tuple):
        return tuple(_hash_value(part) for part in value)
    if isinstance(value, V8Point):
        return V8Point(_hash_value(value.uor), _hash_value(value.master))
    return value


class _PackedSequence(Sequence[_Item]):
    """Tuple-like behaviour shared by the packed sequences of ``V8ElementData``.

    A packed sequence stands for the tuple of items it would build: it has a
    length, can be indexed, sliced (a slice is a tuple) and iterated, and
    compares and hashes equal to that tuple. Items are built each time they are
    read, so ``sequence[0] is sequence[0]`` is false.
    """

    __slots__ = ()

    def _buffers(self) -> tuple[Any, ...]:
        raise NotImplementedError

    def _item(self, position: int) -> _Item:
        raise NotImplementedError

    @overload
    def __getitem__(self, index: int) -> _Item: ...

    @overload
    def __getitem__(self, index: slice) -> tuple[_Item, ...]: ...

    def __getitem__(self, index: int | slice) -> _Item | tuple[_Item, ...]:
        length = len(self)
        if isinstance(index, slice):
            positions = range(*index.indices(length))
            return tuple(self._item(position) for position in positions)
        position = operator.index(index)
        if position < 0:
            position += length
        if not 0 <= position < length:
            raise IndexError(f"{type(self).__name__} index out of range")
        return self._item(position)

    def __eq__(self, other: object) -> bool:
        if isinstance(other, _PackedSequence):
            if type(other) is type(self) and other._buffers() == self._buffers():
                return True
        elif not isinstance(other, tuple):
            return NotImplemented
        return len(self) == len(other) and all(
            mine == theirs for mine, theirs in zip(self, other)
        )

    def __hash__(self) -> int:
        return hash(tuple(_hash_value(item) for item in self))

    def __add__(self, other: object) -> tuple[Any, ...]:
        if isinstance(other, (tuple, _PackedSequence)):
            return tuple(self) + tuple(other)
        return NotImplemented

    def __radd__(self, other: object) -> tuple[Any, ...]:
        if isinstance(other, tuple):
            return other + tuple(self)
        return NotImplemented

    def __repr__(self) -> str:
        shown = [repr(item) for item in self[:_REPR_ITEMS]]
        if len(self) > _REPR_ITEMS:
            shown.append("...")
        return f"<{type(self).__name__} of {len(self)}: {', '.join(shown)}>"

    def __reduce__(self) -> tuple[Any, ...]:
        return type(self), self._buffers()


class V8PointArray(_PackedSequence[V8Point]):
    """The points of one element, kept as packed coordinates.

    ``read_v8`` stores an element's ``vertices`` in this form. It behaves as
    the tuple of :class:`V8Point` it stands for, and builds each point when it
    is read. ``uor_bytes`` and ``master_bytes`` hold the same points as
    little-endian ``float64`` ``x, y, z`` triples for bulk access, for example
    ``numpy.frombuffer(vertices.master_bytes, "<f8").reshape(-1, 3)``.
    """

    __slots__ = ("_uor", "_master")

    def __init__(self, uor: bytes, master: bytes) -> None:
        uor = _as_bytes(uor)
        master = _as_bytes(master)
        if len(uor) != len(master) or len(uor) % _POINT_STRUCT.size:
            raise ValueError(
                "uor and master must hold the same number of little-endian "
                "float64 XYZ triples"
            )
        self._uor = uor
        self._master = master

    @classmethod
    def from_points(cls, points: Iterable[V8Point]) -> V8PointArray:
        uor = bytearray()
        master = bytearray()
        for point in points:
            uor += _POINT_STRUCT.pack(*point.uor)
            master += _POINT_STRUCT.pack(*point.master)
        return cls(bytes(uor), bytes(master))

    @property
    def uor_bytes(self) -> bytes:
        return self._uor

    @property
    def master_bytes(self) -> bytes:
        return self._master

    def __len__(self) -> int:
        return len(self._uor) // _POINT_STRUCT.size

    def __iter__(self) -> Iterator[V8Point]:
        return map(
            V8Point,
            _POINT_STRUCT.iter_unpack(self._uor),
            _POINT_STRUCT.iter_unpack(self._master),
        )

    def _item(self, position: int) -> V8Point:
        offset = position * _POINT_STRUCT.size
        return V8Point(
            _POINT_STRUCT.unpack_from(self._uor, offset),
            _POINT_STRUCT.unpack_from(self._master, offset),
        )

    def _buffers(self) -> tuple[bytes, bytes]:
        return self._uor, self._master


class V8MeshFaceArray(_PackedSequence[V8MeshFace]):
    """The face loops of one mesh, kept as packed index arrays.

    ``read_v8`` stores a mesh's ``faces`` in this form. It behaves as the tuple
    of :class:`V8MeshFace` it stands for, and builds each face when it is read.
    ``point_indices_bytes`` holds the zero-based point indices of all faces, one
    after the other, as little-endian ``uint32``; ``visible_edges_bytes`` holds
    one byte (0 or 1) per index; and ``offsets_bytes`` holds, as little-endian
    ``uint32``, where each face starts in those two arrays, followed by their
    length. Face ``i`` therefore covers ``offsets[i]:offsets[i + 1]``.
    """

    __slots__ = ("_offsets", "_point_indices", "_visible_edges")

    def __init__(
        self, offsets: bytes, point_indices: bytes, visible_edges: bytes
    ) -> None:
        offsets = _as_bytes(offsets)
        point_indices = _as_bytes(point_indices)
        visible_edges = _as_bytes(visible_edges)
        index_count = len(visible_edges)
        offset_bytes = _FACE_OFFSET_STRUCT.size
        if (
            len(offsets) < offset_bytes
            or len(offsets) % offset_bytes
            or len(point_indices) != index_count * offset_bytes
            or _FACE_OFFSET_STRUCT.unpack_from(offsets, 0)[0] != 0
            or _FACE_OFFSET_STRUCT.unpack_from(offsets, len(offsets) - offset_bytes)[0]
            != index_count
        ):
            raise ValueError(
                "offsets must hold one little-endian uint32 per face and a final "
                "one, running from 0 to the number of point indices, and "
                "visible_edges must hold one byte per point index"
            )
        boundaries = _FACE_OFFSET_STRUCT.iter_unpack(offsets)
        if any(start > end for (start,), (end,) in pairwise(boundaries)):
            raise ValueError("face offsets must be nondecreasing")
        self._offsets = offsets
        self._point_indices = point_indices
        self._visible_edges = visible_edges

    @classmethod
    def from_faces(cls, faces: Iterable[V8MeshFace]) -> V8MeshFaceArray:
        offsets = bytearray(_FACE_OFFSET_STRUCT.size)
        point_indices = bytearray()
        visible_edges = bytearray()
        for face in faces:
            if len(face.point_indices) != len(face.visible_edges):
                raise ValueError("a face needs one visible_edges flag per point index")
            point_indices += struct.pack(
                f"<{len(face.point_indices)}I", *face.point_indices
            )
            visible_edges += bytes(map(bool, face.visible_edges))
            offsets += _FACE_OFFSET_STRUCT.pack(len(visible_edges))
        return cls(bytes(offsets), bytes(point_indices), bytes(visible_edges))

    @property
    def offsets_bytes(self) -> bytes:
        return self._offsets

    @property
    def point_indices_bytes(self) -> bytes:
        return self._point_indices

    @property
    def visible_edges_bytes(self) -> bytes:
        return self._visible_edges

    def __len__(self) -> int:
        return len(self._offsets) // _FACE_OFFSET_STRUCT.size - 1

    def __iter__(self) -> Iterator[V8MeshFace]:
        offsets = _FACE_OFFSET_STRUCT.iter_unpack(self._offsets)
        (start,) = next(offsets)
        for (end,) in offsets:
            yield self._face(start, end)
            start = end

    def _item(self, position: int) -> V8MeshFace:
        return self._face(
            *_FACE_RANGE_STRUCT.unpack_from(
                self._offsets, position * _FACE_OFFSET_STRUCT.size
            )
        )

    def _face(self, start: int, end: int) -> V8MeshFace:
        count = end - start
        return V8MeshFace(
            struct.unpack_from(
                f"<{count}I", self._point_indices, start * _FACE_OFFSET_STRUCT.size
            ),
            struct.unpack_from(f"<{count}?", self._visible_edges, start),
        )

    def _buffers(self) -> tuple[bytes, bytes, bytes]:
        return self._offsets, self._point_indices, self._visible_edges


class V8ValueArray(_PackedSequence[Any]):
    """Numbers of one element, kept as a packed little-endian array.

    ``read_v8`` stores matrix ``values`` (``width`` 1: each item is a number)
    and point-string ``orientations`` (``width`` 4: each item is a tuple of
    four numbers) in this form. ``format`` is ``"d"`` for ``float64`` and
    ``"i"`` for ``int32``, as in :mod:`struct`; ``data_bytes`` holds the
    numbers for bulk access.
    """

    __slots__ = ("_format", "_width", "_data")

    def __init__(self, format: Literal["d", "i"], data: bytes, width: int = 1) -> None:
        if format not in _PACKED_VALUE_FORMATS:
            raise ValueError("format must be 'd' (float64) or 'i' (int32)")
        if width < 1:
            raise ValueError("width must be at least 1")
        data = _as_bytes(data)
        if len(data) % _value_struct(format, width).size:
            raise ValueError("data must hold whole items of little-endian numbers")
        self._format = format
        self._width = width
        self._data = data

    @classmethod
    def from_values(
        cls, format: Literal["d", "i"], values: Iterable[Any], width: int = 1
    ) -> V8ValueArray:
        if format not in _PACKED_VALUE_FORMATS or width < 1:
            raise ValueError("format must be 'd' or 'i' and width must be positive")
        item_struct = _value_struct(format, width)
        data = bytearray()
        for value in values:
            row = (value,) if width == 1 else tuple(value)
            if len(row) != width:
                raise ValueError(f"expected {width} numbers per item")
            data.extend(item_struct.pack(*row))
        return cls(format, bytes(data), width)

    @property
    def format(self) -> Literal["d", "i"]:
        return self._format

    @property
    def width(self) -> int:
        return self._width

    @property
    def data_bytes(self) -> bytes:
        return self._data

    def __len__(self) -> int:
        return len(self._data) // _value_struct(self._format, self._width).size

    def __iter__(self) -> Iterator[Any]:
        items = _value_struct(self._format, self._width).iter_unpack(self._data)
        return map(operator.itemgetter(0), items) if self._width == 1 else items

    def _item(self, position: int) -> Any:
        item_struct = _value_struct(self._format, self._width)
        item = item_struct.unpack_from(self._data, position * item_struct.size)
        return item[0] if self._width == 1 else item

    def _buffers(self) -> tuple[str, bytes, int]:
        return self._format, self._data, self._width


@dataclass(frozen=True, slots=True)
class V8ElementData:
    """Native parameters for a supported V8 element kind.

    ``vertices``, ``orientations``, ``faces`` and ``values`` are sequences.
    Data read from a file holds them packed (:class:`V8PointArray`,
    :class:`V8MeshFaceArray`, :class:`V8ValueArray`); data built by hand can
    use plain tuples, and the two compare equal.
    """

    kind: str
    vertices: Sequence[V8Point] = ()
    orientations: Sequence[tuple[float, float, float, float]] = ()
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
    style: int | None = None
    faces: Sequence[V8MeshFace] = ()
    columns: int | None = None
    values: Sequence[float] = ()

    @property
    def rotation_degrees(self) -> float | None:
        if self.rotation_radians is None:
            return None
        return math.degrees(self.rotation_radians)

    @property
    def quaternion(self) -> tuple[float, float, float, float] | None:
        """The stored orientation quaternion ``(w, x, y, z)`` of a 3D element.

        A 3D ellipse or arc stores the four values alone. A 3D text stores two
        other values first, so the quaternion is the last four of its six
        ``orientation`` values. A 2D element has ``rotation_radians`` and no
        quaternion.
        """

        values = self.orientation
        if self.kind == "TEXT" and len(values) == 6:
            values = values[2:]
        elif not (self.kind in {"ELLIPSE", "ARC"} and len(values) == 4):
            return None
        return (values[0], values[1], values[2], values[3])

    def place_definition_point(self, point: Point3) -> Point3 | None:
        """Place a point of a shared-cell definition with this instance.

        ``point`` is in the definition's own space, in UOR. The result is
        ``origin + M * point`` in the UOR of the model, where ``M`` is the 3x3
        matrix that the last nine ``transform`` values hold by rows. Returns
        ``None`` for another element kind and for an instance that does not
        hold fifteen values.
        """

        if (
            self.kind != "SHARED_CELL_INSTANCE"
            or self.origin is None
            or len(self.transform) != 15
        ):
            return None
        matrix = self.transform[6:]
        x, y, z = point
        origin = self.origin.uor
        return (
            origin[0] + (matrix[0] * x + matrix[1] * y + matrix[2] * z),
            origin[1] + (matrix[3] * x + matrix[4] * y + matrix[5] * z),
            origin[2] + (matrix[6] * x + matrix[7] * y + matrix[8] * z),
        )

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
class V8TagData(V8ElementData):
    """The data of a ``TAG`` element: one value for one definition of a tag set.

    ``value`` is a ``str`` for a text value and an ``int`` for an integer
    value; it is ``None`` for a value the reader does not decode, and
    ``value_bytes`` always holds the stored bytes. ``tag_set_element_id`` is
    the element ID of the tag set and ``tag_id`` the definition in it.
    ``host_element_id`` is the element the tag is attached to. ``origin`` is
    given only when the stored point lies inside the stored range of the tag.
    Only a ``TAG`` element has these fields, so the other elements of a
    document stay as small as before.
    """

    tag_id: int | None = None
    value_type: int | None = None
    value_bytes: bytes | None = None
    value: str | int | None = None
    tag_set_element_id: int | None = None
    host_element_id: int | None = None


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
    def item_instances(self) -> tuple[V8ItemInstance, ...]:
        """The item instances attached to this element, in stored order.

        They are decoded from ``auxiliary_records`` on each access. An entry
        that does not match the observed layout is not listed; it stays
        available through the entries of its record. One access returns at
        most ``V8_MAX_ITEM_BYTES`` stored bytes as instances;
        ``decode_item_instances(element.auxiliary_records)`` also tells how
        many item entries that limit left out.
        """

        if not self.auxiliary_records:
            return ()
        return decode_item_instances(self.auxiliary_records).instances

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
            "SHARED_CELL_DEFINITION",
            "TAG",
            "BSPLINE_POLE",
            "TEXT_NODE",
            "MATRIX_HEADER",
            "MATRIX_INTEGER_DATA",
            "MATRIX_DOUBLE_DATA",
        }

    def dxftype(self) -> str:
        """Return the stable uppercase entity label used by ``query`` and CLI."""

        return self.kind


@dataclass(frozen=True, slots=True)
class V8SharedCellDefinition:
    """One shared-cell definition: its header element, then its components.

    The coordinates are in the definition's own space; the master values use
    the scale of the model the definitions were read for, with no global
    origin. ``index``, ``parent_index`` and ``child_indices`` of the elements
    are positions in ``elements``. A shared-cell instance places a definition
    point ``p`` at ``origin + M * p``, where ``M`` is the 3x3 matrix by rows
    in the last nine of the instance's fifteen ``transform`` values.
    """

    name: str
    elements: tuple[V8Element, ...]

    @property
    def header(self) -> V8Element | None:
        return self.elements[0] if self.elements else None

    @property
    def range_values(self) -> tuple[float, ...] | None:
        """The six stored range values: low XYZ, then high XYZ."""

        header = self.header
        if header is None or header.kind != "SHARED_CELL_DEFINITION":
            return None
        values = header.data.transform[:6]
        return values if len(values) == 6 else None

    def matches(self, instance: V8Element) -> bool:
        """Whether a shared-cell instance refers to this definition.

        An instance repeats the name and the six range values of its
        definition. Both must be equal: one file can hold several definitions
        with one name. The values are compared bit for bit, so ``-0.0`` does
        not match ``0.0``.
        """

        data = instance.data
        range_values = self.range_values
        return (
            data.kind == "SHARED_CELL_INSTANCE"
            and data.name is not None
            and data.name == self.name
            and len(data.transform) == 15
            and range_values is not None
            and struct.pack("<6d", *range_values)
            == struct.pack("<6d", *data.transform[:6])
        )


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
class V8Level:
    """One level-table entry: a type-95 named object whose ``level`` field is 1.

    ``level_id`` is the number graphical headers store in their ``level`` field;
    ``number`` is the user-visible level number. ``line_style``, ``line_weight``
    and ``color_index`` are the level's own ("ByLevel") symbology, which an
    element with the ByLevel sentinel (``0xffffffff``) in its header takes over.
    """

    raw: V8RawObject
    level_id: int
    number: int
    name: str
    line_style: int
    line_weight: int
    color_index: int


@dataclass(frozen=True, slots=True)
class V8LevelTable:
    """One level table: a table header and the level entries that follow it.

    Level IDs are unique within a table only. ``header`` is the type-96 named
    object that opens the table, or ``None`` for entries that follow no level
    table header. ``levels`` holds the entries in file order and is empty for
    a header without entries.
    """

    header: V8RawObject | None
    levels: tuple[V8Level, ...]

    def level(self, level_id: int) -> V8Level | None:
        """The first entry of this table with ``level_id``, if it defines one."""

        for item in self.levels:
            if item.level_id == level_id:
                return item
        return None


LEVEL_TABLE_ELEMENT_TYPE = 95
LEVEL_TABLE_HEADER_ELEMENT_TYPE = 96
LEVEL_TABLE_KIND = 1
NAME_LINKAGE_SIGNATURE = 0x56D2
# Symbology sentinels in V8 headers and level entries.
SYMBOLOGY_BY_LEVEL = 0xFFFFFFFF
SYMBOLOGY_BY_CELL = 0xFFFFFFFE
_LEVEL_ENTRY_BYTES = 0x60
_COMPONENT_ROLES = ("component", "header_component")


def level_table(raw: V8RawDocument) -> tuple[V8Level, ...]:
    """Decode the level-table entries of a scanned document (see FORMAT_NOTES).

    Every entry of every level table is returned in file order; use
    ``level_tables()`` to keep the entries of each table apart.
    """

    levels: list[V8Level] = []
    for page in raw.named_pages:
        for item in page.objects:
            if _is_level_entry(item):
                level = _level_from_entry(item)
                if level is not None:
                    levels.append(level)
    return tuple(levels)


def level_tables(raw: V8RawDocument) -> tuple[V8LevelTable, ...]:
    """Group the level-table entries of a scanned document by their table.

    The named objects are read in file order, across pages. A type-96 header
    that is not itself a component ends the table before it, and starts a new
    one when its ``level`` word is 1 (the level table kind). A level entry
    joins the table that is open, or starts a table without a header when none
    is. Any other object that is not a component ends the open table. Tables
    are never merged, so the entries of all tables, in order, are
    ``level_table(raw)``.
    """

    tables: list[tuple[V8RawObject | None, list[V8Level]]] = []
    table_is_open = False
    for page in raw.named_pages:
        for item in page.objects:
            if _is_level_entry(item):
                if not table_is_open:
                    tables.append((None, []))
                    table_is_open = True
                level = _level_from_entry(item)
                if level is not None:
                    tables[-1][1].append(level)
            elif item.role not in _COMPONENT_ROLES:
                table_is_open = _is_level_table_header(item)
                if table_is_open:
                    tables.append((item, []))
    return tuple(
        V8LevelTable(header=header, levels=tuple(levels))
        for header, levels in tables
        if header is not None or levels
    )


def _is_level_entry(item: V8RawObject) -> bool:
    return (
        item.element_type == LEVEL_TABLE_ELEMENT_TYPE
        and item.level == LEVEL_TABLE_KIND
    )


def _is_level_table_header(item: V8RawObject) -> bool:
    return (
        item.element_type == LEVEL_TABLE_HEADER_ELEMENT_TYPE
        and item.level == LEVEL_TABLE_KIND
    )


def _level_from_entry(item: V8RawObject) -> V8Level | None:
    primary = item.primary_bytes
    if len(primary) < _LEVEL_ENTRY_BYTES:
        return None
    return V8Level(
        raw=item,
        level_id=_u32(primary, 0x20),
        number=_u32(primary, 0x5C),
        name=_name_linkage(item.attribute_bytes),
        line_style=_i32(primary, 0x40),
        line_weight=_u32(primary, 0x44),
        color_index=_u32(primary, 0x48),
    )


def _u32(data: bytes, offset: int) -> int:
    return int.from_bytes(data[offset : offset + 4], "little", signed=False)


def _i32(data: bytes, offset: int) -> int:
    return int.from_bytes(data[offset : offset + 4], "little", signed=True)


def _name_linkage(attributes: bytes) -> str:
    """The name stored in the ``0x56d2`` linkage of a table entry, or ``""``.

    Linkages are word-framed (byte length ``2 * (low byte + 1)`` of the first
    word). The name linkage holds a version, the payload byte length and the
    payload: ``ff fd`` + UTF-16LE, ``ff fe`` + a code-page word + 8-bit text, or
    plain 8-bit text.
    """

    position = 0
    while position + 12 <= len(attributes):
        header = int.from_bytes(attributes[position : position + 2], "little")
        length = 2 * ((header & 0xFF) + 1)
        signature = int.from_bytes(attributes[position + 2 : position + 4], "little")
        if signature == NAME_LINKAGE_SIGNATURE:
            size = _u32(attributes, position + 8)
            payload = attributes[position + 12 : position + 12 + size]
            return _decode_name(payload)
        if length <= 0:
            break
        position += length
    return ""


def _decode_name(payload: bytes) -> str:
    if payload[:2] == b"\xff\xfd":
        text = payload[2:].decode("utf-16-le", "replace")
    elif payload[:2] == b"\xff\xfe":
        text = payload[4:].decode("cp1252", "replace")
    else:
        text = payload.decode("cp1252", "replace")
    return text.rstrip("\x00")


@dataclass(frozen=True, slots=True)
class V8TagDefinition:
    """One tag definition of a tag set.

    ``tag_id`` is the number a tag element stores to name this definition.
    ``value_type`` is ``V8_TAG_VALUE_TEXT`` or ``V8_TAG_VALUE_INTEGER`` and
    ``default`` is a ``str`` or an ``int`` to match.
    """

    name: str
    tag_id: int
    prompt: str
    value_type: int
    default: str | int


@dataclass(frozen=True, slots=True)
class V8TagSet:
    """One tag set: a named object that holds the definitions of its tags.

    ``element_id`` is the element ID of the named object; a ``TAG`` element
    refers to it with the ``tag_set_element_id`` of its ``V8TagData``.
    """

    element_id: int
    name: str
    definitions: tuple[V8TagDefinition, ...]

    def definition(self, tag_id: int) -> V8TagDefinition | None:
        """Return the first definition with this tag ID."""

        return next((item for item in self.definitions if item.tag_id == tag_id), None)


TAG_SET_ELEMENT_TYPE = 39


def _iter_tag_set_objects(raw: V8RawDocument) -> Iterator[V8RawObject]:
    # The code is read from the raw bytes when the primary part covers it:
    # eight bytes for each object, and no copy of its primary bytes.
    return (
        item
        for page in raw.named_pages
        for item in page.objects
        if item.element_type == TAG_SET_ELEMENT_TYPE
        and item.attribute_words * 2 >= 0x30
        and item.raw_bytes[0x28:0x30] == V8_TAG_SET_CODE
    )


def tag_set_objects(raw: V8RawDocument) -> tuple[V8RawObject, ...]:
    """The named objects that carry the tag-set code, decodable or not."""

    return tuple(_iter_tag_set_objects(raw))


def decode_tag_set(item: V8RawObject) -> V8TagSet | None:
    """Decode a named object that holds a tag set.

    Returns ``None`` for another object and for bytes that do not match the
    observed layout exactly; the object keeps its ``raw_bytes`` either way.
    """

    if item.element_type != TAG_SET_ELEMENT_TYPE or item.element_id is None:
        return None
    row = cast(
        "dict[str, Any] | None",
        _decode_v8_tag_set_data(
            item.element_id, item.primary_bytes, item.attribute_bytes
        ),
    )
    return None if row is None else _tag_set_from_core(row)


def _tag_set_from_core(row: dict[str, Any]) -> V8TagSet:
    return V8TagSet(
        element_id=row["element_id"],
        name=row["name"],
        definitions=tuple(
            V8TagDefinition(
                name=definition["name"],
                tag_id=definition["tag_id"],
                prompt=definition["prompt"],
                value_type=definition["value_type"],
                default=definition["default"],
            )
            for definition in row["definitions"]
        ),
    )


def tag_sets(raw: V8RawDocument) -> tuple[V8TagSet, ...]:
    """Decode the tag sets of a scanned document, in stored order.

    A tag set that does not match the observed layout is not listed; it stays
    among ``tag_set_objects(raw)``. One call reads at most
    ``V8_MAX_TAG_SET_BYTES`` bytes in sum. An object costs the definition
    bytes it declares plus its attribute bytes, which hold its name. An object
    that cannot decode within the limit is passed over and costs nothing: its
    two stored sizes differ, they declare more than the limit, its primary
    bytes do not end with the declared definition bytes, or its own cost is
    over the limit. The first other object that would pass the sum ends
    the call, and it and the objects after it are not listed. The core applies
    this limit; this function has no second copy of it.
    """

    rows = cast(
        "list[dict[str, Any]]",
        _decode_v8_tag_sets_data(
            [
                (item.element_id, item.raw_bytes, item.attribute_words * 2)
                for item in _iter_tag_set_objects(raw)
                if item.element_id is not None
            ]
        ),
    )
    return tuple(_tag_set_from_core(row) for row in rows)


@dataclass(frozen=True, slots=True)
class V8Document:
    raw: V8RawDocument
    models: tuple[V8Model, ...]

    @property
    def entities(self) -> tuple[V8Element, ...]:
        return tuple(element for model in self.models for element in model.entities)

    @property
    def levels(self) -> tuple[V8Level, ...]:
        """The level table (``0.2.8``): an entry per defined level, in file order."""

        return level_table(self.raw)

    @property
    def level_tables(self) -> tuple[V8LevelTable, ...]:
        """The level tables of the file, in file order (see ``level_tables()``)."""

        return level_tables(self.raw)

    def level(self, level_id: int) -> V8Level | None:
        """The level-table entry a header's ``level`` number refers to, if defined.

        This is the first entry with that ID in file order. A file with several
        level tables can define the ID more than once: see ``level_candidates()``.
        """

        for item in self.levels:
            if item.level_id == level_id:
                return item
        return None

    def level_candidates(self, level_id: int) -> tuple[V8Level, ...]:
        """The entry for ``level_id`` in each level table that defines it.

        The entries are in file order, one per table. More than one entry
        means the tables disagree on, or repeat, what the ID stands for.
        """

        found = (table.level(level_id) for table in self.level_tables)
        return tuple(item for item in found if item is not None)

    @property
    def tag_sets(self) -> tuple[V8TagSet, ...]:
        """The tag sets of the document, decoded on each access.

        Every access decodes every tag set again, so read it once and keep
        the result. ``tag_sets_by_id()`` gives the same sets as a mapping.
        """

        return tag_sets(self.raw)

    def tag_sets_by_id(self) -> dict[int, V8TagSet]:
        """The decodable tag sets by element ID, for the lookup of many tags.

        One call decodes every tag set once, like ``tag_sets``. Keep the
        mapping and look each tag up with
        ``mapping.get(element.data.tag_set_element_id)``. When more than one
        tag-set object has an element ID, the mapping holds the first one
        that decodes, as ``tag_set()`` returns it.

        There is one difference from ``tag_set()``. This call has the limit
        of ``tag_sets`` for all sets together, so a set that the limit
        leaves out is not in the mapping, and ``tag_set()`` can still
        return it.
        """

        result: dict[int, V8TagSet] = {}
        for item in self.tag_sets:
            result.setdefault(item.element_id, item)
        return result

    def tag_set(self, element_id: int | None) -> V8TagSet | None:
        """The tag set a tag's ``tag_set_element_id`` refers to, if decodable.

        One call walks the named objects once and returns the first tag-set
        object with this element ID that decodes. It does not decode the
        sets with another ID, and nothing is kept between calls. To look up
        the sets of many tags, build the mapping once with
        ``tag_sets_by_id()`` and do not call this for each tag.

        The cost of one call: every object with this ID is decoded in turn
        until one decodes. A file with K objects of one ID that each fail at
        their last byte costs K decodes of up to ``V8_MAX_TAG_SET_BYTES``
        each. That is in line with the size of the file, and there is no
        other limit.

        The result is the set that ``tag_sets_by_id()`` holds for this ID,
        with one difference: each set is decoded alone here, with the limit
        of one set, so a set that the limit of one ``tag_sets`` call leaves
        out of the mapping is still returned.
        """

        if element_id is None:
            return None
        for item in _iter_tag_set_objects(self.raw):
            if item.element_id == element_id:
                decoded = decode_tag_set(item)
                if decoded is not None:
                    return decoded
        return None

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


def read_v8_shared_cell_definitions(
    source: DgnSource,
    *,
    limits: V8ScanLimits | None = None,
    model_index: int = 0,
) -> tuple[V8SharedCellDefinition, ...]:
    """Read the shared-cell definitions of a V8 file.

    They are not part of ``read_v8()``: a file can hold many more definition
    components than drawn elements. ``model_index`` selects the model whose
    unit scale gives the master values. A ``model_index`` that is negative or
    not a model of the document raises ``ValueError``. Each call reads the
    file again and builds a new result: keep the result, and note that the
    results of one call for each model hold the definitions once for each
    model.
    """

    actual_limits = limits or V8ScanLimits()
    if model_index < 0:
        raise ValueError("model_index must not be negative")
    data = _read_all(source, max_file_size=actual_limits.max_file_size)
    rows = cast(
        "list[dict[str, Any]]",
        _read_v8_shared_cells(data, actual_limits._native_row(), model_index),
    )
    return tuple(
        V8SharedCellDefinition(
            name=row["name"],
            elements=tuple(
                V8Element(
                    index=element["index"],
                    raw=_raw_object_from_core(element["raw"]),
                    common=_common_from_core(element["common"]),
                    data=_element_data_from_core(element["data"]),
                    parent_index=element["parent_index"],
                    child_indices=tuple(element["child_indices"]),
                    linkages=tuple(
                        _linkage_from_core(item) for item in element["linkages"]
                    ),
                    auxiliary_records=tuple(
                        _aux_record_from_core(item)
                        for item in element["auxiliary_records"]
                    ),
                )
                for element in row["elements"]
            ),
        )
        for row in rows
    )


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


def _aux_entries_from_core(
    rows: list[dict[str, Any]] | None,
) -> tuple[V8AuxiliaryEntry, ...] | None:
    if rows is None:
        return None
    return tuple(
        V8AuxiliaryEntry(
            index=row["index"],
            offset=row["offset"],
            type_id=row["type_id"],
            entry_id=row["entry_id"],
            size_word=row["size_word"],
            reserved=row["reserved"],
            data=row["data"],
        )
        for row in rows
    )


def _item_instance_from_core(row: dict[str, Any]) -> V8ItemInstance:
    return V8ItemInstance(
        entry_id=row["entry_id"],
        schema_element_id=row["schema_element_id"],
        class_name=row["class_name"],
        # The core gives tuples of pairs. Every use of one table string is
        # the same ``str`` object, so the pairs are kept as they are.
        attributes=row["attributes"],
        properties=row["properties"],
    )


def decode_auxiliary_entries(
    payload: bytes, entry_count: int
) -> tuple[V8AuxiliaryEntry, ...] | None:
    """Split an auxiliary payload that declares ``entry_count`` entries.

    Returns ``None`` when the payload does not hold exactly that many entries
    followed by the closing word. Nothing is guessed or skipped. A count above
    ``V8_MAX_AUXILIARY_ENTRIES`` also returns ``None``.
    """

    if not 0 <= entry_count <= 0xFFFF_FFFF:
        return None
    return _aux_entries_from_core(
        cast(
            "list[dict[str, Any]] | None",
            _decode_v8_auxiliary_payload(bytes(payload), entry_count),
        )
    )


def decode_item_instance(entry: V8AuxiliaryEntry) -> V8ItemInstance | None:
    """Decode an entry of type ``V8_ITEM_INSTANCE_ENTRY_TYPE``.

    Returns ``None`` for another entry type, for data that does not match the
    observed layout exactly, and for data of more than ``V8_MAX_ITEM_BYTES``
    bytes; the entry keeps its raw ``data`` in every case.
    """

    if entry.type_id != V8_ITEM_INSTANCE_ENTRY_TYPE:
        return None
    row = _decode_v8_item_instance_data(entry.entry_id, entry.data)
    return None if row is None else _item_instance_from_core(cast("dict[str, Any]", row))


def decode_item_instances(records: Iterable[V8AuxiliaryRecord]) -> V8ItemInstances:
    """Decode the item instances of the auxiliary records of one element.

    The instances are in stored order. An entry of another type, an item
    entry that does not match the observed layout and a record whose payload
    does not split into entries are not listed and cost nothing.

    A table string is decoded once and every use of it is a reference to one
    ``str``, so the memory of the result is at most a fixed multiple of the
    stored bytes: 16 requested bytes for one stored byte on a 64-bit CPython.
    The overhead of the allocator is not in that number; it can add up to
    about 2 times for small blocks. One call returns at most
    ``V8_MAX_ITEM_BYTES`` stored bytes as instances: the data of each
    returned instance counts, and an item entry whose data is larger than
    what is left is counted in ``over_limit`` and stays raw, also when it is
    damaged. The entries after it are still read.
    """

    rows, over_limit = _decode_v8_item_instances_data(
        [
            (bytes(record.payload), record.flags)
            for record in records
            if 0 <= record.flags <= 0xFFFF_FFFF
        ]
    )
    return V8ItemInstances(
        instances=tuple(
            _item_instance_from_core(cast("dict[str, Any]", row)) for row in rows
        ),
        over_limit=over_limit,
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
        skipped_models=tuple(row.get("skipped_models", ())),
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


def _is_packed(row: Any, buffers: int) -> bool:
    return (
        isinstance(row, tuple)
        and len(row) == buffers
        and all(isinstance(item, bytes) for item in row)
    )


def _points_from_core(row: Any) -> Sequence[V8Point]:
    """Packed ``(uor, master)`` buffers, or one ``(uor, master)`` row per point."""

    if _is_packed(row, 2):
        return V8PointArray(*row)
    return tuple(_point_from_core(item) for item in row)


def _faces_from_core(row: Any) -> Sequence[V8MeshFace]:
    """Packed ``(offsets, point_indices, visible_edges)``, or one row per face."""

    if _is_packed(row, 3):
        return V8MeshFaceArray(*row)
    return tuple(
        V8MeshFace(tuple(point_indices), tuple(visible_edges))
        for point_indices, visible_edges in row
    )


def _values_from_core(row: Any) -> Sequence[float]:
    """A packed ``(format, data)`` pair, or the numbers themselves."""

    if isinstance(row, tuple) and len(row) == 2 and isinstance(row[1], bytes):
        return V8ValueArray(*row)
    return tuple(row)


def _orientations_from_core(
    row: Any,
) -> Sequence[tuple[float, float, float, float]]:
    """Packed ``float64`` quadruples, or one row of four numbers per point."""

    if isinstance(row, bytes):
        return V8ValueArray("d", row, _ORIENTATION_VALUES)
    return tuple(tuple(item) for item in row)


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
    if row["kind"] == "TAG":
        return V8TagData(
            kind="TAG",
            origin=(
                _point_from_core(row["origin"]) if row["origin"] is not None else None
            ),
            tag_id=row["tag_id"],
            value_type=row["value_type"],
            value_bytes=row["value_bytes"],
            value=row["value"],
            tag_set_element_id=row["tag_set_element_id"],
            host_element_id=row["host_element_id"],
        )
    return V8ElementData(
        kind=row["kind"],
        vertices=_points_from_core(row.get("vertices", ())),
        orientations=_orientations_from_core(row.get("orientations", ())),
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
        style=row.get("style"),
        faces=_faces_from_core(row.get("faces", ())),
        columns=row.get("columns"),
        values=_values_from_core(row.get("values", ())),
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
    "V8AuxiliaryEntry",
    "V8AuxiliaryPage",
    "V8AuxiliaryRecord",
    "V8CfbEntry",
    "V8CommonHeader",
    "V8ContainerInfo",
    "V8Document",
    "V8Element",
    "V8ElementData",
    "V8ItemInstance",
    "V8ItemInstances",
    "V8Level",
    "V8LevelTable",
    "V8Linkage",
    "V8MeshFace",
    "V8MeshFaceArray",
    "V8Model",
    "V8ModelIndexEntry",
    "V8ModelMetadata",
    "V8ObjectPage",
    "V8PageHeader",
    "V8Point",
    "V8PointArray",
    "V8RawDocument",
    "V8RawModel",
    "V8RawObject",
    "V8ScanLimits",
    "V8SharedCellDefinition",
    "V8TagData",
    "V8TagDefinition",
    "V8TagSet",
    "V8ValueArray",
    "LEVEL_TABLE_ELEMENT_TYPE",
    "LEVEL_TABLE_HEADER_ELEMENT_TYPE",
    "LEVEL_TABLE_KIND",
    "NAME_LINKAGE_SIGNATURE",
    "SYMBOLOGY_BY_CELL",
    "SYMBOLOGY_BY_LEVEL",
    "TAG_SET_ELEMENT_TYPE",
    "V8_ITEM_INSTANCE_ENTRY_TYPE",
    "V8_MAX_AUXILIARY_ENTRIES",
    "V8_MAX_ITEM_BYTES",
    "V8_MAX_TAG_SET_BYTES",
    "V8_TAG_SET_CODE",
    "V8_TAG_VALUE_INTEGER",
    "V8_TAG_VALUE_TEXT",
    "decode_auxiliary_entries",
    "decode_item_instance",
    "decode_item_instances",
    "decode_tag_set",
    "inspect_v8_container",
    "level_table",
    "level_tables",
    "read_v8",
    "read_v8_shared_cell_definitions",
    "read_v8file",
    "scan_v8_objects",
    "tag_set_objects",
    "tag_sets",
]
