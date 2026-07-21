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


def test_reads_real_tcb_settings() -> None:
    smalltest = ezdgn.read_design_settings(SMALLTEST)
    assert smalltest.dimension == 2
    assert smalltest.subunits_per_master == 10
    assert smalltest.uor_per_subunit == 1000
    assert smalltest.uor_per_master == 10_000
    assert smalltest.scale == 0.0001
    assert smalltest.master_unit_label == b"mu"
    assert smalltest.master_unit_name == "mu"
    assert smalltest.sub_unit_label == b"su"
    assert smalltest.sub_unit_name == "su"
    assert smalltest.global_origin_uor == (0.0, 0.0, 0.0)
    assert smalltest.global_origin_master == (0.0, 0.0, 0.0)

    seed_2d = ezdgn.read_design_settings(SEED_2D)
    assert seed_2d.uor_per_master == 120
    assert seed_2d.master_unit_name == "ft"
    assert seed_2d.sub_unit_name == "tf"
    assert seed_2d.global_origin_uor == (
        -249_879_416.0,
        -669_487_710.0,
        0.0,
    )
    assert seed_2d.global_origin_master == pytest.approx(
        (-2_082_328.4666666666, -5_579_064.25, 0.0)
    )

    seed_3d = ezdgn.read_design_settings(SEED_3D)
    assert seed_3d.dimension == 3
    assert seed_3d.uor_per_master == 1000
    assert seed_3d.master_unit_label == b"m\x00"
    assert seed_3d.master_unit_name == "m"
    assert seed_3d.sub_unit_name == "mm"


def test_design_settings_transform_validates_dimension_and_zero_scale() -> None:
    settings = ezdgn.read_design_settings(SMALLTEST)
    assert settings.to_master((7365, 37_198)) == pytest.approx(
        (0.7365, 3.7198)
    )
    with pytest.raises(ValueError, match="expected 2 coordinates"):
        settings.to_master((1, 2, 3))

    zero_scale = ezdgn.read_design_settings(KNOT_OOB)
    assert zero_scale.scale is None
    assert zero_scale.global_origin_master is None
    with pytest.raises(ValueError, match="zero UOR scale"):
        zero_scale.to_master((1, 2))


def test_inspects_common_headers_and_preserves_raw_records() -> None:
    scan = ezdgn.inspect_headers(SMALLTEST)
    assert scan.raw_scan.format == ezdgn.DgnFormatInfo("V7", 2)
    assert scan.design_settings == ezdgn.read_design_settings(SMALLTEST)
    assert len(scan.elements) == 15
    assert sum(element.common_header is not None for element in scan.elements) == 12
    assert scan.elements[0].common_header is None
    assert scan.elements[2].common_header is None

    text = scan.elements[11]
    assert text.record.element_type == 17
    assert text.common_header is not None
    assert text.common_header.range.low_uor == (7365, 37_198)
    assert text.common_header.range.high_uor == (94_083, 57_198)
    assert text.common_header.range.low_master == pytest.approx((0.7365, 3.7198))
    assert text.common_header.range.high_master == pytest.approx((9.4083, 5.7198))
    assert text.common_header.properties.is_new
    assert text.common_header.properties.is_planar
    assert text.common_header.properties.is_snappable
    assert text.common_header.symbology.color == 0

    ellipse = scan.elements[12].common_header
    assert ellipse is not None
    assert ellipse.range.low_master == pytest.approx((0.3285, -0.0961))
    assert ellipse.range.high_master == pytest.approx((9.6878, 9.2631))


def test_attribute_offsets_produce_read_only_zero_copy_views() -> None:
    shape = ezdgn.inspect_headers(SMALLTEST).elements[13]
    header = shape.common_header
    assert header is not None
    assert header.properties.raw == 0x0E00
    assert header.properties.modified
    assert header.properties.has_attributes
    assert header.symbology.raw == 0x5300
    assert header.symbology.color == 83
    assert header.attribute_offset == 78
    assert header.attribute_length == 16
    assert shape.attribute_view is not None
    assert shape.attribute_view.readonly
    assert shape.attribute_view.tobytes() == shape.record.raw_bytes[78:94]


def test_three_dimensional_common_range_keeps_z_component() -> None:
    scan = ezdgn.inspect_headers(SEED_3D)
    assert scan.design_settings.dimension == 3
    digitizer = scan.elements[1].common_header
    assert digitizer is not None
    assert len(digitizer.range.low_uor) == 3
    assert len(digitizer.range.high_uor) == 3
    assert len(digitizer.range.low_master or ()) == 3


def test_phase_two_rejects_v8_and_header_inconsistencies() -> None:
    with pytest.raises(ezdgn.UnsupportedDgnError, match="V8/CFB candidate"):
        ezdgn.read_design_settings(V8)
    with pytest.raises(ezdgn.UnsupportedDgnError, match="V8/CFB candidate"):
        ezdgn.inspect_headers(V8)

    dimension_mismatch = bytearray(SMALLTEST.read_bytes())
    dimension_mismatch[1214] |= 0x40
    with pytest.raises(ezdgn.InvalidDgnError, match="dimension mismatch"):
        ezdgn.inspect_headers(dimension_mismatch)

    invalid_attribute = bytearray(SMALLTEST.read_bytes())
    invalid_attribute[10_278 + 30 : 10_278 + 32] = b"\xff\xff"
    assert ezdgn.read_design_settings(invalid_attribute).uor_per_master == 10_000
    with pytest.raises(ezdgn.InvalidDgnError, match="attribute offset 131102"):
        ezdgn.inspect_headers(invalid_attribute)


def test_phase_two_apis_apply_resource_limits() -> None:
    with pytest.raises(ezdgn.DgnLimitError, match="record count"):
        ezdgn.read_design_settings(SMALLTEST, max_records=1)
    with pytest.raises(ezdgn.DgnLimitError, match="input size"):
        ezdgn.inspect_headers(SMALLTEST, max_file_size=100)


def test_cli_emits_design_settings_and_common_headers() -> None:
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "ezdgn",
            "inspect",
            str(SMALLTEST),
            "--headers",
            "--json",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    payload = json.loads(result.stdout)
    assert payload["design_settings"]["uor_per_master"] == 10_000
    assert payload["design_settings"]["master_unit"] == "mu"
    assert len(payload["records"]) == 15
    shape = payload["records"][13]["common_header"]
    assert shape["attribute_offset"] == 78
    assert shape["attribute_length"] == 16
    assert shape["symbology"]["color"] == 83
    assert shape["range"]["low_master"] == pytest.approx([4.3832, 2.5235])
