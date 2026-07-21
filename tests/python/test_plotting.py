from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

matplotlib = pytest.importorskip("matplotlib")
matplotlib.use("Agg")
from matplotlib import pyplot as plt  # noqa: E402
from matplotlib.collections import LineCollection, PolyCollection  # noqa: E402

import ezdgn  # noqa: E402

DATA = Path(__file__).parents[1] / "data" / "dgn"
SMALLTEST = DATA / "v7" / "smalltest.dgn"
SEED_2D = DATA / "v7" / "seed_2d.dgn"


def test_plot_renders_smalltest_without_flattening_entities() -> None:
    drawing = ezdgn.readfile(SMALLTEST)

    figure, axes = ezdgn.plot(drawing)

    assert figure is axes.figure
    line_collections = [
        item for item in axes.collections if isinstance(item, LineCollection)
    ]
    polygon_collections = [
        item for item in axes.collections if isinstance(item, PolyCollection)
    ]
    assert len(axes.lines) == 0
    assert sum(len(item.get_segments()) for item in line_collections) == 2
    assert sum(len(item.get_paths()) for item in polygon_collections) == 1
    assert len(axes.patches) == 1  # text
    assert axes.get_xlabel() == "x [mu]"
    assert axes.get_ylabel() == "y [mu]"
    assert axes.get_xlim()[0] < 0.4
    assert axes.get_xlim()[1] > 9.6
    assert axes.get_ylim()[0] < 0.0
    assert axes.get_ylim()[1] > 9.2
    assert drawing.query("ELLIPSE")[0].primary_axis_master == pytest.approx(
        4.679_606_583_891_428
    )
    plt.close(figure)


def test_plot_samples_additional_primitives_and_accepts_uor_space() -> None:
    document = ezdgn.new(SEED_2D)
    modelspace = document.modelspace()
    modelspace.add_line_string([(0, 0), (1, 2), (3, 1)])
    modelspace.add_curve([(-1, 0), (0, 1), (1, 0)])
    modelspace.add_arc(
        (5, 5),
        3,
        1.5,
        start_angle=45,
        sweep_angle=-90,
        rotation=15,
    )
    drawing = document.readback()

    figure, axes = ezdgn.plot(
        drawing,
        coordinate_space="uor",
        show_text=False,
        show_axes=False,
        monochrome=True,
        curve_steps=32,
    )

    line_collections = [
        item for item in axes.collections if isinstance(item, LineCollection)
    ]
    assert len(line_collections) == 1
    segments = line_collections[0].get_segments()
    assert len(segments) == 3
    assert len(segments[-1]) == 9
    assert not axes.axison
    plt.close(figure)


def test_plot_batches_many_entities_by_display_style() -> None:
    document = ezdgn.new(SEED_2D)
    modelspace = document.modelspace()
    for index in range(250):
        modelspace.add_line(
            (index, 0),
            (index, 10),
            dgnattribs={"color": 1, "line_weight": 1},
        )
    for index in range(250):
        modelspace.add_line(
            (index, 20),
            (index, 30),
            dgnattribs={"color": 2, "line_weight": 3},
        )
    for index in range(50):
        modelspace.add_text("Label", (index * 5, 40), height=1)

    figure, axes = ezdgn.plot(document.readback())

    line_collections = [
        item for item in axes.collections if isinstance(item, LineCollection)
    ]
    assert len(line_collections) == 2
    assert sum(len(item.get_segments()) for item in line_collections) == 500
    assert len(axes.lines) == 0
    assert len(axes.patches) == 1
    plt.close(figure)


def test_save_plot_writes_png(tmp_path: Path) -> None:
    output = tmp_path / "preview.png"
    open_figures = set(plt.get_fignums())

    result = ezdgn.save_plot(
        ezdgn.readfile(SMALLTEST),
        output,
        dpi=96,
        text_encoding="ascii",
    )

    assert result == output
    assert output.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
    assert set(plt.get_fignums()) == open_figures


def test_plot_validates_options() -> None:
    drawing = ezdgn.readfile(SMALLTEST)
    with pytest.raises(ValueError, match="coordinate_space"):
        ezdgn.plot(drawing, coordinate_space="screen")  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="at least 8"):
        ezdgn.plot(drawing, curve_steps=7)
    with pytest.raises(ValueError, match="greater than zero"):
        ezdgn.save_plot(drawing, "unused.png", dpi=0)


def test_cli_plot_writes_png(tmp_path: Path) -> None:
    output = tmp_path / "cli-preview.png"
    environment = os.environ.copy()
    environment["MPLBACKEND"] = "Agg"

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "ezdgn",
            "plot",
            str(SMALLTEST),
            "-o",
            str(output),
            "--encoding",
            "ascii",
            "--hide-axes",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=environment,
    )

    assert result.stdout.strip() == f"wrote: {output}"
    assert output.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
