from typing import Final, TypeAlias

DEFAULT_MAX_FILE_SIZE_BYTES: Final[int]
DEFAULT_MAX_RECORDS: Final[int]
MAX_V7_RECORD_SIZE_BYTES: Final[int]

class DgnError(Exception): ...
class InvalidDgnError(DgnError): ...
class UnsupportedDgnError(DgnError): ...
class DgnLimitError(DgnError): ...

FormatRow: TypeAlias = tuple[str, int | None]
RecordRow: TypeAlias = tuple[int, int, int, int, bool, bool, bool, int, int]
ScanRow: TypeAlias = tuple[FormatRow, list[RecordRow], str, int, int, int]
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
    ScanRow,
    SettingsRow,
    list[CommonHeaderRow | None],
]
PointI32Row: TypeAlias = tuple[int, int]
PointF64Row: TypeAlias = tuple[float, float]
LineRow: TypeAlias = tuple[
    int,
    PointI32Row,
    PointI32Row,
    tuple[PointF64Row, PointF64Row],
    tuple[PointF64Row, PointF64Row] | None,
]
MultiPointRow: TypeAlias = tuple[
    int,
    list[PointI32Row],
    list[PointF64Row],
    list[PointF64Row] | None,
]
EllipseRow: TypeAlias = tuple[
    int,
    PointF64Row,
    PointF64Row | None,
    tuple[float, float],
    tuple[float, float] | None,
    tuple[int, float],
]
ArcRow: TypeAlias = tuple[
    int,
    PointF64Row,
    PointF64Row | None,
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
    PointI32Row,
    PointF64Row | None,
    tuple[int, int, int],
]
ColorTableRow: TypeAlias = tuple[int, int, list[tuple[int, int, int]]]
CellRow: TypeAlias = tuple[
    int,
    tuple[int, tuple[int, int], str],
    tuple[int, tuple[int, int, int, int]],
    tuple[PointI32Row, PointI32Row],
    tuple[PointF64Row, PointF64Row] | None,
    tuple[tuple[int, int], tuple[int, int]],
    tuple[tuple[float, float], tuple[float, float]],
    PointI32Row,
    PointF64Row | None,
]
TextNodeRow: TypeAlias = tuple[
    int,
    tuple[int, int, int],
    tuple[int, int, int, int],
    tuple[int, float | None],
    tuple[int, int],
    tuple[float, float] | None,
    tuple[int, float],
    PointI32Row,
    PointF64Row | None,
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
    list[PointI32Row],
    list[PointF64Row],
    list[PointF64Row],
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
WriteStyleRow: TypeAlias = tuple[int, int, int, int, int, int]
WriteEntityRow: TypeAlias = tuple[
    str,
    list[PointF64Row],
    list[float],
    bytes,
    tuple[int, int],
    WriteStyleRow,
    int | None,
]
V8CfbEntryRow: TypeAlias = tuple[str, str, int | None]
V8ContainerRow: TypeAlias = tuple[
    int,
    bool,
    list[str],
    list[str],
    list[V8CfbEntryRow],
]

DEFAULT_MAX_CFB_ENTRIES: int

def core_version() -> str: ...
def detect_format_bytes(data: bytes) -> FormatRow: ...
def inspect_v8_cfb(data: bytes, max_entries: int) -> V8ContainerRow: ...
def scan_v7_records(
    data: bytes,
    max_file_size: int,
    max_records: int,
    max_record_size: int,
) -> ScanRow: ...
def read_v7_design_settings(
    data: bytes,
    max_file_size: int,
    max_records: int,
    max_record_size: int,
) -> SettingsRow: ...
def inspect_v7_headers(
    data: bytes,
    max_file_size: int,
    max_records: int,
    max_record_size: int,
) -> HeaderScanRow: ...
def read_v7_2d_primitives(
    data: bytes,
    max_file_size: int,
    max_records: int,
    max_record_size: int,
) -> PrimitiveScanRow: ...
def write_v7_2d_bytes(
    seed: bytes,
    entities: list[WriteEntityRow],
    copy_color_table: bool,
    copy_seed_elements: bool,
) -> bytes: ...
