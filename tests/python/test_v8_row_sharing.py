from __future__ import annotations

from pathlib import Path

import ezdgn


V8 = Path(__file__).parents[1] / "data" / "dgn" / "v8" / "test_dgnv8.dgn"


def keys_are_shared(one: dict[str, object], other: dict[str, object]) -> bool:
    return len(one) == len(other) and all(a is b for a, b in zip(one, other))


def stored_key(row: dict[str, object], name: str) -> str:
    return next(key for key in row if key == name)


def test_core_rows_share_their_keys_and_repeated_strings() -> None:
    rows = ezdgn._core.read_v8_document(
        V8.read_bytes(), ezdgn.V8ScanLimits()._native_row()
    )

    page = rows["raw"]["models"][0]["graphical_pages"][0]
    first_object, second_object = page["objects"][:2]
    assert keys_are_shared(first_object, second_object)
    assert first_object["stream_path"] is page["stream_path"]
    assert second_object["stream_path"] is page["stream_path"]
    assert first_object["family"] is second_object["family"] is page["family"]
    assert first_object["role"] is second_object["role"]

    first_line, second_line = rows["models"][0]["elements"][:2]
    assert first_line["data"]["kind"] == second_line["data"]["kind"] == "LINE"
    assert keys_are_shared(first_line, second_line)
    assert keys_are_shared(first_line["common"], second_line["common"])
    assert keys_are_shared(first_line["data"], second_line["data"])
    assert first_line["data"]["kind"] is second_line["data"]["kind"]

    (auxiliary_page,) = rows["raw"]["named_auxiliary_pages"]
    (record,) = auxiliary_page["records"]
    assert record["stream_path"] is auxiliary_page["stream_path"]
    assert stored_key(record, "stream_path") is stored_key(page, "stream_path")
    assert stored_key(record, "index") is stored_key(first_object, "index")


def test_decoded_objects_of_a_page_share_their_strings() -> None:
    document = ezdgn.read_v8(V8)
    model = document.models[0]
    page = document.raw.models[0].graphical_pages[0]
    first, second = page.objects[:2]

    assert first.stream_path == "/Dgn-Md/#000000/Dgn^G/$1"
    assert first.stream_path is second.stream_path is page.stream_path
    assert first.family is second.family
    assert first.role is second.role
    assert model.elements[0].kind is model.elements[1].kind


def test_definition_components_share_their_named_page_path() -> None:
    (definition,) = ezdgn.read_v8_shared_cell_definitions(V8)
    header, *components = definition.elements
    assert components
    for component in components:
        assert component.raw.stream_path is header.raw.stream_path
