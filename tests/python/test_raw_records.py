from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

import ezdgn

DATA = Path(__file__).parents[1] / "data" / "dgn"
SMALLTEST = DATA / "v7" / "smalltest.dgn"
SEED_2D = DATA / "v7" / "seed_2d.dgn"
SEED_3D = DATA / "v7" / "seed_3d.dgn"
KNOT_OOB = DATA / "malformed" / "knot_oob.dgn"
V8 = DATA / "v8" / "test_dgnv8.dgn"


def test_limits_are_exported_from_the_rust_core() -> None:
    assert ezdgn.DEFAULT_MAX_FILE_SIZE_BYTES == 1024**3
    assert ezdgn.DEFAULT_MAX_RECORDS == 1_000_000
    assert ezdgn.MAX_V7_RECORD_SIZE_BYTES == 131_074
    assert (
        ezdgn.DEFAULT_MAX_FILE_SIZE_BYTES
        == ezdgn._core.DEFAULT_MAX_FILE_SIZE_BYTES
    )


def test_detects_v7_dimensions_and_v8_candidate() -> None:
    assert ezdgn.detect_format(SMALLTEST) == ezdgn.DgnFormatInfo("V7", 2)
    assert ezdgn.detect_format(SEED_3D) == ezdgn.DgnFormatInfo("V7", 3)
    assert ezdgn.detect_format(V8) == ezdgn.DgnFormatInfo("V8_CFB", None)
    assert ezdgn.detect_format(V8).is_v8_candidate


def test_inspects_v8_cfb_markers_without_claiming_semantic_support() -> None:
    info = ezdgn.inspect_v8_container(V8)
    assert info.cfb_version == 3
    assert info.has_dgn_v8_markers
    assert info.missing_markers == ()
    assert info.model_storage_paths == ("/Dgn-Md/#000000",)
    assert len(info.entries) == 24
    assert info.storage_count == 9
    assert info.stream_count == 15
    assert ezdgn.V8CfbEntry("/Dgn~H", "stream", 68) in info.entries

    assert ezdgn.inspect_v8_container(V8.read_bytes()) == info


def test_bounds_and_validates_v8_container_inspection() -> None:
    with pytest.raises(ezdgn.DgnLimitError, match="CFB entry count"):
        ezdgn.inspect_v8_container(V8, max_entries=0)
    with pytest.raises(ezdgn.DgnLimitError, match="input size"):
        ezdgn.inspect_v8_container(V8.read_bytes(), max_file_size=100)
    with pytest.raises(ezdgn.InvalidDgnError, match="invalid V8/CFB container"):
        ezdgn.inspect_v8_container(V8.read_bytes()[:512])
    with pytest.raises(ezdgn.UnsupportedDgnError, match="expected a V8/CFB"):
        ezdgn.inspect_v8_container(SMALLTEST)


def test_scans_smalltest_without_interpreting_marker_padding() -> None:
    source = SMALLTEST.read_bytes()
    scan = ezdgn.scan_records(source)
    assert scan.format == ezdgn.DgnFormatInfo("V7", 2)
    assert len(scan.records) == 15
    assert scan.termination == "end_marker"
    assert scan.eof_marker_offset == 10_424
    assert scan.trailing_bytes == 326
    assert scan.source_size == 10_752
    assert [record.element_type for record in scan.records[-4:]] == [17, 15, 6, 3]

    text = scan.records[-4]
    assert text.offset == 10_136
    assert text.level == 1
    assert text.size_bytes == 70
    assert text.raw_bytes == source[text.offset : text.offset + text.size_bytes]
    assert text.raw_view.readonly
    assert text.raw_view.obj is source
    assert len(text.payload) == 66
    assert len(text.payload_view) == 66


def test_path_and_bytes_scans_are_equivalent() -> None:
    from_path = ezdgn.scan_records(SEED_2D)
    from_bytes = ezdgn.scan_records(SEED_2D.read_bytes())
    assert from_path == from_bytes
    assert len(from_path.records) == 12
    assert from_path.eof_marker_offset == 9130


def test_accepts_physical_eof_on_record_boundary() -> None:
    scan = ezdgn.scan_records(SEED_3D)
    assert scan.format.dimension == 3
    assert len(scan.records) == 3
    assert scan.termination == "physical_eof"
    assert scan.end_offset == 2048
    assert scan.eof_marker_offset is None


def test_semantically_malformed_record_remains_bounded_raw_data() -> None:
    scan = ezdgn.scan_records(KNOT_OOB)
    assert len(scan.records) == 2
    assert scan.records[1].element_type == 26
    assert scan.records[1].size_bytes == 40


def test_rejects_v8_scan_with_specific_exception() -> None:
    with pytest.raises(
        ezdgn.UnsupportedDgnError,
        match="V8/CFB candidate.*external converter",
    ):
        ezdgn.scan_records(V8)


def test_rejects_truncated_record_and_limits() -> None:
    with pytest.raises(ezdgn.InvalidDgnError, match="declares 1536 bytes"):
        ezdgn.scan_records(bytes.fromhex("08 09 fe 02"))
    with pytest.raises(ezdgn.DgnLimitError, match="record count"):
        ezdgn.scan_records(SMALLTEST, max_records=1)
    with pytest.raises(ezdgn.DgnLimitError, match="input size"):
        ezdgn.scan_records(SMALLTEST, max_file_size=100)
    with pytest.raises(ValueError, match="non-negative"):
        ezdgn.scan_records(SMALLTEST, max_records=-1)


def test_exception_hierarchy() -> None:
    assert issubclass(ezdgn.InvalidDgnError, ezdgn.DgnError)
    assert issubclass(ezdgn.UnsupportedDgnError, ezdgn.DgnError)
    assert issubclass(ezdgn.DgnLimitError, ezdgn.DgnError)


def test_cli_inspect_json() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "ezdgn", "inspect", str(SMALLTEST), "--json"],
        check=True,
        capture_output=True,
        text=True,
    )
    payload = json.loads(result.stdout)
    assert payload["format"] == "V7"
    assert payload["dimension"] == 2
    assert payload["record_scan_supported"] is True
    assert payload["record_count"] == 15
    assert payload["termination"] == "end_marker"


def test_cli_identifies_v8_without_claiming_reader_support() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "ezdgn", "inspect", str(V8), "--json"],
        check=True,
        capture_output=True,
        text=True,
    )
    payload = json.loads(result.stdout)
    assert payload["path"] == str(V8)
    assert payload["format"] == "V8_CFB"
    assert payload["dimension"] is None
    assert payload["record_scan_supported"] is False
    assert payload["v8_read_policy"] == "external_conversion_required"
    assert payload["v8_container"] == {
        "cfb_version": 3,
        "has_dgn_v8_markers": True,
        "missing_markers": [],
        "model_storage_paths": ["/Dgn-Md/#000000"],
        "entry_count": 24,
        "storage_count": 9,
        "stream_count": 15,
    }
