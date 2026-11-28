from __future__ import annotations

import os
import subprocess
import sys
from dataclasses import replace
from pathlib import Path

import pytest

matplotlib = pytest.importorskip("matplotlib")
matplotlib.use("Agg")
from matplotlib import pyplot as plt  # noqa: E402
from matplotlib.collections import LineCollection, PolyCollection  # noqa: E402
from matplotlib.path import Path as MplPath  # noqa: E402

import ezdgn  # noqa: E402
import ezdgn.plotting  # noqa: E402

DATA = Path(__file__).parents[1] / "data" / "dgn"
SMALLTEST = DATA / "v7" / "smalltest.dgn"
SEED_2D = DATA / "v7" / "seed_2d.dgn"
V8 = DATA / "v8" / "test_dgnv8.dgn"


MESH_EDGES = [
    ((0, 0), (2, 0)),
    ((2, 0), (2, 2)),
    ((0, 0), (0, 2)),
    ((0, 2), (2, 0)),
]


def path_edges(path: MplPath, scale: float = 1) -> list[tuple[tuple[float, ...], ...]]:
    """The MOVETO/LINETO pairs of a mesh edge path as ``(start, end)`` points."""

    assert list(path.codes) == [MplPath.MOVETO, MplPath.LINETO] * (len(path.codes) // 2)
    points = [tuple(value / scale for value in vertex) for vertex in path.vertices]
    return list(zip(points[0::2], points[1::2]))


def moved_mesh(element: ezdgn.V8Element, offset: int, **common: int) -> ezdgn.V8Element:
    vertices = tuple(
        ezdgn.V8Point(
            uor=tuple(value + offset * 1000 for value in point.uor),
            master=tuple(value + offset for value in point.master),
        )
        for point in element.data.vertices
    )
    return replace(
        element,
        common=replace(element.common, **common),
        data=replace(element.data, vertices=vertices),
    )


def with_elements(
    document: ezdgn.V8Document, *elements: ezdgn.V8Element
) -> ezdgn.V8Document:
    return replace(document, models=(replace(document.models[0], elements=elements),))


@pytest.mark.parametrize("coordinate_space,scale", [("master", 1), ("uor", 1000)])
def test_mesh_plot_hides_edges_and_draws_shared_edges_once(
    mesh_document: ezdgn.V8Document, coordinate_space: str, scale: int
) -> None:
    figure, axes = ezdgn.plot(mesh_document, coordinate_space=coordinate_space)

    assert not axes.collections
    (patch,) = axes.patches
    assert path_edges(patch.get_path(), scale) == MESH_EDGES
    assert patch.get_facecolor()[3] == 0
    low, high = axes.dataLim.get_points()
    assert tuple(low / scale) == (0, 0)
    assert tuple(high / scale) == (2, 2)
    plt.close(figure)


def test_mesh_edge_path_is_one_path_of_visible_edges(
    mesh_document: ezdgn.V8Document,
) -> None:
    data = mesh_document.models[0].elements[0].data

    assert path_edges(ezdgn.plotting.mesh_edge_path(data)) == MESH_EDGES
    in_uor = ezdgn.plotting.mesh_edge_path(data, coordinate_space="uor")
    assert path_edges(in_uor, 1000) == MESH_EDGES

    hidden = replace(
        data,
        faces=tuple(
            replace(face, visible_edges=(False,) * len(face.visible_edges))
            for face in data.faces
        ),
    )
    assert len(ezdgn.plotting.mesh_edge_path(hidden).vertices) == 0
    collapsed = replace(data, faces=(ezdgn.V8MeshFace((1, 1, 1), (True, True, True)),))
    assert len(ezdgn.plotting.mesh_edge_path(collapsed).vertices) == 0
    assert len(ezdgn.plotting.mesh_edge_path(replace(data, faces=())).vertices) == 0
    with pytest.raises(ValueError, match="coordinate_space"):
        ezdgn.plotting.mesh_edge_path(data, coordinate_space="screen")  # type: ignore[arg-type]


def test_meshes_of_one_style_share_a_compound_path(
    mesh_document: ezdgn.V8Document,
) -> None:
    mesh = mesh_document.models[0].elements[0]
    document = with_elements(
        mesh_document,
        mesh,
        moved_mesh(mesh, 10),
        moved_mesh(mesh, 20, color_index=mesh.common.color_index + 1),
        replace(mesh, data=replace(mesh.data, faces=())),
    )

    figure, axes = ezdgn.plot(document)

    assert not axes.collections
    assert tuple(axes.dataLim.minpos) == (2, 2)
    assert sorted(len(path_edges(patch.get_path())) for patch in axes.patches) == [4, 8]
    shared, other = sorted(axes.patches, key=lambda patch: -len(patch.get_path().codes))
    assert path_edges(shared.get_path())[:4] == MESH_EDGES
    assert path_edges(shared.get_path())[4] == ((10, 10), (12, 10))
    assert path_edges(other.get_path())[0] == ((20, 20), (22, 20))
    assert shared.get_edgecolor() != other.get_edgecolor()
    assert tuple(axes.dataLim.get_points()[1]) == (22, 22)
    plt.close(figure)


def test_mesh_paths_are_bounded_in_size(
    mesh_document: ezdgn.V8Document, monkeypatch: pytest.MonkeyPatch
) -> None:
    mesh = mesh_document.models[0].elements[0]
    document = with_elements(
        mesh_document, mesh, moved_mesh(mesh, 10), moved_mesh(mesh, 20)
    )

    monkeypatch.setattr(ezdgn.plotting, "_MESH_PATH_EDGES", 8)
    figure, axes = ezdgn.plot(document)
    assert [len(path_edges(patch.get_path())) for patch in axes.patches] == [8, 4]
    plt.close(figure)

    monkeypatch.setattr(ezdgn.plotting, "_MESH_PATH_EDGES", 3)
    figure, axes = ezdgn.plot(document)
    paths = [path_edges(patch.get_path()) for patch in axes.patches]
    assert [len(edges) for edges in paths] == [3, 1] * 3
    assert paths[0] + paths[1] == MESH_EDGES
    assert tuple(axes.dataLim.get_points()[1]) == (22, 22)
    plt.close(figure)


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


def test_plot_renders_native_v8_document() -> None:
    document = ezdgn.read_v8(V8)

    figure, axes = ezdgn.plot(document, show_text=True, curve_steps=32)

    line_collections = [
        item for item in axes.collections if isinstance(item, LineCollection)
    ]
    polygon_collections = [
        item for item in axes.collections if isinstance(item, PolyCollection)
    ]
    assert sum(len(item.get_segments()) for item in line_collections) > 20
    assert sum(len(item.get_paths()) for item in polygon_collections) >= 1
    assert len(axes.patches) >= 1
    assert axes.get_xlabel() == "x [m]"
    assert max(map(abs, axes.get_xlim())) < 10
    assert max(map(abs, axes.get_ylim())) < 10
    plt.close(figure)


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
