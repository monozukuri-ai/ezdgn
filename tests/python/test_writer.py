from __future__ import annotations

from io import BytesIO
from pathlib import Path

import pytest

import ezdgn

DATA = Path(__file__).parents[1] / "data" / "dgn"
SEED_2D = DATA / "v7" / "seed_2d.dgn"
SEED_3D = DATA / "v7" / "seed_3d.dgn"


def test_new_uses_bundled_empty_seed_when_omitted() -> None:
    document = ezdgn.new()

    assert document.source_path is None
    assert document.design_settings.dimension == 2
    assert document.design_settings.uor_per_master == 120
    document.modelspace().add_line((0, 0), (1, 1))

    drawing = document.readback()
    assert len(drawing.elements) == 4
    line = drawing.query("LINE")[0]
    assert isinstance(line, ezdgn.Line)
    assert line.start_master == pytest.approx((0, 0))
    assert line.end_master == pytest.approx((1, 1))


def test_seed_writer_round_trips_every_phase_five_entity(tmp_path: Path) -> None:
    document = ezdgn.new(SEED_2D)
    assert document.design_settings.dimension == 2
    assert document.source_path == str(SEED_2D)
    modelspace = document.modelspace()
    assert modelspace is document.modelspace()

    line = modelspace.add_line(
        (1.25, -2.5),
        (4.5, 3.75),
        dgnattribs={
            "level": 7,
            "color": 12,
            "line_style": 3,
            "line_weight": 5,
            "graphic_group": 42,
        },
    )
    modelspace.add_line_string([(0, 0), (1, 2), (3, 1)])
    shape = modelspace.add_shape(
        [(0, 0), (2, 0), (2, 2)],
        fill_color=83,
    )
    modelspace.add_curve([(-1, 0), (0, 1), (1, 0)])
    modelspace.add_ellipse((10, 20), 4, 2, rotation=30)
    modelspace.add_circle((20, 20), 2)
    modelspace.add_arc(
        (-5, 6),
        3,
        1.5,
        rotation=15,
        start_angle=45,
        sweep_angle=-90,
    )
    text = modelspace.add_text(
        "Phase 5",
        (2, 8),
        width=0.5,
        height=1.0,
        rotation=10,
        font_id=3,
        justification=7,
    )

    assert line.dxftype() == "LINE"
    assert shape.points[0] == shape.points[-1]
    assert text.text_bytes == b"Phase 5"
    assert len(modelspace) == 8
    assert modelspace.query("line-string")[0].kind == "LINE_STRING"
    assert document.entities == tuple(modelspace)

    output = document.to_bytes()
    assert output.endswith(b"\xff\xff")
    roundtrip = document.readback()
    assert [entity.kind for entity in roundtrip.entities] == [
        "LINE",
        "LINE_STRING",
        "SHAPE",
        "CURVE",
        "ELLIPSE",
        "ELLIPSE",
        "ARC",
        "TEXT",
    ]
    assert len(roundtrip.elements) == 11  # three seed controls plus graphics

    parsed_line = roundtrip.query("LINE")[0]
    assert isinstance(parsed_line, ezdgn.Line)
    assert parsed_line.start_master == pytest.approx((1.25, -2.5))
    assert parsed_line.end_master == pytest.approx((4.5, 3.75))
    assert parsed_line.common_header is not None
    assert parsed_line.common_header.graphic_group == 42
    assert parsed_line.level == 7
    assert parsed_line.style == ezdgn.BasicStyle(12, 3, 5, (0, 254, 160))

    parsed_shape = roundtrip.query("SHAPE")[0]
    assert isinstance(parsed_shape, ezdgn.Shape)
    assert parsed_shape.is_closed
    assert parsed_shape.style is not None
    assert parsed_shape.style.fill_color_index == 83
    assert parsed_shape.linkages[0].kind == "SHAPE_FILL"

    parsed_ellipse = roundtrip.query("ELLIPSE")[0]
    assert isinstance(parsed_ellipse, ezdgn.Ellipse)
    assert parsed_ellipse.center_master == pytest.approx((10, 20))
    assert parsed_ellipse.primary_axis_master == pytest.approx(4)
    assert parsed_ellipse.secondary_axis_master == pytest.approx(2)
    assert parsed_ellipse.rotation_degrees == pytest.approx(30)

    parsed_arc = roundtrip.query("ARC")[0]
    assert isinstance(parsed_arc, ezdgn.Arc)
    assert parsed_arc.start_angle_degrees == pytest.approx(45)
    assert parsed_arc.sweep_angle_degrees == pytest.approx(-90)

    parsed_text = roundtrip.query("TEXT")[0]
    assert isinstance(parsed_text, ezdgn.Text)
    assert parsed_text.text_bytes == b"Phase 5"
    assert parsed_text.length_multiplier_master == pytest.approx(0.5)
    assert parsed_text.height_multiplier_master == pytest.approx(1.0)
    assert parsed_text.rotation_degrees == pytest.approx(10)

    destination = tmp_path / "phase5.dgn"
    document.saveas(destination)
    assert destination.read_bytes() == output
    assert ezdgn.readfile(destination).query("LINE")[0].start_master == pytest.approx(
        (1.25, -2.5)
    )

    stream = BytesIO()
    assert document.write(stream) == len(output)
    assert stream.getvalue() == output


def test_writer_preserves_raw_text_encoding_and_seed_copy_policy() -> None:
    minimal = ezdgn.new(SEED_2D)
    minimal.modelspace().add_text("日本語", (0, 0), encoding="cp932")
    minimal_scan = ezdgn.scan_records(minimal.to_bytes())
    assert len(minimal_scan.records) == 4
    parsed = minimal.readback().query("TEXT")[0]
    assert isinstance(parsed, ezdgn.Text)
    with pytest.raises(UnicodeDecodeError):
        parsed.decode_text()
    assert parsed.decode_text("cp932") == "日本語"

    complete = ezdgn.new(SEED_2D, copy_seed_elements=True)
    complete.modelspace().add_line((0, 0), (1, 1))
    complete_scan = ezdgn.scan_records(complete.to_bytes())
    assert len(complete_scan.records) == 13
    assert [record.element_type for record in complete_scan.records[:12]] == [
        record.element_type for record in ezdgn.scan_records(SEED_2D).records
    ]


def test_writer_rejects_invalid_seed_attributes_and_geometry() -> None:
    with pytest.raises(ezdgn.UnsupportedDgnError, match="3D seeds"):
        ezdgn.new(SEED_3D)
    with pytest.raises(ValueError, match="level must be between"):
        ezdgn.DgnAttributes(level=64)
    with pytest.raises(ValueError, match="unsupported DGN attributes"):
        ezdgn.new(SEED_2D).modelspace().add_line(
            (0, 0), (1, 1), dgnattribs={"layer": 1}
        )
    with pytest.raises(ValueError, match="at least 2 points"):
        ezdgn.new(SEED_2D).modelspace().add_line_string([(0, 0)])
    with pytest.raises(ValueError, match="at most 101 points"):
        ezdgn.new(SEED_2D).modelspace().add_curve([(0, 0)] * 102)
    with pytest.raises(ValueError, match="after closure"):
        ezdgn.new(SEED_2D).modelspace().add_shape(
            [(index, index % 2) for index in range(101)]
        )
    with pytest.raises(ValueError, match="sweep_angle"):
        ezdgn.new(SEED_2D).modelspace().add_arc(
            (0, 0), 1, sweep_angle=361
        )
    with pytest.raises(ValueError, match="at most 255 bytes"):
        ezdgn.new(SEED_2D).modelspace().add_text(b"x" * 256, (0, 0))
    with pytest.raises(ValueError, match="must be finite"):
        ezdgn.new(SEED_2D).modelspace().add_line((float("nan"), 0), (1, 1))

    outside = ezdgn.new(SEED_2D)
    outside.modelspace().add_line((1.0e20, 0), (1, 1))
    with pytest.raises(ezdgn.InvalidDgnError, match="outside the V7 design plane"):
        outside.to_bytes()
