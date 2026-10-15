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


def test_level_table_entries_are_decoded() -> None:
    document = ezdgn.read_v8(V8)
    levels = document.levels
    assert [
        (item.level_id, item.number, item.name, item.line_style, item.line_weight, item.color_index)
        for item in levels
    ] == [(64, 0, "Default", 0, 0, 0)]
    entry = levels[0]
    assert entry.raw.element_type == ezdgn.v8.LEVEL_TABLE_ELEMENT_TYPE
    assert entry.raw.level == ezdgn.v8.LEVEL_TABLE_KIND
    assert entry.raw.element_id == 33
    assert document.level(64) == entry
    assert document.level(65) is None
    # the graphical elements of the fixture sit on that level
    assert {element.level for element in document.entities} <= {64, 0, 1}
    assert ezdgn.v8.level_table(document.raw) == levels


def test_level_names_decode_the_observed_linkage_encodings() -> None:
    from ezdgn.v8 import _name_linkage

    # UTF-16LE after ff fd (the checked-in fixture, written by the ODA toolkit)
    utf16 = bytes.fromhex(
        "0f10d2560100000010000000fffd44006500660061007500" "6c0074000000000000000000"
    )
    assert _name_linkage(utf16) == "Default"
    # 8-bit text after the ff fe 01 00 code-page marker (MicroStation-native file)
    marked = bytes.fromhex("0f10d256010000000d000000fffe01005761746572" "6d61696e00000000000000")
    assert _name_linkage(marked) == "Watermain"
    # plain 8-bit text (a file converted from DWG)
    plain = bytes.fromhex("0b10d256010000000600000040455452415300000000000000".replace("40", "4c", 1))
    assert _name_linkage(plain) == "LETRAS"
    # another linkage first, then the name; an entry without a name linkage is ""
    assert _name_linkage(bytes.fromhex("0110abcd00000000") + plain) == "LETRAS"
    assert _name_linkage(bytes.fromhex("0110abcd00000000")) == ""
    assert _name_linkage(b"") == ""


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


def test_mesh_rows_become_faces_with_edge_visibility() -> None:
    from ezdgn.v8 import _element_data_from_core

    point = ((10.0, 0.0, 0.0), (0.001, 0.0, 0.0))
    data = _element_data_from_core(
        {
            "kind": "MESH",
            "child_count": 4,
            "style": 1,
            "vertices": [point, point, point],
            "faces": [([0, 1, 2], [True, True, False])],
        }
    )

    assert data.kind == "MESH"
    assert data.style == 1
    assert len(data.vertices) == 3
    assert data.faces == (ezdgn.V8MeshFace((0, 1, 2), (True, True, False)),)
