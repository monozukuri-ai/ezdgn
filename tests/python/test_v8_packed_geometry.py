from __future__ import annotations

import json
import pickle
import struct
from dataclasses import replace
from pathlib import Path

import pytest

import ezdgn
from ezdgn import V8MeshFace, V8MeshFaceArray, V8Point, V8PointArray, V8ValueArray
from ezdgn.v8 import _element_data_from_core


V8 = Path(__file__).parents[1] / "data" / "dgn" / "v8" / "test_dgnv8.dgn"
LINE_STRING_INDEX = 10
POINT_STRING_INDEX = 12
LINE_STRING_POINTS = (
    V8Point(uor=(0.0, 10_000.0, 20_000.0), master=(0.0, 1.0, 2.0)),
    V8Point(uor=(30_000.0, 40_000.0, 50_000.0), master=(3.0, 4.0, 5.0)),
    V8Point(uor=(60_000.0, 70_000.0, 80_000.0), master=(6.0, 7.0, 8.0)),
)
POINT_STRING_ORIENTATIONS = ((1.0, 0.0, 0.0, 1.0), (1.0, 0.0, 0.0, 1.0))
FACES = (
    V8MeshFace((0, 1, 2), (True, True, False)),
    V8MeshFace((0, 3, 1, 2), (True, False, True, True)),
)
FACE_OFFSETS = (0, 3, 7)
FACE_POINT_INDICES = (0, 1, 2, 0, 3, 1, 2)
FACE_VISIBLE_EDGES = bytes([1, 1, 0, 1, 0, 1, 1])


def packed_points(points: tuple[V8Point, ...]) -> tuple[bytes, bytes]:
    uor = b"".join(struct.pack("<3d", *point.uor) for point in points)
    master = b"".join(struct.pack("<3d", *point.master) for point in points)
    return uor, master


def packed_faces() -> tuple[bytes, bytes, bytes]:
    return (
        struct.pack("<3I", *FACE_OFFSETS),
        struct.pack("<7I", *FACE_POINT_INDICES),
        FACE_VISIBLE_EDGES,
    )


def assert_stands_for(packed: object, expected: tuple[object, ...]) -> None:
    assert not isinstance(packed, tuple)
    assert len(packed) == len(expected)  # type: ignore[arg-type]
    assert tuple(packed) == expected  # type: ignore[arg-type]
    assert packed == expected
    assert expected == packed
    assert not packed != expected
    assert hash(packed) == hash(expected)
    assert packed != expected[:-1]
    assert packed != list(expected)


def test_file_vertices_are_a_packed_sequence_equal_to_their_tuple() -> None:
    vertices = ezdgn.read_v8(V8).models[0].elements[LINE_STRING_INDEX].data.vertices

    assert isinstance(vertices, V8PointArray)
    assert_stands_for(vertices, LINE_STRING_POINTS)
    assert vertices[0] == LINE_STRING_POINTS[0]
    assert vertices[-1] == LINE_STRING_POINTS[2]
    assert vertices[1:] == LINE_STRING_POINTS[1:]
    assert vertices[::-1] == LINE_STRING_POINTS[::-1]
    assert isinstance(vertices[1:], tuple)
    assert [point.master for point in vertices] == [
        point.master for point in LINE_STRING_POINTS
    ]
    assert LINE_STRING_POINTS[1] in vertices
    assert vertices.index(LINE_STRING_POINTS[2]) == 2
    first = LINE_STRING_POINTS[:1]
    assert vertices + first == LINE_STRING_POINTS + first
    assert first + vertices == first + LINE_STRING_POINTS
    assert vertices + vertices == LINE_STRING_POINTS * 2
    with pytest.raises(IndexError):
        vertices[3]
    with pytest.raises(IndexError):
        vertices[-4]


def test_point_array_exposes_little_endian_coordinate_buffers() -> None:
    vertices = ezdgn.read_v8(V8).models[0].elements[LINE_STRING_INDEX].data.vertices
    uor, master = packed_points(LINE_STRING_POINTS)

    assert vertices.uor_bytes == uor
    assert vertices.master_bytes == master
    assert struct.unpack("<9d", vertices.master_bytes)[3:6] == (3.0, 4.0, 5.0)
    assert V8PointArray(uor, master) == vertices
    assert V8PointArray(bytearray(uor), memoryview(master)) == vertices
    assert V8PointArray.from_points(LINE_STRING_POINTS) == vertices
    assert V8PointArray.from_points(()) == ()
    assert not V8PointArray(b"", b"")
    assert repr(vertices).startswith("<V8PointArray of 3: V8Point(uor=(0.0, 10000.0")
    assert repr(V8PointArray.from_points(LINE_STRING_POINTS * 2)).endswith(", ...>")
    with pytest.raises(ValueError, match="XYZ triples"):
        V8PointArray(uor, master[:-8])
    with pytest.raises(ValueError, match="XYZ triples"):
        V8PointArray(uor[:-8], master[:-8])
    with pytest.raises(TypeError):
        V8PointArray(48, 48)  # type: ignore[arg-type]


def test_equal_values_compare_equal_when_their_bytes_differ() -> None:
    zero = V8PointArray.from_points((V8Point((0.0, 0.0, 0.0), (0.0, 0.0, 0.0)),))
    negative_zero = V8PointArray.from_points(
        (V8Point((-0.0, 0.0, 0.0), (0.0, 0.0, 0.0)),)
    )

    assert zero.uor_bytes != negative_zero.uor_bytes
    assert zero == negative_zero
    assert hash(zero) == hash(negative_zero)
    integers = V8ValueArray.from_values("i", (1, 2))
    assert integers == V8ValueArray.from_values("d", (1.0, 2.0))


def test_equal_nan_buffers_work_as_dictionary_keys() -> None:
    nan = float("nan")
    point = V8Point((nan, 1, 2), (0, nan, 2))
    for first in (
        V8PointArray.from_points((point,)),
        V8ValueArray.from_values("d", (nan,) * 5),
        V8ValueArray.from_values("d", ((1, nan),), width=2),
    ):
        second = pickle.loads(pickle.dumps(first))
        assert first == second
        assert {first: 1}[second] == 1


def test_value_array_checks_the_width_of_each_input_row() -> None:
    with pytest.raises(ValueError, match="2 numbers per item"):
        V8ValueArray.from_values("i", [(1,), (2, 3, 4)], width=2)


def test_file_orientations_are_packed_quadruples() -> None:
    data = ezdgn.read_v8(V8).models[0].elements[POINT_STRING_INDEX].data

    assert isinstance(data.orientations, V8ValueArray)
    assert (data.orientations.format, data.orientations.width) == ("d", 4)
    assert_stands_for(data.orientations, POINT_STRING_ORIENTATIONS)
    assert data.orientations[1] == POINT_STRING_ORIENTATIONS[1]
    assert len(data.orientations.data_bytes) == 2 * 4 * 8
    assert len(data.vertices) == len(data.orientations)


def test_value_array_holds_doubles_or_integers() -> None:
    doubles = V8ValueArray("d", struct.pack("<3d", 1.5, -2.0, 3.25))
    integers = V8ValueArray.from_values("i", (1, 2, -3, 0))

    assert_stands_for(doubles, (1.5, -2.0, 3.25))
    assert_stands_for(integers, (1, 2, -3, 0))
    assert integers.data_bytes == struct.pack("<4i", 1, 2, -3, 0)
    assert (integers.format, integers.width) == ("i", 1)
    assert integers[-2:] == (-3, 0)
    assert all(isinstance(value, int) for value in integers)
    assert V8ValueArray.from_values("d", POINT_STRING_ORIENTATIONS, width=4) == (
        POINT_STRING_ORIENTATIONS
    )
    with pytest.raises(ValueError, match="format"):
        V8ValueArray("f", b"")  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="whole items"):
        V8ValueArray("d", bytes(12))
    with pytest.raises(ValueError, match="whole items"):
        V8ValueArray("d", bytes(16), width=4)
    with pytest.raises(ValueError, match="width"):
        V8ValueArray("d", b"", width=0)


def test_mesh_face_array_stands_for_its_faces() -> None:
    faces = V8MeshFaceArray(*packed_faces())

    assert_stands_for(faces, FACES)
    assert faces[1] == FACES[1]
    assert faces[-1].visible_edges == (True, False, True, True)
    assert faces[:1] == FACES[:1]
    assert packed_faces() == (
        faces.offsets_bytes,
        faces.point_indices_bytes,
        faces.visible_edges_bytes,
    )
    assert V8MeshFaceArray.from_faces(FACES) == faces
    assert V8MeshFaceArray.from_faces(()) == ()
    assert len(V8MeshFaceArray(bytes(4), b"", b"")) == 0
    with pytest.raises(IndexError):
        faces[2]


def test_mesh_face_array_rejects_inconsistent_buffers() -> None:
    offsets, point_indices, visible_edges = packed_faces()

    for arguments in (
        (b"", b"", b""),
        (offsets[:-1], point_indices, visible_edges),
        (struct.pack("<4I", 0, 5, 3, 7), point_indices, visible_edges),
        (struct.pack("<3I", 0, 8, 7), point_indices, visible_edges),
        (offsets, point_indices[:-4], visible_edges),
        (offsets, point_indices, visible_edges[:-1]),
        (struct.pack("<3I", 1, 3, 7), point_indices, visible_edges),
        (struct.pack("<3I", 0, 3, 6), point_indices, visible_edges),
    ):
        with pytest.raises(ValueError, match="offsets"):
            V8MeshFaceArray(*arguments)
    with pytest.raises(ValueError, match="one visible_edges flag"):
        V8MeshFaceArray.from_faces((V8MeshFace((0, 1, 2), (True, True)),))


def test_packed_and_row_shaped_core_data_decode_to_equal_elements() -> None:
    points = LINE_STRING_POINTS + LINE_STRING_POINTS[:1]
    header = {"kind": "MESH", "child_count": 4, "style": 1}
    packed = _element_data_from_core(
        {**header, "vertices": packed_points(points), "faces": packed_faces()}
    )
    rows = _element_data_from_core(
        {
            **header,
            "vertices": [(list(point.uor), list(point.master)) for point in points],
            "faces": [
                (list(face.point_indices), list(face.visible_edges)) for face in FACES
            ],
        }
    )

    assert isinstance(packed.vertices, V8PointArray)
    assert isinstance(packed.faces, V8MeshFaceArray)
    assert rows.vertices == points and isinstance(rows.vertices, tuple)
    assert rows.faces == FACES and isinstance(rows.faces, tuple)
    assert packed == rows
    assert rows == packed
    assert hash(packed) == hash(rows)

    packed_values = _element_data_from_core(
        {"kind": "MATRIX_INTEGER_DATA", "values": ("i", struct.pack("<3i", 1, -2, 0))}
    )
    row_values = _element_data_from_core(
        {"kind": "MATRIX_INTEGER_DATA", "values": [1, -2, 0]}
    )
    assert isinstance(packed_values.values, V8ValueArray)
    assert packed_values == row_values
    assert hash(packed_values) == hash(row_values)

    quadruples = POINT_STRING_ORIENTATIONS
    packed_orientations = _element_data_from_core(
        {
            "kind": "POINT_STRING",
            "orientations": b"".join(struct.pack("<4d", *item) for item in quadruples),
        }
    )
    row_orientations = _element_data_from_core(
        {"kind": "POINT_STRING", "orientations": [list(item) for item in quadruples]}
    )
    assert isinstance(packed_orientations.orientations, V8ValueArray)
    assert packed_orientations == row_orientations
    assert hash(packed_orientations) == hash(row_orientations)


def test_hand_built_element_data_with_tuples_equals_file_data() -> None:
    document = ezdgn.read_v8(V8)
    for element in document.models[0].elements:
        data = element.data
        by_hand = ezdgn.V8ElementData(
            **{
                name: getattr(data, name)
                for name in data.__dataclass_fields__
                if name not in {"vertices", "orientations", "faces", "values"}
            },
            vertices=tuple(data.vertices),
            orientations=tuple(data.orientations),
            faces=tuple(data.faces),
            values=tuple(data.values),
        )
        assert isinstance(by_hand.vertices, tuple)
        assert by_hand == data
        assert data == by_hand
        assert hash(by_hand) == hash(data)
        assert replace(data, vertices=tuple(data.vertices)) == data


def test_packed_sequences_survive_pickling() -> None:
    document = ezdgn.read_v8(V8)
    faces = V8MeshFaceArray(*packed_faces())
    values = V8ValueArray.from_values("d", POINT_STRING_ORIENTATIONS, width=4)

    restored = pickle.loads(pickle.dumps(document))
    assert restored == document
    vertices = restored.models[0].elements[LINE_STRING_INDEX].data.vertices
    assert isinstance(vertices, V8PointArray)
    assert vertices == LINE_STRING_POINTS
    for packed in (faces, values):
        copy = pickle.loads(pickle.dumps(packed))
        assert type(copy) is type(packed)
        assert copy == packed


def test_cli_payload_writes_packed_sequences_as_json_arrays() -> None:
    from ezdgn.__main__ import _v8_entity_payload

    elements = ezdgn.read_v8(V8).models[0].elements
    payload = json.loads(json.dumps(_v8_entity_payload(elements[POINT_STRING_INDEX])))
    assert payload["orientations"] == [list(item) for item in POINT_STRING_ORIENTATIONS]
    assert payload["vertices_master"] == [[0.0, 1.0, 0.0], [3.0, 4.0, 0.0]]

    mesh = replace(
        elements[0],
        data=_element_data_from_core(
            {
                "kind": "MESH",
                "style": 1,
                "vertices": packed_points(LINE_STRING_POINTS + LINE_STRING_POINTS[:1]),
                "faces": packed_faces(),
            }
        ),
    )
    payload = json.loads(json.dumps(_v8_entity_payload(mesh)))
    assert payload["vertices_uor"][1] == [30_000.0, 40_000.0, 50_000.0]
    assert payload["faces"] == [
        {"point_indices": [0, 1, 2], "visible_edges": [True, True, False]},
        {"point_indices": [0, 3, 1, 2], "visible_edges": [True, False, True, True]},
    ]
