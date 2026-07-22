from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

import ezdgn

DATA = Path(__file__).parents[1] / "data" / "dgn"
SMALLTEST = DATA / "v7" / "smalltest.dgn"
SEED_3D = DATA / "v7" / "seed_3d.dgn"


def test_reads_smalltest_as_ordered_lossless_entities() -> None:
    source = SMALLTEST.read_bytes()
    drawing = ezdgn.read(source)

    assert drawing.design_settings.uor_per_master == 10_000
    assert len(drawing.elements) == 15
    assert len(drawing.unsupported_elements) == 11
    assert drawing.color_table is None
    assert [entity.kind for entity in drawing.entities] == [
        "TEXT",
        "ELLIPSE",
        "SHAPE",
        "LINE",
    ]
    assert [entity.dxftype() for entity in drawing] == [
        "TEXT",
        "ELLIPSE",
        "SHAPE",
        "LINE",
    ]
    assert drawing.elements[4].record.element_type == 5
    assert isinstance(drawing.elements[4], ezdgn.UnsupportedElement)
    assert all(
        element.record.raw_bytes
        == source[
            element.record.offset : element.record.offset
            + element.record.size_bytes
        ]
        for element in drawing.elements
    )

    text = drawing.query("text")[0]
    assert isinstance(text, ezdgn.Text)
    assert text.font_id == 3
    assert text.justification == 7
    assert text.length_multiplier_raw == 1_666_667
    assert text.height_multiplier_raw == 1_666_667
    assert text.length_multiplier_master == pytest.approx(1.000_000_2)
    assert text.height_multiplier_master == pytest.approx(1.000_000_2)
    assert text.origin_uor == (7_365, 42_198)
    assert text.origin_master == pytest.approx((0.7365, 4.2198))
    assert text.text_view.readonly
    assert text.text_bytes == b"Demo Text"
    assert text.level == 1
    assert text.level == text.record.level
    assert text.decode_text() == "Demo Text"
    assert text.decode_text("ascii") == "Demo Text"

    ellipse = drawing.query("ellipse")[0]
    assert isinstance(ellipse, ezdgn.Ellipse)
    assert ellipse.center_uor == pytest.approx((50_082.0, 45_835.0))
    assert ellipse.center_master == pytest.approx((5.0082, 4.5835))
    assert ellipse.primary_axis_uor == pytest.approx(46_796.065_838_914_28)
    assert ellipse.primary_axis_master == pytest.approx(4.679_606_583_891_428)
    assert ellipse.secondary_axis_uor == pytest.approx(
        ellipse.primary_axis_uor
    )
    assert ellipse.rotation_degrees == 0.0

    shape = drawing.query("shape")[0]
    assert isinstance(shape, ezdgn.Shape)
    assert shape.is_closed
    assert shape.has_repeated_closing_vertex
    assert shape.vertices_uor == (
        (45_355, 33_170),
        (43_832, 26_517),
        (49_441, 25_235),
        (48_320, 33_331),
        (45_355, 33_170),
    )
    assert shape.vertices_master is not None
    assert shape.vertices_master[2] == pytest.approx((4.9441, 2.5235))
    assert shape.style == ezdgn.BasicStyle(
        83,
        0,
        0,
        (180, 0, 0),
        fill_color_index=83,
        fill_rgb=(180, 0, 0),
    )
    assert shape.attribute_view is not None
    assert shape.attribute_view.tobytes() == source[10_278 + 78 : 10_278 + 94]
    assert len(shape.linkages) == 1
    assert shape.linkages[0].kind == "SHAPE_FILL"
    assert shape.linkages[0].linkage_type_name == "SHAPE_FILL"
    assert shape.linkages[0].color_index == 83
    assert shape.linkages[0].raw_bytes == shape.attribute_view.tobytes()

    line = drawing.query("line")[0]
    assert isinstance(line, ezdgn.Line)
    assert line.start_uor == (25_562, 57_218)
    assert line.end_uor == (25_242, 60_709)
    assert line.start_master == pytest.approx((2.5562, 5.7218))
    assert line.end_master == pytest.approx((2.5242, 6.0709))
    assert drawing.query("line-string") == ()


def test_uses_microstation_v7_colors_when_table_is_absent() -> None:
    drawing = ezdgn.readfile(SMALLTEST)

    assert drawing.color_table is None
    assert drawing.resolve_color(0) == (255, 255, 255)
    assert drawing.resolve_color(1) == (0, 0, 255)
    assert drawing.resolve_color(10) == (254, 0, 96)
    assert drawing.resolve_color(83) == (180, 0, 0)
    assert drawing.resolve_color(254) == (192, 192, 192)
    assert drawing.resolve_color(255) == (28, 0, 100)
    with pytest.raises(IndexError, match="between 0 and 255"):
        drawing.resolve_color(-1)
    with pytest.raises(IndexError, match="between 0 and 255"):
        drawing.resolve_color(256)

    line = drawing.query("LINE")[0]
    assert isinstance(line, ezdgn.Line)
    assert line.style is not None
    assert line.style.rgb == drawing.resolve_color(line.style.color_index)


def test_decodes_line_string_arc_color_table_and_resolves_style() -> None:
    line_string_body = bytearray(2 + 3 * 8)
    line_string_body[:2] = (3).to_bytes(2, "little")
    for index, (x, y) in enumerate(((10, 20), (-30, 40), (50, -60))):
        line_string_body[2 + index * 8 : 6 + index * 8] = _middle_i32(x)
        line_string_body[6 + index * 8 : 10 + index * 8] = _middle_i32(y)

    arc_body = bytearray(44)
    arc_body[0:4] = _middle_i32(45 * 360_000)
    arc_body[4:8] = _middle_u32(0x8000_0000 | 90 * 360_000)
    arc_body[8:16] = bytes.fromhex("80 40 00 00 00 00 00 00")  # 1.0
    arc_body[16:24] = bytes.fromhex("00 41 00 00 00 00 00 00")  # 2.0
    arc_body[24:28] = _middle_i32(30 * 360_000)
    arc_body[28:36] = bytes.fromhex("80 41 00 00 00 00 00 00")  # 4.0
    arc_body[36:44] = bytes.fromhex("80 c0 00 00 00 00 00 00")  # -1.0

    color_body = bytearray(770)
    color_body[0:2] = (1).to_bytes(2, "little")
    color_body[2:5] = bytes((7, 8, 9))  # color 255 is stored first
    color_body[5:8] = bytes((255, 255, 255))
    color_body[8:11] = bytes((10, 20, 30))

    drawing = ezdgn.read(
        _with_phase_three_records(
            _record(5, 1, color_body),
            _record(4, 2, line_string_body, color=1),
            _record(16, 2, arc_body, color=1),
        )
    )
    assert [element.kind for element in drawing.elements[-3:]] == [
        "COLOR_TABLE",
        "LINE_STRING",
        "ARC",
    ]

    table = drawing.color_table
    assert isinstance(table, ezdgn.ColorTable)
    assert table.screen_flag == 1
    assert len(table.colors) == 256
    assert table[0] == (255, 255, 255)
    assert table.color(1) == (10, 20, 30)
    assert table[255] == (7, 8, 9)
    assert drawing.resolve_color(1) == (10, 20, 30)
    with pytest.raises(IndexError, match="between 0 and 255"):
        table[-1]

    line_string = drawing.query("line string")[0]
    assert isinstance(line_string, ezdgn.LineString)
    assert line_string.vertices_uor == ((10, 20), (-30, 40), (50, -60))
    assert line_string.vertices_master is not None
    assert line_string.vertices_master[0] == pytest.approx((0.001, 0.002))
    assert line_string.vertices_master[1] == pytest.approx((-0.003, 0.004))
    assert line_string.vertices_master[2] == pytest.approx((0.005, -0.006))
    assert line_string.style == ezdgn.BasicStyle(1, 0, 0, (10, 20, 30))

    arc = drawing.query("arc")[0]
    assert isinstance(arc, ezdgn.Arc)
    assert arc.center_uor == (4.0, -1.0)
    assert arc.center_master == pytest.approx((0.0004, -0.0001))
    assert arc.primary_axis_uor == 1.0
    assert arc.secondary_axis_uor == 2.0
    assert arc.primary_axis_master == pytest.approx(0.0001)
    assert arc.secondary_axis_master == pytest.approx(0.0002)
    assert arc.rotation_degrees == 30.0
    assert arc.start_angle_degrees == 45.0
    assert arc.sweep_angle_raw == -90 * 360_000
    assert arc.sweep_angle_degrees == -90.0
    assert arc.style is not None
    assert arc.style.rgb == (10, 20, 30)


def test_high_level_reader_rejects_unsupported_dimensions_and_bad_entities() -> None:
    with pytest.raises(ezdgn.UnsupportedDgnError, match="3D geometry"):
        ezdgn.readfile(SEED_3D)
    with pytest.raises(ezdgn.DgnLimitError, match="record count"):
        ezdgn.readfile(SMALLTEST, max_records=1)
    with pytest.raises(TypeError, match="filesystem path"):
        ezdgn.readfile(SMALLTEST.read_bytes())  # type: ignore[arg-type]

    invalid_body = bytearray(2 + 8)
    invalid_body[:2] = (1).to_bytes(2, "little")
    with pytest.raises(ezdgn.InvalidDgnError, match="declares 1 vertices"):
        ezdgn.read(_with_phase_three_records(_record(4, 2, invalid_body)))


def test_last_color_table_is_active_for_all_entity_styles() -> None:
    first = _color_table_body((1, 2, 3))
    last = _color_table_body((30, 20, 10))
    line_body = _middle_i32(0) * 2 + _middle_i32(100) * 2
    drawing = ezdgn.read(
        _with_phase_three_records(
            _record(5, 1, first),
            _record(3, 2, line_body, color=1),
            _record(5, 1, last),
        )
    )

    assert len(drawing.query("COLOR_TABLE")) == 2
    assert drawing.color_table is drawing.elements[-1]
    assert drawing.resolve_color(1) == (30, 20, 10)
    line = drawing.query("LINE")[0]
    assert isinstance(line, ezdgn.Line)
    assert line.style is not None
    assert line.style.rgb == (30, 20, 10)


def test_cli_emits_phase_three_entities_without_decoding_text() -> None:
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "ezdgn",
            "inspect",
            str(SMALLTEST),
            "--entities",
            "--json",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    payload = json.loads(result.stdout)
    assert payload["entity_count"] == 4
    assert payload["active_color_table_index"] is None
    assert len(payload["records"]) == 15
    text_payload = payload["records"][11]["entity"]
    assert text_payload["kind"] == "TEXT"
    assert text_payload["text_hex"] == b"Demo Text".hex()
    assert "text" not in text_payload
    assert payload["records"][12]["entity"]["kind"] == "ELLIPSE"
    assert payload["records"][13]["entity"]["vertices_uor"][2] == [
        49_441,
        25_235,
    ]


@pytest.mark.parametrize(
    ("complex_type", "complex_kind", "complex_class"),
    [
        (12, "COMPLEX CHAIN", ezdgn.ComplexChain),
        (14, "COMPLEX SHAPE", ezdgn.ComplexShape),
    ],
)
def test_restores_nested_cell_and_complex_hierarchy_without_flattening(
    complex_type: int,
    complex_kind: str,
    complex_class: type[ezdgn.ComplexElement],
) -> None:
    line = _record(3, 2, _middle_i32(1) * 2 + _middle_i32(2) * 2, complex=True)
    curve_body = _multipoint_body(((0, 0), (10, 10), (20, 5)))
    curve = _record(11, 2, curve_body, complex=True)

    complex_size = 48 + len(line) + len(curve)
    complex_body = bytearray(4)
    complex_body[0:2] = ((complex_size - 38) // 2).to_bytes(2, "little")
    complex_body[2:4] = (2).to_bytes(2, "little")
    complex_header = _record(
        complex_type,
        2,
        complex_body,
        complex=True,
        linkages=bytes(8),
    )

    cell_size = 92 + complex_size
    cell_body = bytearray(56)
    cell_body[0:2] = ((cell_size - 38) // 2).to_bytes(2, "little")
    cell_body[2:4] = _rad50("ABC")
    cell_body[4:6] = _rad50("123")
    cell_body[6:8] = (7).to_bytes(2, "little")
    cell_body[8:16] = b"\x01\x00\x02\x00\x04\x00\x08\x00"
    cell_body[16:20] = _middle_i32(-10)
    cell_body[20:24] = _middle_i32(-20)
    cell_body[24:28] = _middle_i32(30)
    cell_body[28:32] = _middle_i32(40)
    cell_body[32:36] = _middle_i32(214_748)
    cell_body[44:48] = _middle_i32(214_748)
    cell_body[48:52] = _middle_i32(100)
    cell_body[52:56] = _middle_i32(200)
    cell = _record(2, 2, cell_body)

    drawing = ezdgn.read(
        _with_phase_three_records(cell, complex_header, line, curve)
    )
    parsed_cell = drawing.query("CELL")[0]
    complex_element = drawing.query(complex_kind)[0]
    parsed_line = drawing.query("LINE")[0]
    parsed_curve = drawing.query("CURVE")[0]
    assert isinstance(parsed_cell, ezdgn.Cell)
    assert isinstance(complex_element, complex_class)
    assert isinstance(parsed_curve, ezdgn.Curve)
    assert parsed_cell.name == "ABC123"
    assert parsed_cell.cell_class == 7
    assert parsed_cell.levels == (1, 2, 4, 8)
    assert parsed_cell.origin_uor == (100, 200)
    assert parsed_cell.transform[0][0] == pytest.approx(0.999_998_301_3)
    assert drawing.children(parsed_cell) == (complex_element,)
    assert drawing.parent(complex_element) is parsed_cell
    assert drawing.children(complex_element) == (parsed_line, parsed_curve)
    assert drawing.descendants(parsed_cell) == (
        complex_element,
        parsed_line,
        parsed_curve,
    )
    assert parsed_line.parent_index == complex_element.record.index
    assert parsed_line.level == 2
    assert parsed_curve.vertices_uor == ((0, 0), (10, 10), (20, 5))
    assert drawing.entities == (parsed_cell,)
    assert drawing.all_entities == (
        parsed_cell,
        complex_element,
        parsed_line,
        parsed_curve,
    )


def test_decodes_linkages_and_applies_sub_uor_precision() -> None:
    association = bytes.fromhex("03 10 2f 7d 78 56 34 12")
    precision = bytes.fromhex(
        "07 10 a9 51 04 00 00 00 ff ff 02 00 03 00 fc ff"
    )
    unknown = bytes.fromhex("03 10 34 12 01 02 03 04")
    line_body = (
        _middle_i32(10)
        + _middle_i32(20)
        + _middle_i32(-30)
        + _middle_i32(40)
    )
    drawing = ezdgn.read(
        _with_phase_three_records(
            _record(
                3,
                2,
                line_body,
                linkages=association + precision + unknown,
            )
        )
    )
    line = drawing.query("LINE")[0]
    assert isinstance(line, ezdgn.Line)
    assert line.start_uor == (10, 20)
    assert line.end_uor == (-30, 40)
    assert line.start_uor_precise == pytest.approx(
        (10 - 1 / 32_767, 20 + 2 / 32_767)
    )
    assert line.end_uor_precise == pytest.approx(
        (-30 + 3 / 32_767, 40 - 4 / 32_767)
    )
    assert line.start_master == pytest.approx(
        tuple(value / 10_000 for value in line.start_uor_precise)
    )
    assert [link.kind for link in line.linkages] == [
        "ASSOCIATION_ID",
        "HIGH_PRECISION",
        "USER",
    ]
    assert line.association_ids == (0x1234_5678,)
    assert line.linkages[1].deltas == ((-1, 2), (3, -4))
    assert line.linkages[1].is_complete
    assert line.linkages[2].linkage_type == 0x1234
    assert line.linkages[2].raw_bytes == unknown


def test_decodes_text_node_and_bspline_groups_in_fixed_component_order() -> None:
    text_body = bytearray(24)
    text_body[0] = 3
    text_body[1] = 7
    text = _record(17, 2, text_body, complex=True)
    node_size = 70 + len(text)
    node_body = bytearray(34)
    node_body[0:2] = ((node_size - 38) // 2).to_bytes(2, "little")
    node_body[2:4] = (1).to_bytes(2, "little")
    node_body[4:6] = (42).to_bytes(2, "little")
    node_body[6:10] = bytes((80, 12, 3, 7))
    node_body[10:14] = _middle_i32(500)
    node_body[14:18] = _middle_i32(1_000)
    node_body[18:22] = _middle_i32(2_000)
    node_body[22:26] = _middle_i32(90 * 360_000)
    node_body[26:30] = _middle_i32(100)
    node_body[30:34] = _middle_i32(200)
    text_node = _record(7, 2, node_body)

    knot = _record(26, 2, _middle_i32(1_073_741_824), complex=True)
    pole = _record(
        21,
        2,
        _multipoint_body(((0, 0), (10, 20), (30, 40), (50, 60))),
        complex=True,
    )
    weight = _record(
        28,
        2,
        _middle_i32(2_147_483_647) * 4,
        complex=True,
    )
    spline_size = 46 + len(knot) + len(pole) + len(weight)
    spline_body = bytearray(10)
    spline_body[0:4] = _middle_i32((spline_size - 40) // 2)
    spline_body[4] = 0x41  # order 3, rational
    spline_body[5] = 2
    spline_body[6:8] = (4).to_bytes(2, "little")
    spline_body[8:10] = (1).to_bytes(2, "little")
    spline = _record(27, 2, spline_body)

    drawing = ezdgn.read(
        _with_phase_three_records(
            text_node,
            text,
            spline,
            knot,
            pole,
            weight,
        )
    )
    node = drawing.query("TEXT_NODE")[0]
    assert isinstance(node, ezdgn.TextNode)
    assert node.node_number == 42
    assert node.num_text_strings == 1
    assert node.line_spacing_raw == 500
    assert node.line_spacing_master == pytest.approx(0.05)
    assert node.rotation_degrees == 90
    assert [child.kind for child in drawing.children(node)] == ["TEXT"]

    curve = drawing.query("BSPLINE_CURVE")[0]
    assert isinstance(curve, ezdgn.BSplineCurve)
    assert curve.order == 3
    assert curve.is_rational
    assert curve.num_poles == 4
    assert curve.num_knots == 1
    assert [child.kind for child in drawing.children(curve)] == [
        "BSPLINE_KNOT",
        "BSPLINE_POLE",
        "BSPLINE_WEIGHT",
    ]
    parsed_knot = drawing.children(curve)[0]
    assert isinstance(parsed_knot, ezdgn.BSplineKnot)
    assert parsed_knot.values == pytest.approx((0.5,))
    parsed_pole = drawing.children(curve)[1]
    assert isinstance(parsed_pole, ezdgn.BSplinePole)
    assert parsed_pole.vertices_uor[-1] == (50, 60)
    parsed_weight = drawing.children(curve)[2]
    assert isinstance(parsed_weight, ezdgn.BSplineWeight)
    assert parsed_weight.values == pytest.approx((1, 1, 1, 1))


def test_rejects_complex_count_and_bspline_order_mismatches() -> None:
    line = _record(3, 2, bytes(16), complex=True)
    group_size = 48 + len(line)
    header_body = bytearray(4)
    header_body[0:2] = ((group_size - 38) // 2).to_bytes(2, "little")
    header_body[2:4] = (2).to_bytes(2, "little")
    header = _record(12, 2, header_body, linkages=bytes(8))
    with pytest.raises(ezdgn.InvalidDgnError, match="declares 2 direct"):
        ezdgn.read(_with_phase_three_records(header, line))

    pole = _record(21, 2, _multipoint_body(((0, 0), (1, 1))), complex=True)
    spline_size = 46 + len(pole)
    spline_body = bytearray(10)
    spline_body[0:4] = _middle_i32((spline_size - 40) // 2)
    spline_body[4] = 0x40  # rational, but no weight follows
    spline_body[6:8] = (2).to_bytes(2, "little")
    spline = _record(27, 2, spline_body)
    with pytest.raises(ezdgn.InvalidDgnError, match="missing rational weight"):
        ezdgn.read(_with_phase_three_records(spline, pole))


def test_decodes_trimmed_bspline_surface_without_stroking_it() -> None:
    boundary_body = bytearray(20)
    boundary_body[0:2] = (1).to_bytes(2, "little")
    boundary_body[2:4] = (2).to_bytes(2, "little")
    boundary_body[4:8] = _middle_i32(0)
    boundary_body[8:12] = _middle_i32(0)
    boundary_body[12:16] = _middle_i32(2_147_483_647)
    boundary_body[16:20] = _middle_i32(2_147_483_647)
    boundary = _record(25, 2, boundary_body, complex=True)
    first_row = _record(
        21,
        2,
        _multipoint_body(((0, 0), (10, 0))),
        complex=True,
    )
    second_row = _record(
        21,
        2,
        _multipoint_body(((0, 10), (10, 10))),
        complex=True,
    )
    surface_size = 58 + len(boundary) + len(first_row) + len(second_row)
    surface_body = bytearray(22)
    surface_body[0:4] = _middle_i32((surface_size - 40) // 2)
    surface_body[4] = 0x10  # order 2, surface display enabled
    surface_body[5] = 1
    surface_body[6:8] = (2).to_bytes(2, "little")
    surface_body[10:12] = (4).to_bytes(2, "little")
    surface_body[12] = 0
    surface_body[14:16] = (2).to_bytes(2, "little")
    surface_body[18:20] = (4).to_bytes(2, "little")
    surface_body[20:22] = (1).to_bytes(2, "little")
    surface = _record(24, 2, surface_body)

    drawing = ezdgn.read(
        _with_phase_three_records(
            surface, boundary, first_row, second_row
        )
    )
    parsed = drawing.query("BSPLINE_SURFACE")[0]
    assert isinstance(parsed, ezdgn.BSplineSurface)
    assert parsed.u_order == 2
    assert parsed.v_order == 2
    assert parsed.num_poles_u == 2
    assert parsed.num_poles_v == 2
    assert parsed.num_bounds == 1
    assert not parsed.is_rational
    assert [child.kind for child in drawing.children(parsed)] == [
        "BSPLINE_SURFACE_BOUNDARY",
        "BSPLINE_POLE",
        "BSPLINE_POLE",
    ]
    parsed_boundary = drawing.children(parsed)[0]
    assert isinstance(parsed_boundary, ezdgn.BSplineSurfaceBoundary)
    assert parsed_boundary.number == 1
    assert parsed_boundary.vertices_raw == (
        (0, 0),
        (2_147_483_647, 2_147_483_647),
    )
    assert parsed_boundary.vertices_uv == ((0.0, 0.0), (1.0, 1.0))


def _with_phase_three_records(*records: bytes) -> bytes:
    controls = SMALLTEST.read_bytes()[:10_136]
    return controls + b"".join(records) + b"\xff\xff"


def _record(
    element_type: int,
    level: int,
    body: bytes | bytearray,
    *,
    color: int = 0,
    complex: bool = False,
    linkages: bytes = b"",
) -> bytes:
    semantic_size = 36 + len(body)
    size = semantic_size + len(linkages)
    assert size % 2 == 0
    result = bytearray(size)
    result[0] = level | (0x80 if complex else 0)
    result[1] = element_type
    result[2:4] = (size // 2 - 2).to_bytes(2, "little")
    for offset in (4, 8, 12, 16, 20, 24):
        result[offset : offset + 4] = _offset_i32(0)
    result[35] = color
    result[36:semantic_size] = body
    if linkages:
        assert semantic_size >= 36 and semantic_size % 2 == 0
        result[30:32] = ((semantic_size - 32) // 2).to_bytes(2, "little")
        result[32:34] = (0x0800).to_bytes(2, "little")
        result[semantic_size:] = linkages
    return bytes(result)


def _color_table_body(color_one: tuple[int, int, int]) -> bytearray:
    body = bytearray(770)
    body[5:8] = bytes((255, 255, 255))
    body[8:11] = bytes(color_one)
    return body


def _multipoint_body(points: tuple[tuple[int, int], ...]) -> bytearray:
    body = bytearray(2 + len(points) * 8)
    body[0:2] = len(points).to_bytes(2, "little")
    for index, (x, y) in enumerate(points):
        body[2 + index * 8 : 6 + index * 8] = _middle_i32(x)
        body[6 + index * 8 : 10 + index * 8] = _middle_i32(y)
    return body


def _rad50(value: str) -> bytes:
    assert len(value) == 3
    encoded = 0
    for character in value:
        if "A" <= character <= "Z":
            digit = ord(character) - ord("A") + 1
        elif "0" <= character <= "9":
            digit = ord(character) - ord("0") + 30
        elif character == "$":
            digit = 27
        elif character == ".":
            digit = 28
        else:
            digit = 0
        encoded = encoded * 40 + digit
    return encoded.to_bytes(2, "little")


def _middle_i32(value: int) -> bytes:
    return _middle_u32(value & 0xFFFF_FFFF)


def _middle_u32(value: int) -> bytes:
    return bytes(
        (
            (value >> 16) & 0xFF,
            (value >> 24) & 0xFF,
            value & 0xFF,
            (value >> 8) & 0xFF,
        )
    )


def _offset_i32(value: int) -> bytes:
    return _middle_u32((value & 0xFFFF_FFFF) ^ 0x8000_0000)
