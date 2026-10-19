from dataclasses import replace
from pathlib import Path

import pytest

import ezdgn


@pytest.fixture
def mesh_document() -> ezdgn.V8Document:
    """Synthetic mesh over fixture metadata for plotting and CLI contract tests."""
    path = Path(__file__).parents[1] / "data" / "dgn" / "v8" / "test_dgnv8.dgn"
    document = ezdgn.read_v8(path)
    model = document.models[0]
    vertices = tuple(
        ezdgn.V8Point(uor=xyz, master=tuple(value / 1000 for value in xyz))
        for xyz in ((0, 0, 0), (2000, 0, 0), (2000, 2000, 0), (0, 2000, 1000))
    )
    mesh = replace(
        model.elements[0],
        index=0,
        parent_index=None,
        child_indices=(),
        data=ezdgn.V8ElementData(
            kind="MESH",
            style=1,
            vertices=vertices,
            faces=(
                ezdgn.V8MeshFace((0, 1, 2), (True, True, False)),
                ezdgn.V8MeshFace((0, 3, 1), (True, True, True)),
            ),
        ),
    )
    return replace(document, models=(replace(model, elements=(mesh,)),))
