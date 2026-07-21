"""V7 design settings and common element-header inspection."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal, Sequence, TypeAlias, cast

from ._core import (
    DEFAULT_MAX_FILE_SIZE_BYTES,
    DEFAULT_MAX_RECORDS,
    MAX_V7_RECORD_SIZE_BYTES,
)
from ._core import inspect_v7_headers as _inspect_v7_headers
from ._core import read_v7_design_settings as _read_v7_design_settings
from .raw import (
    CoreScanRow,
    DgnSource,
    RawElement,
    RawScan,
    _raw_scan_from_core,
    _read_all,
    _validate_limit,
)

Dimension: TypeAlias = Literal[2, 3]
RawCoordinate: TypeAlias = tuple[int, int] | tuple[int, int, int]
MasterCoordinate: TypeAlias = tuple[float, float] | tuple[float, float, float]
RawPointRow: TypeAlias = tuple[int, int, int | None]
MasterPointRow: TypeAlias = tuple[float, float, float | None]
SettingsRow: TypeAlias = tuple[
    int,
    int,
    int,
    tuple[int, int],
    tuple[int, int],
    tuple[float, float, float],
    int,
    float | None,
    tuple[float, float, float] | None,
]
PropertiesRow: TypeAlias = tuple[
    int,
    int,
    int,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
]
SymbologyRow: TypeAlias = tuple[int, int, int, int]
CommonHeaderRow: TypeAlias = tuple[
    RawPointRow,
    RawPointRow,
    MasterPointRow | None,
    MasterPointRow | None,
    int,
    int,
    PropertiesRow,
    SymbologyRow,
    int | None,
    int,
]
HeaderScanRow: TypeAlias = tuple[
    CoreScanRow,
    SettingsRow,
    list[CommonHeaderRow | None],
]


@dataclass(frozen=True, slots=True)
class DesignSettings:
    """Coordinate system and unit values from the leading type-9 TCB."""

    dimension: Dimension
    subunits_per_master: int
    uor_per_subunit: int
    master_unit_label: bytes
    sub_unit_label: bytes
    global_origin_uor: tuple[float, float, float]
    uor_per_master: int
    scale: float | None
    global_origin_master: tuple[float, float, float] | None

    @property
    def master_unit_name(self) -> str | None:
        return _ascii_unit_name(self.master_unit_label)

    @property
    def sub_unit_name(self) -> str | None:
        return _ascii_unit_name(self.sub_unit_label)

    def to_master(self, coordinates: Sequence[float]) -> MasterCoordinate:
        """Transform one raw UOR point into master units."""

        if len(coordinates) != self.dimension:
            raise ValueError(
                f"expected {self.dimension} coordinates, got {len(coordinates)}"
            )
        if self.scale is None or self.global_origin_master is None:
            raise ValueError("cannot transform coordinates with zero UOR scale")
        transformed = tuple(
            float(coordinate) * self.scale - self.global_origin_master[index]
            for index, coordinate in enumerate(coordinates)
        )
        return cast(MasterCoordinate, transformed)


@dataclass(frozen=True, slots=True)
class ElementRange:
    """Offset-binary range in raw UOR and transformed master units."""

    low_uor: RawCoordinate
    high_uor: RawCoordinate
    low_master: MasterCoordinate | None
    high_master: MasterCoordinate | None


@dataclass(frozen=True, slots=True)
class ElementProperties:
    """Decoded V7 property word with ambiguous H-bit preserved verbatim."""

    raw: int
    element_class: int
    reserved: int
    locked: bool
    is_new: bool
    modified: bool
    has_attributes: bool
    screen_relative: bool
    non_planar: bool
    not_snappable: bool
    h_bit: bool

    @property
    def is_planar(self) -> bool:
        return not self.non_planar

    @property
    def is_snappable(self) -> bool:
        return not self.not_snappable


@dataclass(frozen=True, slots=True)
class ElementSymbology:
    """Basic color, line-weight, and line-style indices."""

    raw: int
    style: int
    weight: int
    color: int


@dataclass(frozen=True, slots=True)
class CommonElementHeader:
    """The standard range and display header of a V7 element."""

    range: ElementRange
    graphic_group: int
    attribute_index: int
    properties: ElementProperties
    symbology: ElementSymbology
    attribute_offset: int | None
    attribute_length: int


@dataclass(frozen=True, slots=True)
class ElementMetadata:
    """Raw record paired with its optional standard common header."""

    record: RawElement
    common_header: CommonElementHeader | None

    @property
    def attribute_view(self) -> memoryview | None:
        """Zero-copy view of attribute data when the A-bit is set."""

        header = self.common_header
        if header is None or header.attribute_offset is None:
            return None
        start = header.attribute_offset
        return self.record.raw_view[start : start + header.attribute_length]


@dataclass(frozen=True, slots=True)
class HeaderScan:
    """Raw V7 scan enriched with TCB and common-header metadata."""

    raw_scan: RawScan
    design_settings: DesignSettings
    elements: tuple[ElementMetadata, ...]


def read_design_settings(
    source: DgnSource,
    *,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
    max_records: int = DEFAULT_MAX_RECORDS,
    max_record_size: int = MAX_V7_RECORD_SIZE_BYTES,
) -> DesignSettings:
    """Decode only the leading TCB after a bounded record scan."""

    data = _load_data(source, max_file_size, max_records, max_record_size)
    row = _read_v7_design_settings(
        data, max_file_size, max_records, max_record_size
    )
    return _settings_from_row(row)


def inspect_headers(
    source: DgnSource,
    *,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
    max_records: int = DEFAULT_MAX_RECORDS,
    max_record_size: int = MAX_V7_RECORD_SIZE_BYTES,
) -> HeaderScan:
    """Decode the leading TCB and standard headers without entity geometry."""

    data = _load_data(source, max_file_size, max_records, max_record_size)
    row = cast(
        HeaderScanRow,
        _inspect_v7_headers(data, max_file_size, max_records, max_record_size),
    )
    return _header_scan_from_core(data, row)


def _header_scan_from_core(data: bytes, row: HeaderScanRow) -> HeaderScan:
    """Build the shared public header model from one native result."""

    scan_row, settings_row, common_rows = row
    raw_scan = _raw_scan_from_core(data, scan_row)
    if len(raw_scan.records) != len(common_rows):
        raise RuntimeError("native header scan returned inconsistent record counts")
    elements = tuple(
        ElementMetadata(record=record, common_header=_common_header_from_row(row))
        for record, row in zip(raw_scan.records, common_rows)
    )
    return HeaderScan(
        raw_scan=raw_scan,
        design_settings=_settings_from_row(settings_row),
        elements=elements,
    )


def _load_data(
    source: DgnSource,
    max_file_size: int,
    max_records: int,
    max_record_size: int,
) -> bytes:
    _validate_limit("max_file_size", max_file_size)
    _validate_limit("max_records", max_records)
    _validate_limit("max_record_size", max_record_size)
    return _read_all(source, max_file_size=max_file_size)


def _settings_from_row(row: SettingsRow) -> DesignSettings:
    (
        dimension,
        subunits_per_master,
        uor_per_subunit,
        master_unit_label,
        sub_unit_label,
        global_origin_uor,
        uor_per_master,
        scale,
        global_origin_master,
    ) = row
    return DesignSettings(
        dimension=cast(Dimension, dimension),
        subunits_per_master=subunits_per_master,
        uor_per_subunit=uor_per_subunit,
        master_unit_label=bytes(master_unit_label),
        sub_unit_label=bytes(sub_unit_label),
        global_origin_uor=global_origin_uor,
        uor_per_master=uor_per_master,
        scale=scale,
        global_origin_master=global_origin_master,
    )


def _common_header_from_row(
    row: CommonHeaderRow | None,
) -> CommonElementHeader | None:
    if row is None:
        return None
    (
        low_uor,
        high_uor,
        low_master,
        high_master,
        graphic_group,
        attribute_index,
        properties,
        symbology,
        attribute_offset,
        attribute_length,
    ) = row
    (
        properties_raw,
        element_class,
        properties_reserved,
        locked,
        is_new,
        modified,
        has_attributes,
        screen_relative,
        non_planar,
        not_snappable,
        h_bit,
    ) = properties
    symbology_raw, style, weight, color = symbology
    return CommonElementHeader(
        range=ElementRange(
            low_uor=_raw_coordinate(low_uor),
            high_uor=_raw_coordinate(high_uor),
            low_master=(
                None if low_master is None else _master_coordinate(low_master)
            ),
            high_master=(
                None if high_master is None else _master_coordinate(high_master)
            ),
        ),
        graphic_group=graphic_group,
        attribute_index=attribute_index,
        properties=ElementProperties(
            raw=properties_raw,
            element_class=element_class,
            reserved=properties_reserved,
            locked=locked,
            is_new=is_new,
            modified=modified,
            has_attributes=has_attributes,
            screen_relative=screen_relative,
            non_planar=non_planar,
            not_snappable=not_snappable,
            h_bit=h_bit,
        ),
        symbology=ElementSymbology(
            raw=symbology_raw,
            style=style,
            weight=weight,
            color=color,
        ),
        attribute_offset=attribute_offset,
        attribute_length=attribute_length,
    )


def _raw_coordinate(row: RawPointRow) -> RawCoordinate:
    x, y, z = row
    if z is None:
        return (x, y)
    return (x, y, z)


def _master_coordinate(row: MasterPointRow) -> MasterCoordinate:
    x, y, z = row
    if z is None:
        return (x, y)
    return (x, y, z)


def _ascii_unit_name(label: bytes) -> str | None:
    try:
        return label.rstrip(b"\x00 ").decode("ascii")
    except UnicodeDecodeError:
        return None
