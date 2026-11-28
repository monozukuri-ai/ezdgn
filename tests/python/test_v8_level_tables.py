from __future__ import annotations

import struct
from dataclasses import replace
from pathlib import Path

import ezdgn
from ezdgn.v8 import (
    LEVEL_TABLE_ELEMENT_TYPE,
    LEVEL_TABLE_HEADER_ELEMENT_TYPE,
    LEVEL_TABLE_KIND,
    NAME_LINKAGE_SIGNATURE,
    V8ContainerInfo,
    V8ObjectPage,
    V8PageHeader,
    V8RawDocument,
    V8RawObject,
    level_table,
    level_tables,
)


V8 = Path(__file__).parents[1] / "data" / "dgn" / "v8" / "test_dgnv8.dgn"
OTHER_TABLE_KIND = 4
CELL_HEADER_TYPE = 34
ENTRY_PRIMARY_BYTES = 0x68
TABLE_HEADER_BYTES = 0x40
DEFAULT = (64, 0, "Default")
TRACK = (70, 6, "Track")
CABLE = (70, 12, "Cable")
LIGHTING = (71, 7, "Lighting")


def name_linkage(name: str) -> bytes:
    payload = b"\xff\xfd" + name.encode("utf-16-le")
    body = struct.pack("<II", 1, len(payload)) + payload
    body += bytes(len(body) % 2)
    words_to_follow = (4 + len(body)) // 2 - 1
    return struct.pack("<HH", 0x1000 | words_to_follow, NAME_LINKAGE_SIGNATURE) + body


def named_object(
    element_type: int,
    level: int,
    role: str,
    primary: bytes = bytes(TABLE_HEADER_BYTES),
    attributes: bytes = b"",
) -> V8RawObject:
    return V8RawObject(
        index=0,
        page_index=0,
        family="named",
        stream_path="/Dgn^Nm/$1",
        inflated_offset=0,
        framing_prefix=0,
        type_and_flags=element_type,
        element_type=element_type,
        role=role,  # type: ignore[arg-type]
        words=(len(primary) + len(attributes)) // 2,
        attribute_words=len(primary) // 2,
        level=level,
        element_id=None,
        model_id=None,
        raw_bytes=primary + attributes,
    )


def table_header(kind: int = LEVEL_TABLE_KIND, role: str = "header") -> V8RawObject:
    return named_object(LEVEL_TABLE_HEADER_ELEMENT_TYPE, kind, role)


def entry(
    level: tuple[int, int, str],
    kind: int = LEVEL_TABLE_KIND,
    primary_bytes: int = ENTRY_PRIMARY_BYTES,
) -> V8RawObject:
    level_id, number, name = level
    primary = bytearray(ENTRY_PRIMARY_BYTES)
    struct.pack_into("<I", primary, 0x20, level_id)
    struct.pack_into("<I", primary, 0x5C, number)
    return named_object(
        LEVEL_TABLE_ELEMENT_TYPE,
        kind,
        "component",
        bytes(primary[:primary_bytes]),
        name_linkage(name),
    )


def raw_document(*pages: list[V8RawObject]) -> V8RawDocument:
    index = 0
    named_pages = []
    for number, objects in enumerate(pages, start=1):
        numbered = []
        for item in objects:
            numbered.append(replace(item, index=index, page_index=len(numbered)))
            index += 1
        named_pages.append(
            V8ObjectPage(
                stream_path=f"/Dgn^Nm/${number}",
                family="named",
                header=V8PageHeader(len(numbered), 2, number, len(numbered)),
                inflated_size=sum(item.size_bytes for item in numbered),
                objects=tuple(numbered),
            )
        )
    return V8RawDocument(
        container=V8ContainerInfo(4, True, (), (), ()),
        models=(),
        named_pages=tuple(named_pages),
        named_auxiliary_pages=(),
        total_inflated_bytes=0,
        graphical_object_count=0,
        total_object_count=index,
    )


def rows(raw: V8RawDocument) -> list[list[tuple[int, int, str]]]:
    return [
        [(level.level_id, level.number, level.name) for level in table.levels]
        for table in level_tables(raw)
    ]


def test_the_fixture_has_one_level_table_under_its_header() -> None:
    document = ezdgn.read_v8(V8)

    (table,) = document.level_tables
    assert table.header is not None
    assert table.header.element_type == LEVEL_TABLE_HEADER_ELEMENT_TYPE
    assert table.header.level == LEVEL_TABLE_KIND
    assert table.header.element_id == 1
    assert table.levels == document.levels
    assert table.level(64) == document.level(64)
    assert table.level(65) is None
    assert document.level_tables == level_tables(document.raw)
    assert document.level_candidates(64) == (document.levels[0],)
    assert document.level_candidates(65) == ()
    assert isinstance(table, ezdgn.V8LevelTable)


def test_groups_level_entries_under_their_table_headers() -> None:
    raw = raw_document(
        [
            table_header(),
            entry(DEFAULT),
            entry(TRACK),
            table_header(OTHER_TABLE_KIND),
            table_header(),
            entry(DEFAULT),
            entry(CABLE),
            table_header(),
            entry(LIGHTING),
        ]
    )

    assert rows(raw) == [[DEFAULT, TRACK], [DEFAULT, CABLE], [LIGHTING]]
    headers = [table.header for table in level_tables(raw)]
    assert [header.index for header in headers if header] == [0, 4, 7]


def test_entries_without_a_level_table_header_form_a_table_of_their_own() -> None:
    headerless = raw_document([entry(DEFAULT), entry(TRACK)])
    assert rows(headerless) == [[DEFAULT, TRACK]]
    assert level_tables(headerless)[0].header is None

    after_another_table = raw_document(
        [table_header(), entry(DEFAULT), table_header(OTHER_TABLE_KIND), entry(CABLE)]
    )
    assert rows(after_another_table) == [[DEFAULT], [CABLE]]
    first, second = level_tables(after_another_table)
    assert first.header is not None and second.header is None


def test_a_table_continues_across_pages_and_other_components() -> None:
    other_kind_entry = entry(LIGHTING, kind=2)
    nested_header = table_header(OTHER_TABLE_KIND, role="header_component")
    raw = raw_document(
        [table_header(), entry(DEFAULT), other_kind_entry, nested_header],
        [entry(TRACK), entry(LIGHTING)],
    )

    assert rows(raw) == [[DEFAULT, TRACK, LIGHTING]]
    assert len(level_table(raw)) == 3


def test_an_object_that_is_not_a_component_ends_the_table() -> None:
    cell = named_object(CELL_HEADER_TYPE, 64, "header")
    raw = raw_document([table_header(), entry(DEFAULT), cell, entry(TRACK)])

    assert rows(raw) == [[DEFAULT], [TRACK]]
    assert level_tables(raw)[1].header is None


def test_a_header_without_entries_is_an_empty_table() -> None:
    truncated = entry(TRACK, primary_bytes=0x5C)
    raw = raw_document(
        [truncated, table_header(), table_header(), truncated, entry(DEFAULT)]
    )

    assert rows(raw) == [[], [DEFAULT]]
    assert [table.header.index for table in level_tables(raw) if table.header] == [1, 2]
    assert level_table(raw) == level_tables(raw)[1].levels


def test_candidates_list_the_entry_of_each_table_that_defines_the_id() -> None:
    raw = raw_document(
        [table_header(), entry(DEFAULT), entry(TRACK), entry((70, 9, "Duplicate"))],
        [table_header(OTHER_TABLE_KIND), table_header(), entry(DEFAULT), entry(CABLE)],
        [table_header(), entry(LIGHTING)],
    )
    document = ezdgn.V8Document(raw=raw, models=())

    def candidates(level_id: int) -> list[tuple[int, str]]:
        found = document.level_candidates(level_id)
        return [(item.number, item.name) for item in found]

    assert candidates(64) == [(0, "Default"), (0, "Default")]
    assert candidates(70) == [(6, "Track"), (12, "Cable")]
    assert candidates(71) == [(7, "Lighting")]
    assert candidates(72) == []
    own, attached, _ = document.level_tables
    assert own.level(70).name == "Track"
    assert attached.level(70).name == "Cable"
    assert attached.level(71) is None


def test_the_flat_level_list_and_lookup_keep_every_entry_in_file_order() -> None:
    raw = raw_document(
        [table_header(), entry(DEFAULT), entry(TRACK)],
        [table_header(), entry(DEFAULT), entry(CABLE), entry(LIGHTING)],
    )
    document = ezdgn.V8Document(raw=raw, models=())

    assert [(item.level_id, item.name) for item in document.levels] == [
        (64, "Default"),
        (70, "Track"),
        (64, "Default"),
        (70, "Cable"),
        (71, "Lighting"),
    ]
    assert document.levels == level_table(raw)
    assert document.levels == tuple(
        level for table in document.level_tables for level in table.levels
    )
    assert document.level(70).name == "Track"
    assert document.level(71).name == "Lighting"
    assert document.level(72) is None
