"""Native V7 DGN reader and seed-based writer for Python."""

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
    V8CfbEntry,
    V8ContainerInfo,
    inspect_v8_container,
)

try:
    __version__ = version("ezdgn")
except PackageNotFoundError:  # pragma: no cover - source tree without install
    __version__ = _core.core_version()


def main(argv: Sequence[str] | None = None) -> int:
    """Run the ezdgn command-line interface."""

    from .__main__ import main as cli_main

    return cli_main(argv)


__all__ = [
    "DEFAULT_MAX_FILE_SIZE_BYTES",
    "DEFAULT_MAX_CFB_ENTRIES",
    "DEFAULT_MAX_RECORDS",
    "MAX_V7_RECORD_SIZE_BYTES",
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
    "V8ContainerInfo",
    "__version__",
    "detect_format",
    "inspect_headers",
    "inspect_v8_container",
    "main",
    "new",
    "plot",
    "read",
    "readfile",
    "read_design_settings",
    "scan_records",
    "save_plot",
]
