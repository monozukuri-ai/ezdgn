"""Seed-based V7 2D document creation."""

from __future__ import annotations

import math
import os
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, BinaryIO, Iterator, Literal, TypeAlias, cast

from ._core import InvalidDgnError, UnsupportedDgnError
from ._core import write_v7_2d_bytes as _write_v7_2d_bytes
from .entities import Drawing, read
from .metadata import DesignSettings, read_design_settings
from .raw import (
    DEFAULT_MAX_FILE_SIZE_BYTES,
    DgnSource,
    PathSource,
    _read_all,
    scan_records,
)

Point2: TypeAlias = tuple[float, float]
WriterKind: TypeAlias = Literal[
    "LINE",
    "LINE_STRING",
    "SHAPE",
    "CURVE",
    "ELLIPSE",
    "ARC",
    "TEXT",
]
WriteStyleRow: TypeAlias = tuple[int, int, int, int, int, int]
WriteEntityRow: TypeAlias = tuple[
    str,
    list[Point2],
    list[float],
    bytes,
    tuple[int, int],
    WriteStyleRow,
    int | None,
]


@dataclass(frozen=True, slots=True)
class DgnAttributes:
    """Common V7 display attributes for a newly written element."""

    level: int = 1
    color: int = 0
    line_style: int = 0
    line_weight: int = 0
    graphic_group: int = 0
    properties: int = 0x0200

    def __post_init__(self) -> None:
        _bounded_int("level", self.level, 0, 63)
        _bounded_int("color", self.color, 0, 255)
        _bounded_int("line_style", self.line_style, 0, 7)
        _bounded_int("line_weight", self.line_weight, 0, 31)
        _bounded_int("graphic_group", self.graphic_group, 0, 65_535)
        _bounded_int("properties", self.properties, 0, 65_535)

    def _core_row(self) -> WriteStyleRow:
        return (
            self.level,
            self.color,
            self.line_style,
            self.line_weight,
            self.graphic_group,
            self.properties,
        )


@dataclass(frozen=True, slots=True)
class V7WriteEntity:
    """One immutable native entity queued for the V7 writer."""

    kind: WriterKind
    points: tuple[Point2, ...]
    parameters: tuple[float, ...] = ()
    text_bytes: bytes = b""
    font_id: int = 0
    justification: int = 0
    dgnattribs: DgnAttributes = field(default_factory=DgnAttributes)
    fill_color: int | None = None

    def dxftype(self) -> str:
        """Return the stable entity name, matching reader entities."""

        return self.kind

    def _core_row(self) -> WriteEntityRow:
        return (
            self.kind,
            list(self.points),
            list(self.parameters),
            self.text_bytes,
            (self.font_id, self.justification),
            self.dgnattribs._core_row(),
            self.fill_color,
        )


class Modelspace:
    """Mutable collection of entities to append after the seed controls."""

    __slots__ = ("_document",)

    def __init__(self, document: V7Document) -> None:
        self._document = document

    def __iter__(self) -> Iterator[V7WriteEntity]:
        return iter(self._document._entities)

    def __len__(self) -> int:
        return len(self._document._entities)

    def query(self, kind: str) -> tuple[V7WriteEntity, ...]:
        normalized = kind.strip().upper().replace("-", "_").replace(" ", "_")
        return tuple(entity for entity in self if entity.kind == normalized)

    def add_line(
        self,
        start: Iterable[float],
        end: Iterable[float],
        *,
        dgnattribs: DgnAttributes | Mapping[str, Any] | None = None,
    ) -> V7WriteEntity:
        return self._append(
            V7WriteEntity(
                "LINE",
                (_point(start, "start"), _point(end, "end")),
                dgnattribs=_attributes(dgnattribs),
            )
        )

    def add_line_string(
        self,
        points: Iterable[Iterable[float]],
        *,
        dgnattribs: DgnAttributes | Mapping[str, Any] | None = None,
    ) -> V7WriteEntity:
        return self._append(
            V7WriteEntity(
                "LINE_STRING",
                _point_sequence(points, "line string", minimum=2),
                dgnattribs=_attributes(dgnattribs),
            )
        )

    add_linestring = add_line_string

    def add_shape(
        self,
        points: Iterable[Iterable[float]],
        *,
        close: bool = True,
        fill_color: int | None = None,
        dgnattribs: DgnAttributes | Mapping[str, Any] | None = None,
    ) -> V7WriteEntity:
        vertices = list(_point_sequence(points, "shape", minimum=3))
        if close and vertices[0] != vertices[-1]:
            vertices.append(vertices[0])
        if len(vertices) > 101:
            raise ValueError("shape supports at most 101 points after closure")
        if fill_color is not None:
            _bounded_int("fill_color", fill_color, 0, 255)
        return self._append(
            V7WriteEntity(
                "SHAPE",
                tuple(vertices),
                dgnattribs=_attributes(dgnattribs),
                fill_color=fill_color,
            )
        )

    def add_curve(
        self,
        control_points: Iterable[Iterable[float]],
        *,
        dgnattribs: DgnAttributes | Mapping[str, Any] | None = None,
    ) -> V7WriteEntity:
        return self._append(
            V7WriteEntity(
                "CURVE",
                _point_sequence(control_points, "curve", minimum=2),
                dgnattribs=_attributes(dgnattribs),
            )
        )

    def add_ellipse(
        self,
        center: Iterable[float],
        primary_axis: float,
        secondary_axis: float | None = None,
        *,
        rotation: float = 0.0,
        dgnattribs: DgnAttributes | Mapping[str, Any] | None = None,
    ) -> V7WriteEntity:
        primary = _positive_float("primary_axis", primary_axis)
        secondary = primary if secondary_axis is None else _positive_float(
            "secondary_axis", secondary_axis
        )
        return self._append(
            V7WriteEntity(
                "ELLIPSE",
                (_point(center, "center"),),
                (primary, secondary, _finite_float("rotation", rotation)),
                dgnattribs=_attributes(dgnattribs),
            )
        )

    def add_circle(
        self,
        center: Iterable[float],
        radius: float,
        *,
        dgnattribs: DgnAttributes | Mapping[str, Any] | None = None,
    ) -> V7WriteEntity:
        return self.add_ellipse(
            center,
            radius,
            radius,
            dgnattribs=dgnattribs,
        )

    def add_arc(
        self,
        center: Iterable[float],
        primary_axis: float,
        secondary_axis: float | None = None,
        *,
        start_angle: float = 0.0,
        sweep_angle: float = 360.0,
        rotation: float = 0.0,
        dgnattribs: DgnAttributes | Mapping[str, Any] | None = None,
    ) -> V7WriteEntity:
        primary = _positive_float("primary_axis", primary_axis)
        secondary = primary if secondary_axis is None else _positive_float(
            "secondary_axis", secondary_axis
        )
        sweep = _finite_float("sweep_angle", sweep_angle)
        if not -360.0 <= sweep <= 360.0:
            raise ValueError("sweep_angle must be between -360 and 360")
        return self._append(
            V7WriteEntity(
                "ARC",
                (_point(center, "center"),),
                (
                    primary,
                    secondary,
                    _finite_float("rotation", rotation),
                    _finite_float("start_angle", start_angle),
                    sweep,
                ),
                dgnattribs=_attributes(dgnattribs),
            )
        )

    def add_text(
        self,
        text: str | bytes | bytearray | memoryview,
        insert: Iterable[float],
        *,
        height: float = 1.0,
        width: float | None = None,
        rotation: float = 0.0,
        font_id: int = 1,
        justification: int = 0,
        encoding: str = "ascii",
        errors: str = "strict",
        dgnattribs: DgnAttributes | Mapping[str, Any] | None = None,
    ) -> V7WriteEntity:
        if isinstance(text, str):
            payload = text.encode(encoding, errors)
        elif isinstance(text, (bytes, bytearray, memoryview)):
            payload = bytes(text)
        else:
            raise TypeError("text must be str or bytes-like")
        if len(payload) > 255:
            raise ValueError("encoded V7 text must be at most 255 bytes")
        text_height = _positive_float("height", height)
        text_width = text_height if width is None else _positive_float("width", width)
        _bounded_int("font_id", font_id, 0, 255)
        _bounded_int("justification", justification, 0, 255)
        return self._append(
            V7WriteEntity(
                "TEXT",
                (_point(insert, "insert"),),
                (
                    text_width,
                    text_height,
                    _finite_float("rotation", rotation),
                ),
                payload,
                font_id,
                justification,
                _attributes(dgnattribs),
            )
        )

    def _append(self, entity: V7WriteEntity) -> V7WriteEntity:
        self._document._entities.append(entity)
        return entity


@dataclass(slots=True)
class V7Document:
    """Mutable seed-backed V7 2D document ready to serialize."""

    _seed_bytes: bytes = field(repr=False)
    design_settings: DesignSettings
    copy_color_table: bool = True
    copy_seed_elements: bool = False
    source_path: str | None = None
    _entities: list[V7WriteEntity] = field(default_factory=list, repr=False)
    _modelspace: Modelspace = field(init=False, repr=False)

    def __post_init__(self) -> None:
        self._modelspace = Modelspace(self)

    @property
    def entities(self) -> tuple[V7WriteEntity, ...]:
        return tuple(self._entities)

    def modelspace(self) -> Modelspace:
        return self._modelspace

    def to_bytes(self) -> bytes:
        rows = [entity._core_row() for entity in self._entities]
        return cast(
            bytes,
            _write_v7_2d_bytes(
                self._seed_bytes,
                rows,
                self.copy_color_table,
                self.copy_seed_elements,
            ),
        )

    def write(self, stream: BinaryIO) -> int:
        """Write to an open binary stream and return the byte count."""

        if not hasattr(stream, "write"):
            raise TypeError("write() requires a binary stream")
        result = stream.write(self.to_bytes())
        if result is None:
            return 0
        return int(result)

    def saveas(self, path: PathSource) -> None:
        """Serialize the document to a filesystem path."""

        if not isinstance(path, (str, os.PathLike)):
            raise TypeError("saveas() requires a filesystem path")
        Path(path).write_bytes(self.to_bytes())

    def readback(self) -> Drawing:
        """Serialize and parse again with ezdgn's native reader."""

        return read(self.to_bytes())


def new(
    seed: DgnSource,
    *,
    copy_color_table: bool = True,
    copy_seed_elements: bool = False,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
) -> V7Document:
    """Create a mutable V7 2D document using *seed* as its design settings."""

    source_path = os.fspath(seed) if isinstance(seed, (str, os.PathLike)) else None
    data = _read_all(seed, max_file_size=max_file_size)
    scan = scan_records(data, max_file_size=max_file_size)
    if scan.format.dimension != 2:
        raise UnsupportedDgnError("V7 3D seeds are not supported by the 2D writer")
    if (
        len(scan.records) < 3
        or [record.element_type for record in scan.records[:3]] != [9, 8, 10]
        or any(record.deleted for record in scan.records[:3])
    ):
        raise InvalidDgnError(
            "V7 writer seed must begin with TCB, digitizer setup, and level symbology"
        )
    settings = read_design_settings(data, max_file_size=max_file_size)
    if settings.scale is None:
        raise InvalidDgnError("V7 writer seed TCB has a zero UOR scale")
    return V7Document(
        data,
        settings,
        bool(copy_color_table),
        bool(copy_seed_elements),
        source_path,
    )


def _attributes(
    value: DgnAttributes | Mapping[str, Any] | None,
) -> DgnAttributes:
    if value is None:
        return DgnAttributes()
    if isinstance(value, DgnAttributes):
        return value
    if not isinstance(value, Mapping):
        raise TypeError("dgnattribs must be DgnAttributes, a mapping, or None")
    allowed = {
        "level",
        "color",
        "line_style",
        "line_weight",
        "graphic_group",
        "properties",
    }
    unknown = set(value) - allowed
    if unknown:
        names = ", ".join(sorted(map(str, unknown)))
        raise ValueError(f"unsupported DGN attributes: {names}")
    return DgnAttributes(**dict(value))


def _point(value: Iterable[float], name: str) -> Point2:
    if isinstance(value, (str, bytes, bytearray, memoryview)):
        raise TypeError(f"{name} must contain exactly two coordinates")
    try:
        coordinates = tuple(value)
    except TypeError as error:
        raise TypeError(f"{name} must contain exactly two coordinates") from error
    if len(coordinates) != 2:
        raise ValueError(f"{name} must contain exactly two coordinates")
    return (
        _finite_float(f"{name}.x", coordinates[0]),
        _finite_float(f"{name}.y", coordinates[1]),
    )


def _point_sequence(
    values: Iterable[Iterable[float]],
    name: str,
    *,
    minimum: int,
) -> tuple[Point2, ...]:
    points = tuple(_point(value, f"{name} point") for value in values)
    if len(points) < minimum:
        raise ValueError(f"{name} requires at least {minimum} points")
    if len(points) > 101:
        raise ValueError(f"{name} supports at most 101 points per V7 element")
    return points


def _finite_float(name: str, value: Any) -> float:
    if isinstance(value, bool):
        raise TypeError(f"{name} must be a real number")
    try:
        result = float(value)
    except (TypeError, ValueError) as error:
        raise TypeError(f"{name} must be a real number") from error
    if not math.isfinite(result):
        raise ValueError(f"{name} must be finite")
    return result


def _positive_float(name: str, value: Any) -> float:
    result = _finite_float(name, value)
    if result <= 0.0:
        raise ValueError(f"{name} must be positive")
    return result


def _bounded_int(name: str, value: Any, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{name} must be an int")
    if not minimum <= value <= maximum:
        raise ValueError(f"{name} must be between {minimum} and {maximum}")
    return value


__all__ = [
    "DgnAttributes",
    "Modelspace",
    "V7Document",
    "V7WriteEntity",
    "new",
]
