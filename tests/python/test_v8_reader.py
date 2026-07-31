from __future__ import annotations

import json
import subprocess
import sys
from collections import Counter
from pathlib import Path

import pytest

import ezdgn


DATA = Path(__file__).parents[1] / "data" / "dgn"
V8 = DATA / "v8" / "test_dgnv8.dgn"
V7 = DATA / "v7" / "smalltest.dgn"


def test_scans_lossless_v8_model_pages_and_objects() -> None:
    raw = ezdgn.scan_v8_objects(V8)

    assert raw.container.has_dgn_v8_markers
    assert raw.total_inflated_bytes == 31_942
    assert raw.graphical_object_count == 58
    assert raw.total_object_count == 83
    assert len(raw.models) == 1
    model = raw.models[0]
    assert model.index.name == "my_model"
    assert model.index.description == "my_description"
    assert len(model.graphical_pages) == 1
    assert len(model.graphical_objects) == 58

    first = model.graphical_objects[0]
    assert first.element_type == 3
    assert first.role == "standalone"
    assert first.level == 1
    assert first.element_id == 36
    assert first.size_bytes == 152
    assert first.raw_bytes[:12] == bytes.fromhex("030000104c0000004c000000")
    assert first.primary_bytes == first.raw_bytes
    assert first.attribute_bytes == b""


def test_reads_v8_metadata_geometry_text_hierarchy_and_unknowns() -> None:
    document = ezdgn.read_v8(V8)
    assert isinstance(ezdgn.open_document(V8), ezdgn.V8Document)
    assert isinstance(ezdgn.openfile(V7), ezdgn.Drawing)
    assert len(document.models) == 1

    model = document.models[0]
    metadata = model.metadata
    assert metadata.name == "my_model"
    assert metadata.description == "my_description"
    assert metadata.dimension == 3
    assert metadata.uor_per_master == 10_000
    assert metadata.master_unit == "m"
    assert metadata.sub_unit == "mm"
    assert metadata.extents_uor == ((-19_577, -24_358, 0), (60_000, 70_000, 80_000))
    assert len(model.elements) == 58
    assert len(model.entities) == 34
    assert Counter(element.raw.element_type for element in model.entities) == Counter(
        {
            2: 2,
            3: 4,
            4: 2,
            6: 2,
            11: 2,
            12: 3,
            14: 3,
            15: 3,
            16: 3,
            17: 4,
            22: 2,
            27: 2,
            35: 1,
            36: 1,
        }
    )

    line = model.elements[0]
    assert line.kind == "LINE"
    assert line.data.vertices[0].uor == (0.0, 10_000.0, 20_000.0)
    assert line.data.vertices[0].master == (0.0, 1.0, 2.0)
    assert line.common.color_index == 3
    assert line.common.line_style == 4
    assert line.raw is document.raw.models[0].graphical_objects[0]

    text = model.elements[4]
    assert text.kind == "TEXT"
    assert text.data.text == "myTéxt"
    assert text.data.encoding == "v8-escaped-windows-1252"
    assert text.data.text_bytes == b"\xff\xfe\x01\0myT\xe9xt"
    assert text.data.origin is not None
    assert text.data.origin.master == (0.0, 1.0, 0.0)
    assert text.data.width_multiplier_raw == pytest.approx(1_666_666.6666666665)
    assert text.data.height_multiplier_raw == pytest.approx(1_666_666.6666666665)
    assert text.data.width_uor == pytest.approx(10_000)
    assert text.data.height_uor == pytest.approx(10_000)
    assert text.data.width_master == pytest.approx(1)
    assert text.data.height_master == pytest.approx(1)

    header = model.elements[27]
    assert model.children(header) == (model.elements[28], model.elements[29])
    assert model.parent(model.elements[28]) is header
    assert tuple(model.descendants(header)) == (model.elements[28], model.elements[29])
    assert model.query("shared cell instance")[0].data.name == "Named definition"
    assert model.unknown_elements == (model.elements[57],)


def test_v8_path_and_bytes_results_are_equivalent() -> None:
    assert ezdgn.read_v8(V8.read_bytes()) == ezdgn.read_v8(V8)
    assert ezdgn.scan_v8_objects(V8.read_bytes()) == ezdgn.scan_v8_objects(V8)


def test_v8_limits_are_configurable_and_mapped_to_limit_errors() -> None:
    with pytest.raises(ezdgn.DgnLimitError, match="V8 page count"):
        ezdgn.scan_v8_objects(V8, limits=ezdgn.V8ScanLimits(max_pages=0))
    with pytest.raises(ezdgn.DgnLimitError, match="input size"):
        ezdgn.read_v8(V8, limits=ezdgn.V8ScanLimits(max_file_size=100))
    with pytest.raises(ValueError, match="non-negative"):
        ezdgn.read_v8(V8, limits=ezdgn.V8ScanLimits(max_vertices=-1))
    with pytest.raises(ezdgn.DgnLimitError, match="hierarchy exceeds"):
        ezdgn.read_v8(V8, limits=ezdgn.V8ScanLimits(max_hierarchy_depth=0))


def test_cli_v8_entities_include_native_text_and_hierarchy() -> None:
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "ezdgn",
            "inspect",
            str(V8),
            "--entities",
            "--json",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    payload = json.loads(result.stdout)
    records = payload["v8_models"][0]["records"]
    assert len(records) == 58
    assert records[0]["entity"]["kind"] == "LINE"
    assert records[4]["entity"]["text"] == "myTéxt"
    assert records[4]["entity"]["width_multiplier_raw"] == pytest.approx(
        1_666_666.6666666665
    )
    assert records[4]["entity"]["height_multiplier_raw"] == pytest.approx(
        1_666_666.6666666665
    )
    assert records[4]["entity"]["width_uor"] == pytest.approx(10_000)
    assert records[4]["entity"]["height_uor"] == pytest.approx(10_000)
    assert records[4]["entity"]["width_master"] == pytest.approx(1)
    assert records[4]["entity"]["height_master"] == pytest.approx(1)
    assert records[27]["child_indices"] == [28, 29]
    assert records[28]["parent_index"] == 27
