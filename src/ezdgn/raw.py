"""Low-level, lossless V7 DGN record inspection."""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal, TypeAlias, cast

from ._core import (
    DEFAULT_MAX_FILE_SIZE_BYTES,
    DEFAULT_MAX_RECORDS,
    MAX_V7_RECORD_SIZE_BYTES,
    DgnLimitError,
)
from ._core import detect_format_bytes as _detect_format_bytes
from ._core import scan_v7_records as _scan_v7_records

PathSource: TypeAlias = str | os.PathLike[str]
BytesSource: TypeAlias = bytes | bytearray | memoryview
DgnSource: TypeAlias = PathSource | BytesSource
FormatKind: TypeAlias = Literal["V7", "V8_CFB"]
TerminationKind: TypeAlias = Literal["end_marker", "physical_eof"]
CoreFormatRow: TypeAlias = tuple[str, int | None]
CoreRecordRow: TypeAlias = tuple[
    int, int, int, int, bool, bool, bool, int, int
]
CoreScanRow: TypeAlias = tuple[
    CoreFormatRow,
    list[CoreRecordRow],
    str,
    int,
    int,
    int,
]


@dataclass(frozen=True, slots=True)
class DgnFormatInfo:
    """Format information established only from the leading signature."""

    kind: FormatKind
    dimension: Literal[2, 3] | None

    @property
    def is_v7(self) -> bool:
        return self.kind == "V7"

    @property
    def is_v8_candidate(self) -> bool:
        """Whether this is a CFB container that may contain V8 DGN streams."""

        return self.kind == "V8_CFB"


@dataclass(frozen=True, slots=True)
class RawElement:
    """One bounded V7 record with its original bytes intact."""

    index: int
    offset: int
    level: int
    element_type: int
    complex_component: bool
    reserved: bool
    deleted: bool
    words_to_follow: int
    _raw_view: memoryview = field(repr=False)

    @property
    def size_bytes(self) -> int:
        return len(self._raw_view)

    @property
    def raw_view(self) -> memoryview:
        """Read-only zero-copy view of the complete record."""

        return self._raw_view

    @property
    def raw_bytes(self) -> bytes:
        """Copy the complete record into a standalone ``bytes`` value."""

        return self._raw_view.tobytes()

    @property
    def payload_view(self) -> memoryview:
        """Read-only zero-copy view after the four-byte record header."""

        return self._raw_view[4:]

    @property
    def payload(self) -> bytes:
        return self.payload_view.tobytes()


@dataclass(frozen=True, slots=True)
class RawScan:
    """Complete result of a bounded V7 record scan."""

    format: DgnFormatInfo
    records: tuple[RawElement, ...]
    termination: TerminationKind
    end_offset: int
    trailing_bytes: int
    source_size: int

    @property
    def eof_marker_offset(self) -> int | None:
        if self.termination == "end_marker":
            return self.end_offset
        return None


def detect_format(source: DgnSource) -> DgnFormatInfo:
    """Identify V7 dimensionality or a V8/CFB candidate.

    A CFB signature alone is not enough to prove that a file contains DGN V8
    streams, so V8 is deliberately reported as ``V8_CFB``.
    """

    signature = _read_signature(source)
    kind, dimension = _detect_format_bytes(signature)
    return DgnFormatInfo(
        kind=cast(FormatKind, kind),
        dimension=cast(Literal[2, 3] | None, dimension),
    )


def scan_records(
    source: DgnSource,
    *,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
    max_records: int = DEFAULT_MAX_RECORDS,
    max_record_size: int = MAX_V7_RECORD_SIZE_BYTES,
) -> RawScan:
    """Scan a V7 stream without decoding element-specific payloads."""

    _validate_limit("max_file_size", max_file_size)
    _validate_limit("max_records", max_records)
    _validate_limit("max_record_size", max_record_size)
    data = _read_all(source, max_file_size=max_file_size)
    row = _scan_v7_records(data, max_file_size, max_records, max_record_size)
    return _raw_scan_from_core(data, row)


def _raw_scan_from_core(data: bytes, row: CoreScanRow) -> RawScan:
    """Build the public raw model from one native scan result."""

    (
        (kind, dimension),
        record_rows,
        termination,
        end_offset,
        trailing_bytes,
        source_size,
    ) = row

    source_view = memoryview(data)
    records = tuple(
        RawElement(
            index=index,
            offset=offset,
            level=level,
            element_type=element_type,
            complex_component=complex_component,
            reserved=reserved,
            deleted=deleted,
            words_to_follow=words_to_follow,
            _raw_view=source_view[offset : offset + size_bytes],
        )
        for (
            index,
            offset,
            level,
            element_type,
            complex_component,
            reserved,
            deleted,
            words_to_follow,
            size_bytes,
        ) in record_rows
    )
    return RawScan(
        format=DgnFormatInfo(
            kind=cast(FormatKind, kind),
            dimension=cast(Literal[2, 3] | None, dimension),
        ),
        records=records,
        termination=cast(TerminationKind, termination),
        end_offset=end_offset,
        trailing_bytes=trailing_bytes,
        source_size=source_size,
    )


def _read_signature(source: DgnSource) -> bytes:
    if isinstance(source, (str, os.PathLike)):
        with Path(source).open("rb") as stream:
            return stream.read(8)
    return bytes(memoryview(source)[:8])


def _read_all(source: DgnSource, *, max_file_size: int) -> bytes:
    if isinstance(source, (str, os.PathLike)):
        path = Path(source)
        size = path.stat().st_size
        if size > max_file_size:
            raise DgnLimitError(
                f"input size {size} bytes exceeds configured limit "
                f"{max_file_size} bytes"
            )
        data = path.read_bytes()
    else:
        data = bytes(source)
    return data


def _validate_limit(name: str, value: int) -> None:
    if not isinstance(value, int):
        raise TypeError(f"{name} must be an int")
    if value < 0:
        raise ValueError(f"{name} must be non-negative")
