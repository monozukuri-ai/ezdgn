"""Native V8 DGN reader and V7 DGN reader/writer for Python."""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version
from typing import Sequence

from . import _core
from ._core import DgnError, DgnLimitError, InvalidDgnError, UnsupportedDgnError
from .entities import (
    Arc,
    AttributeLinkage,
    BSplineCurve,
    BSplineKnot,
    BSplinePole,
    BSplineSurface,
    BSplineSurfaceBoundary,
    BSplineWeight,
    BasicStyle,
    Cell,
    ColorTable,
    ComplexChain,
    ComplexElement,
    ComplexShape,
    Curve,
    DgnElement,
    Drawing,
    Ellipse,
    GraphicElement,
    Line,
    LineString,
    Shape,
    Text,
    TextNode,
    UnsupportedElement,
    read,
    readfile,
)
from .metadata import (
    CommonElementHeader,
    DesignSettings,
    ElementMetadata,
    ElementProperties,
    ElementRange,
    ElementSymbology,
    HeaderScan,
    inspect_headers,
    read_design_settings,
)
from .plotting import CoordinateSpace, plot, save_plot
from .raw import (
    DEFAULT_MAX_FILE_SIZE_BYTES,
    DEFAULT_MAX_RECORDS,
    MAX_V7_RECORD_SIZE_BYTES,
    DgnFormatInfo,
    RawElement,
    RawScan,
    detect_format,
    scan_records,
)
from .writer import DgnAttributes, Modelspace, V7Document, V7WriteEntity, new
from .v8 import (
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
    V8AuxiliaryPage,
    V8AuxiliaryRecord,
    V8CfbEntry,
    V8CommonHeader,
    V8ContainerInfo,
    V8Document,
    V8Element,
    V8ElementData,
    V8Linkage,
    V8Model,
    V8ModelIndexEntry,
    V8ModelMetadata,
    V8ObjectPage,
    V8PageHeader,
    V8Point,
    V8RawDocument,
    V8RawModel,
    V8RawObject,
    V8ScanLimits,
    inspect_v8_container,
    read_v8,
    read_v8file,
    scan_v8_objects,
)

try:
    __version__ = version("ezdgn")
except PackageNotFoundError:  # pragma: no cover - source tree without install
    __version__ = _core.core_version()


def open_document(
    source: object,
    *,
    max_file_size: int = DEFAULT_MAX_FILE_SIZE_BYTES,
    max_records: int = DEFAULT_MAX_RECORDS,
    max_record_size: int = MAX_V7_RECORD_SIZE_BYTES,
    v8_limits: V8ScanLimits | None = None,
) -> Drawing | V8Document:
    """Open V7 2D or V8 through one format-neutral read entry point."""

    info = detect_format(source)  # type: ignore[arg-type]
    if info.is_v7:
        return read(
            source,  # type: ignore[arg-type]
            max_file_size=max_file_size,
            max_records=max_records,
            max_record_size=max_record_size,
        )
    limits = v8_limits or V8ScanLimits(max_file_size=max_file_size)
    return read_v8(source, limits=limits)  # type: ignore[arg-type]


openfile = open_document


def main(argv: Sequence[str] | None = None) -> int:
    """Run the ezdgn command-line interface."""

    from .__main__ import main as cli_main

    return cli_main(argv)


__all__ = [
    "DEFAULT_MAX_FILE_SIZE_BYTES",
    "DEFAULT_MAX_CFB_ENTRIES",
    "DEFAULT_MAX_CFB_STREAM_SIZE_BYTES",
    "DEFAULT_MAX_CFB_TOTAL_STREAM_BYTES",
    "DEFAULT_MAX_RECORDS",
    "MAX_V7_RECORD_SIZE_BYTES",
    "DEFAULT_MAX_V8_HIERARCHY_DEPTH",
    "DEFAULT_MAX_V8_INFLATED_STREAM_BYTES",
    "DEFAULT_MAX_V8_MODELS",
    "DEFAULT_MAX_V8_OBJECT_SIZE_BYTES",
    "DEFAULT_MAX_V8_OBJECTS",
    "DEFAULT_MAX_V8_PAGES",
    "DEFAULT_MAX_V8_STRING_BYTES",
    "DEFAULT_MAX_V8_TOTAL_INFLATED_BYTES",
    "DEFAULT_MAX_V8_VERTICES",
    "DgnError",
    "DgnElement",
    "DgnAttributes",
    "DgnFormatInfo",
    "DgnLimitError",
    "Arc",
    "AttributeLinkage",
    "BSplineCurve",
    "BSplineKnot",
    "BSplinePole",
    "BSplineSurface",
    "BSplineSurfaceBoundary",
    "BSplineWeight",
    "BasicStyle",
    "Cell",
    "ColorTable",
    "ComplexChain",
    "ComplexElement",
    "ComplexShape",
    "CoordinateSpace",
    "CommonElementHeader",
    "DesignSettings",
    "Drawing",
    "Curve",
    "Ellipse",
    "ElementMetadata",
    "ElementProperties",
    "ElementRange",
    "ElementSymbology",
    "HeaderScan",
    "GraphicElement",
    "InvalidDgnError",
    "Line",
    "LineString",
    "Modelspace",
    "RawElement",
    "RawScan",
    "Shape",
    "Text",
    "TextNode",
    "UnsupportedDgnError",
    "UnsupportedElement",
    "V7Document",
    "V7WriteEntity",
    "V8CfbEntry",
    "V8AuxiliaryPage",
    "V8AuxiliaryRecord",
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
    "__version__",
    "detect_format",
    "inspect_headers",
    "inspect_v8_container",
    "main",
    "new",
    "open_document",
    "openfile",
    "plot",
    "read",
    "readfile",
    "read_v8",
    "read_v8file",
    "read_design_settings",
    "scan_records",
    "scan_v8_objects",
    "save_plot",
]
