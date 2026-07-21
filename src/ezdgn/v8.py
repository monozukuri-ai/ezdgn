"""Structural inspection and support policy helpers for DGN V8 containers."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal, TypeAlias, cast

from ._core import DEFAULT_MAX_CFB_ENTRIES, DgnLimitError
from ._core import inspect_v8_cfb as _inspect_v8_cfb
from .raw import (
    DEFAULT_MAX_FILE_SIZE_BYTES,
    DgnSource,
    _read_all,
    _validate_limit,
)

V8CfbEntryKind: TypeAlias = Literal["storage", "stream"]
CoreV8CfbEntryRow: TypeAlias = tuple[str, str, int | None]
CoreV8ContainerRow: TypeAlias = tuple[
    int,
    bool,
    list[str],
    list[str],
    list[CoreV8CfbEntryRow],
]


@dataclass(frozen=True, slots=True)
class V8CfbEntry:
    """One non-root storage or stream in a V8 candidate's CFB directory."""

    path: str
    kind: V8CfbEntryKind
    size_bytes: int | None


@dataclass(frozen=True, slots=True)
class V8ContainerInfo:
    """Bounded CFB metadata without decoding proprietary DGN V8 streams.

    ``has_dgn_v8_markers`` checks only the expected ``/Dgn~H``, ``/Dgn~S``,
    and ``/Dgn-Md`` entries. It is not semantic validation of those streams.
    """

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


def inspect_v8_container(
    source: DgnSource,
    *,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
    max_entries: int = DEFAULT_MAX_CFB_ENTRIES,
) -> V8ContainerInfo:
    """Inspect DGN-specific markers and directory entries in a V8 CFB candidate.

    This function intentionally stops at the public CFB container boundary. It
    neither reads V8 entities nor makes a round-trip fidelity claim.
    """

    _validate_limit("max_file_size", max_file_size)
    _validate_limit("max_entries", max_entries)
    data = _read_all(source, max_file_size=max_file_size)
    if len(data) > max_file_size:
        raise DgnLimitError(
            f"input size {len(data)} bytes exceeds configured limit "
            f"{max_file_size} bytes"
        )
    row: CoreV8ContainerRow = _inspect_v8_cfb(data, max_entries)
    (
        cfb_version,
        has_dgn_v8_markers,
        missing_markers,
        model_storage_paths,
        entries,
    ) = row
    return V8ContainerInfo(
        cfb_version=cast(Literal[3, 4], cfb_version),
        has_dgn_v8_markers=has_dgn_v8_markers,
        missing_markers=tuple(missing_markers),
        model_storage_paths=tuple(model_storage_paths),
        entries=tuple(
            V8CfbEntry(
                path=path,
                kind=cast(V8CfbEntryKind, kind),
                size_bytes=size_bytes,
            )
            for path, kind, size_bytes in entries
        ),
    )
