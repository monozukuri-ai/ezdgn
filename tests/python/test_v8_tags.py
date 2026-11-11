from __future__ import annotations

import os
import struct
from collections import Counter
from dataclasses import replace
from pathlib import Path

import pytest

import ezdgn
from ezdgn.v8 import (
    TAG_SET_ELEMENT_TYPE,
    V8_MAX_TAG_SET_BYTES,
    V8_TAG_SET_CODE,
    V8_TAG_VALUE_INTEGER,
    V8_TAG_VALUE_TEXT,
    decode_tag_set,
    tag_set_objects,
    tag_sets,
)


DATA = Path(__file__).parents[1] / "data" / "dgn"
V8 = DATA / "v8" / "test_dgnv8.dgn"
# Written by the test builder of this repository from made-up values; see
# docs/v8/PROVENANCE.md.
CONSTRUCTED_TAGS = DATA / "v8" / "constructed_tags.dgn"


def _definition(
    name: bytes,
    tag_id: int,
    prompt: bytes,
    value_type: int,
    unresolved: tuple[int, int],
    default: bytes,
) -> bytes:
    """Build one tag definition by hand.

    ``unresolved`` are the two words that the observed writers fill
    differently; ``default`` is the stored default with its closing byte.
    """

    return (
        name
        + b"\x00"
        + struct.pack("<H", tag_id)
        + prompt
        + b"\x00"
        + struct.pack("<HHH", value_type, *unresolved)
        + b"\x00"
        + default
    )


def _name_linkage(name: bytes) -> bytes:
    linkage = bytearray(b"\x00\x10\xd2\x56" + struct.pack("<II", 1, len(name)) + name)
    linkage += bytes(-len(linkage) % 4)
    linkage[0] = len(linkage) // 2 - 1
    return bytes(linkage)


def _primary(definitions: bytes, *, sizes: tuple[int, int] | None = None) -> bytes:
    first, second = sizes or (len(definitions), len(definitions))
    primary = bytearray(0x3C)
    primary[0x28:0x30] = V8_TAG_SET_CODE
    primary[0x34:0x3C] = struct.pack("<II", first, second)
    primary += definitions
    primary += bytes(len(primary) % 2)
    return bytes(primary)


def _named_object(
    element_id: int | None,
    primary: bytes,
    attributes: bytes = b"",
    *,
    element_type: int = TAG_SET_ELEMENT_TYPE,
) -> ezdgn.V8RawObject:
    type_and_flags = 0x3000_0000 | element_type
    raw = bytearray(primary + attributes)
    raw[0:12] = struct.pack("<III", type_and_flags, len(raw) // 2, len(primary) // 2)
    raw[0x10:0x18] = struct.pack("<Q", element_id or 0)
    return ezdgn.V8RawObject(
        index=0,
        page_index=0,
        family="named",
        stream_path="/Dgn^Nm/$1",
        inflated_offset=4,
        framing_prefix=0,
        type_and_flags=type_and_flags,
        element_type=element_type,
        role="header",
        words=len(raw) // 2,
        attribute_words=len(primary) // 2,
        level=0,
        element_id=element_id,
        model_id=0,
        raw_bytes=bytes(raw),
    )


# Two definitions as one observed writer stores them (text length and value
# type in the unresolved words, odd total padded with a zero byte) and two as
# the other does (default length and zero).
DEFINITIONS = (
    _definition(b"Variety", 1, b"Kind of pear", 1, (3, 1), b"Oak\x00")
    + _definition(b"Yield", 2, b"", 3, (4, 3), struct.pack("<i", -7))
    + _definition(b"Note", 3, b"Say why", 1, (1, 0), b"\x00")
    + _definition(b"Caf\xe9", 4, b"", 1, (5, 0), b"none\x00")
)
EXPECTED = ezdgn.V8TagSet(
    element_id=300,
    name="Grove",
    definitions=(
        ezdgn.V8TagDefinition("Variety", 1, "Kind of pear", V8_TAG_VALUE_TEXT, "Oak"),
        ezdgn.V8TagDefinition("Yield", 2, "", V8_TAG_VALUE_INTEGER, -7),
        ezdgn.V8TagDefinition("Note", 3, "Say why", V8_TAG_VALUE_TEXT, ""),
        ezdgn.V8TagDefinition("Café", 4, "", V8_TAG_VALUE_TEXT, "none"),
    ),
)


def test_tag_set_decodes_from_hand_made_bytes() -> None:
    assert len(DEFINITIONS) % 2 == 1
    item = _named_object(300, _primary(DEFINITIONS), _name_linkage(b"Grove"))
    assert item.primary_bytes[0x28:0x30] == b"teSt\x81\x0f\x00\x00"
    tag_set = decode_tag_set(item)
    assert tag_set == EXPECTED
    assert tag_set is not None
    assert tag_set.definition(2) == EXPECTED.definitions[1]
    assert tag_set.definition(5) is None
    assert (V8_TAG_VALUE_TEXT, V8_TAG_VALUE_INTEGER) == (1, 3)

    # no padding after an even length; no name linkage gives an empty name
    even = DEFINITIONS + _definition(b"X", 5, b"", 1, (0, 0), b"\x00")
    assert len(even) % 2 == 0
    unnamed = decode_tag_set(_named_object(301, _primary(even)))
    assert unnamed is not None
    assert (unnamed.element_id, unnamed.name, len(unnamed.definitions)) == (301, "", 5)


@pytest.mark.parametrize(
    "item",
    [
        # another object type, and an object without an element ID
        _named_object(1, _primary(DEFINITIONS), element_type=95),
        _named_object(None, _primary(DEFINITIONS)),
        # another code at 0x28
        _named_object(1, _primary(DEFINITIONS).replace(b"teSt", b"teSu")),
        # the two stored sizes differ; the size runs past the object
        _named_object(1, _primary(DEFINITIONS, sizes=(len(DEFINITIONS), 0))),
        _named_object(1, _primary(DEFINITIONS, sizes=(4096, 4096))),
        # bytes remain after the definitions; a padding byte that is not zero
        _named_object(1, _primary(DEFINITIONS) + b"\x00\x00"),
        _named_object(1, _primary(DEFINITIONS)[:-1] + b"\x01"),
        # a value type that is neither text nor integer
        _named_object(1, _primary(_definition(b"A", 1, b"", 5, (0, 0), b"\x00"))),
        # the byte before the default is not zero
        _named_object(
            1,
            _primary(
                b"A\x00"
                + struct.pack("<H", 1)
                + b"\x00"
                + struct.pack("<HHH", 1, 0, 0)
                + b"\x01\x00"
            ),
        ),
        # a text default without its closing byte; a short integer default
        _named_object(1, _primary(_definition(b"A", 1, b"", 1, (0, 0), b"abc"))),
        _named_object(1, _primary(_definition(b"A", 1, b"", 3, (0, 0), b"\x01\x00"))),
        # truncated inside the fixed head
        _named_object(1, _primary(DEFINITIONS)[:0x38]),
    ],
)
def test_tag_sets_that_do_not_match_the_layout_stay_raw(
    item: ezdgn.V8RawObject,
) -> None:
    assert decode_tag_set(item) is None


def _tag_fields(data: ezdgn.V8ElementData) -> tuple[object, ...]:
    assert isinstance(data, ezdgn.V8TagData)
    return (
        data.kind,
        data.tag_id,
        data.value_type,
        data.value_bytes,
        data.value,
        data.tag_set_element_id,
        data.host_element_id,
        data.origin,
    )


def test_tags_of_a_constructed_file_reach_python_with_every_field() -> None:
    # This test runs without the public file: it covers the way of a tag
    # from the Rust reader to the Python objects in every test run.
    document = ezdgn.read_v8(CONSTRUCTED_TAGS)
    (model,) = document.models
    assert [element.kind for element in model.elements] == [
        "SHARED_CELL_INSTANCE",
        "TAG",
        "TAG",
        "TAG",
        "UNKNOWN",
        "UNKNOWN_COMPLEX",
        "UNKNOWN",
    ]
    host, attached, free, other_type = model.elements[:4]
    # a text tag on a host: both IDs, and the origin inside the stored range
    assert _tag_fields(attached.data) == (
        "TAG",
        1,
        V8_TAG_VALUE_TEXT,
        b"Oak\x00",
        "Oak",
        300,
        800,
        ezdgn.V8Point(uor=(960.0, -100.0, 0.0), master=(0.96, -0.1, 0.0)),
    )
    assert host.common.element_id == attached.data.host_element_id
    # an integer tag with no host and with a point outside its range
    assert _tag_fields(free.data) == (
        "TAG",
        2,
        V8_TAG_VALUE_INTEGER,
        bytes([42, 0, 0, 0]),
        42,
        300,
        None,
        None,
    )
    # a value type that is not decoded keeps its bytes
    assert _tag_fields(other_type.data) == (
        "TAG",
        1,
        5,
        bytes([1, 2, 3]),
        None,
        None,
        None,
        ezdgn.V8Point(uor=(0.0, 0.0, 0.0), master=(0.0, 0.0, 0.0)),
    )
    for element in model.elements:
        assert isinstance(element.data, ezdgn.V8TagData) == (element.kind == "TAG")
        assert not (element.kind == "TAG" and element.is_drawable)

    # the tag set of the file, and the join that the reader leaves to the caller
    sets = document.tag_sets_by_id()
    assert list(sets) == [300]
    grove = sets[300]
    assert grove.name == "Grove"
    assert [(item.tag_id, item.name) for item in grove.definitions] == [
        (1, "Variety"),
        (2, "Yield"),
    ]
    assert document.tag_set(free.data.tag_set_element_id) == grove
    assert grove.definition(free.data.tag_id).value_type == free.data.value_type

    # a tag that is a component of a shared-cell definition
    (definition,) = ezdgn.read_v8_shared_cell_definitions(CONSTRUCTED_TAGS)
    inner = definition.elements[2]
    assert (inner.kind, inner.parent_index) == ("TAG", 0)
    assert _tag_fields(inner.data) == (
        "TAG",
        1,
        V8_TAG_VALUE_TEXT,
        b"Elm\x00",
        "Elm",
        300,
        None,
        ezdgn.V8Point(uor=(300.0, 100.0, 0.0), master=(0.3, 0.1, 0.0)),
    )


def test_tag_sets_of_a_document_are_decoded_on_request() -> None:
    document = ezdgn.read_v8(V8)
    # the checked-in fixture holds no tag set
    assert tag_set_objects(document.raw) == ()
    assert document.tag_sets == ()
    assert all(
        element.kind != "TAG" and not isinstance(element.data, ezdgn.V8TagData)
        for model in document.models
        for element in model.elements
    )

    good = _named_object(300, _primary(DEFINITIONS), _name_linkage(b"Grove"))
    undecodable = _named_object(
        301, _primary(_definition(b"A", 1, b"", 5, (0, 0), b"\x00"))
    )
    # the bytes of a tag set under another code, and under another type
    other_code = _named_object(302, _primary(DEFINITIONS).replace(b"teSt", b"teSu"))
    other_type = _named_object(303, _primary(DEFINITIONS), element_type=95)
    page = document.raw.named_pages[0]
    raw = replace(
        document.raw,
        named_pages=(
            replace(
                page,
                objects=(*page.objects, other_code, good, other_type, undecodable),
            ),
        ),
    )
    assert tag_set_objects(raw) == (good, undecodable)
    assert tag_sets(raw) == (EXPECTED,)
    with_sets = replace(document, raw=raw)
    assert with_sets.tag_sets == (EXPECTED,)
    assert with_sets.tag_set(300) == EXPECTED
    assert with_sets.tag_set(301) is None
    assert with_sets.tag_set(None) is None


def test_one_tag_set_lookup_decodes_at_most_one_set(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    document = ezdgn.read_v8(V8)
    sets = tuple(
        _named_object(300 + index, _primary(DEFINITIONS), _name_linkage(b"Grove"))
        for index in range(5)
    )
    page = document.raw.named_pages[0]
    with_sets = replace(
        document,
        raw=replace(document.raw, named_pages=(replace(page, objects=sets),)),
    )
    decoded: list[int | None] = []
    real = ezdgn.v8.decode_tag_set

    def counting(item: ezdgn.V8RawObject) -> ezdgn.V8TagSet | None:
        decoded.append(item.element_id)
        return real(item)

    monkeypatch.setattr(ezdgn.v8, "decode_tag_set", counting)
    # one lookup decodes the one set it returns and nothing else
    assert with_sets.tag_set(303) == replace(EXPECTED, element_id=303)
    assert decoded == [303]
    # an ID that no tag set has, and a tag without a tag set: nothing decoded
    decoded.clear()
    assert with_sets.tag_set(999) is None
    assert with_sets.tag_set(None) is None
    assert decoded == []

    # the mapping for the tags of a whole model: every set, decoded once
    by_id = with_sets.tag_sets_by_id()
    assert sorted(by_id) == [300, 301, 302, 303, 304]
    assert by_id[302] == replace(EXPECTED, element_id=302)
    assert by_id.get(999) is None


def test_one_lookup_and_the_mapping_give_the_same_tag_set() -> None:
    document = ezdgn.read_v8(V8)
    page = document.raw.named_pages[0]

    def with_objects(*objects: ezdgn.V8RawObject) -> ezdgn.V8Document:
        raw = replace(document.raw, named_pages=(replace(page, objects=objects),))
        return replace(document, raw=raw)

    good = _named_object(300, _primary(DEFINITIONS), _name_linkage(b"Grove"))
    other = _named_object(300, _primary(DEFINITIONS), _name_linkage(b"Hedge"))
    damaged = _named_object(300, _primary(_definition(b"A", 1, b"", 5, (0, 0), b"\x00")))
    assert decode_tag_set(damaged) is None

    # Two objects with one ID: both ways give the first one.
    twice = with_objects(good, other)
    assert twice.tag_set(300) == EXPECTED
    assert twice.tag_sets_by_id() == {300: EXPECTED}
    # The first object with the ID does not decode: both ways give the
    # first one that does.
    after_damage = with_objects(damaged, other, good)
    hedge = replace(EXPECTED, name="Hedge")
    assert after_damage.tag_set(300) == hedge
    assert after_damage.tag_sets_by_id() == {300: hedge}
    # No object with the ID decodes: both ways give nothing.
    only_damage = with_objects(damaged, damaged)
    assert only_damage.tag_set(300) is None
    assert only_damage.tag_sets_by_id() == {}

    # The one stated difference: the mapping has the limit of one call for
    # all sets together. The third set of 400,000 bytes is over that sum, so
    # the mapping leaves it out, and one lookup still returns it.
    filled = _definition(b"A", 1, b"", 1, (0, 0), b"x" * (400_000 - 13) + b"\x00")
    big = with_objects(*(_named_object(300 + index, _primary(filled)) for index in range(3)))
    by_id = big.tag_sets_by_id()
    assert sorted(by_id) == [300, 301]
    assert all(big.tag_set(element_id) == by_id[element_id] for element_id in by_id)
    last = big.tag_set(302)
    assert last is not None and last.element_id == 302


def test_one_lookup_decodes_every_object_with_the_id_until_one_decodes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # The stated cost of tag_set(): K objects of one ID that fail at their
    # end are K decodes. Each object here holds 100,000 valid bytes and then
    # one definition of a value type that the reader does not decode. All
    # objects together stay under the limit of one call for all sets.
    filled = _definition(b"A", 1, b"", 1, (0, 0), b"x" * (100_000 - 13) + b"\x00")
    unread = _definition(b"B", 2, b"", 5, (0, 0), b"\x00")
    late = _named_object(300, _primary(filled + unread))
    assert decode_tag_set(late) is None
    assert decode_tag_set(_named_object(300, _primary(filled))) is not None
    good = _named_object(300, _primary(DEFINITIONS), _name_linkage(b"Grove"))
    other = _named_object(301, _primary(filled + unread))
    document = ezdgn.read_v8(V8)
    page = document.raw.named_pages[0]
    objects = (late,) * 6 + (other, good, late, good)
    raw = replace(document.raw, named_pages=(replace(page, objects=objects),))
    with_sets = replace(document, raw=raw)

    decoded: list[int | None] = []
    real = ezdgn.v8.decode_tag_set

    def counting(item: ezdgn.V8RawObject) -> ezdgn.V8TagSet | None:
        decoded.append(item.element_id)
        return real(item)

    monkeypatch.setattr(ezdgn.v8, "decode_tag_set", counting)
    # six objects fail late, the seventh with the ID decodes: seven decodes,
    # none for another ID and none after the first that decodes
    assert with_sets.tag_set(300) == EXPECTED
    assert decoded == [300] * 7
    # the mapping gives the same set
    assert with_sets.tag_sets_by_id() == {300: EXPECTED}


def test_a_tag_set_code_outside_the_primary_bytes_makes_no_tag_set() -> None:
    document = ezdgn.read_v8(V8)
    page = document.raw.named_pages[0]
    good = _named_object(300, _primary(DEFINITIONS), _name_linkage(b"Grove"))
    # The eight code bytes at 0x28, but the primary part ends before them:
    # they are attribute bytes of this object, so it is no tag set.
    short = replace(good, attribute_words=0x20 // 2)
    assert short.raw_bytes[0x28:0x30] == V8_TAG_SET_CODE
    assert short.primary_bytes[0x28:0x30] == b""
    # The primary part ends in the middle of the code.
    cut = replace(good, attribute_words=0x2C // 2)
    raw = replace(document.raw, named_pages=(replace(page, objects=(short, cut, good)),))
    assert tag_set_objects(raw) == (good,)
    assert replace(document, raw=raw).tag_set(300) == EXPECTED


def test_tag_set_decoding_is_bounded() -> None:
    assert V8_MAX_TAG_SET_BYTES == 1_048_576

    def filled(length: int) -> bytes:
        # one definition whose default text fills the set up to a length
        return _definition(b"A", 1, b"", 1, (0, 0), b"x" * (length - 13) + b"\x00")

    # one tag set holds at most the limit
    at_limit = filled(1_048_576)
    assert len(at_limit) == 1_048_576
    assert decode_tag_set(_named_object(1, _primary(at_limit))) is not None
    assert decode_tag_set(_named_object(1, _primary(filled(1_048_578)))) is None
    # The smallest definition is 12 bytes. A hostile set of them cannot ask
    # for more definitions than the limit has room for.
    smallest = _definition(b"", 0, b"", 1, (0, 0), b"\x00")
    assert len(smallest) == 12
    most = decode_tag_set(_named_object(1, _primary(smallest * 87_381)))
    assert most is not None and len(most.definitions) == 87_381
    assert decode_tag_set(_named_object(1, _primary(smallest * 87_382))) is None

    # one call for a document: the third set of 400,000 bytes would pass the
    # limit, so it is not decoded; it decodes when it is asked for alone
    big = [_named_object(300 + index, _primary(filled(400_000))) for index in range(3)]
    document = ezdgn.read_v8(V8)
    page = document.raw.named_pages[0]
    raw = replace(document.raw, named_pages=(replace(page, objects=tuple(big)),))
    assert tag_set_objects(raw) == tuple(big)
    assert [item.element_id for item in tag_sets(raw)] == [300, 301]
    last = decode_tag_set(big[2])
    assert last is not None and last.element_id == 302

    # An object that declares more than the limit cannot decode. It is
    # passed over: it does not end the call for the sets after it.
    hostile = _named_object(299, _primary(DEFINITIONS, sizes=(1 << 21, 1 << 21)))
    raw = replace(
        document.raw, named_pages=(replace(page, objects=(hostile, big[0])),)
    )
    assert tag_set_objects(raw) == (hostile, big[0])
    assert [item.element_id for item in tag_sets(raw)] == [300]


def test_the_cli_payload_reads_the_fields_of_an_element_strictly() -> None:
    from ezdgn.__main__ import _v8_entity_payload

    class WithoutStyle:
        """Element data that has lost one of its base fields."""

        def __init__(self, data: ezdgn.V8ElementData) -> None:
            self._data = data

        def __getattr__(self, name: str) -> object:
            if name == "style":
                raise AttributeError(name)
            return getattr(self._data, name)

    element = ezdgn.read_v8(V8).models[0].elements[0]
    payload = _v8_entity_payload(element)
    # an element that is no tag gets no tag key, and is not asked for one
    assert not {"tag_id", "value_type", "tag_set_element_id"} & set(payload)
    # a base field that is missing is an error and is not left out in silence
    broken = replace(element, data=WithoutStyle(element.data))  # type: ignore[arg-type]
    with pytest.raises(AttributeError, match="style"):
        _v8_entity_payload(broken)


def test_a_tag_is_not_drawable_and_keeps_its_fields_in_the_cli_payload() -> None:
    import json

    from ezdgn.__main__ import _v8_entity_payload

    document = ezdgn.read_v8(V8)
    model = document.models[0]
    tag = replace(
        model.elements[0],
        index=0,
        parent_index=None,
        child_indices=(),
        data=ezdgn.V8TagData(
            kind="TAG",
            tag_id=1,
            value_type=V8_TAG_VALUE_TEXT,
            value_bytes=b"Oak\x00",
            value="Oak",
            tag_set_element_id=300,
            host_element_id=800,
            origin=ezdgn.V8Point(uor=(9050.0, -100.0, 0.0), master=(9.05, -0.1, 0.0)),
        ),
    )
    assert not tag.is_drawable
    tagged = replace(model, elements=(tag,))
    assert tagged.entities == ()
    assert tagged.unknown_elements == ()
    assert json.loads(json.dumps(_v8_entity_payload(tag))) == {
        "kind": "TAG",
        "origin_uor": [9050.0, -100.0, 0.0],
        "origin_master": [9.05, -0.1, 0.0],
        "tag_id": 1,
        "value_type": 1,
        "value": "Oak",
        "tag_set_element_id": 300,
        "host_element_id": 800,
        "value_hex": "4f616b00",
    }


# The checks below read a public file that the repository does not
# redistribute: the "Tree text-node labels with tags" support example recorded
# in docs/v8/PROVENANCE.md. Set EZDGN_PUBLIC_V8_DIR to the folder that holds it.
PUBLIC_DIR = os.environ.get("EZDGN_PUBLIC_V8_DIR")
TREE_TAGS = "TreeTextNodeLabelsWithTags.dgn"
TREE_TAGS_SHA256 = "8439e6d3a18227122c10c5367799561ebe94737d9d27932f4eb89d743b108270"


@pytest.mark.skipif(
    not PUBLIC_DIR or not (Path(PUBLIC_DIR or ".") / TREE_TAGS).is_file(),
    reason="set EZDGN_PUBLIC_V8_DIR to a folder with the public tree tags file",
)
def test_public_tree_tags() -> None:
    import hashlib

    path = Path(PUBLIC_DIR or ".") / TREE_TAGS
    assert hashlib.sha256(path.read_bytes()).hexdigest() == TREE_TAGS_SHA256
    document = ezdgn.read_v8(path)
    (model,) = document.models
    assert Counter(element.kind for element in model.elements) == {
        "TEXT_NODE": 6_904,
        "TEXT": 12_623,
        "TAG": 13_808,
    }
    # the feature view is unchanged: a tag is not an entity
    assert len(model.entities) == 12_623
    assert model.unknown_elements == ()

    (tag_set,) = document.tag_sets
    assert tag_set_objects(document.raw) == (
        next(
            item
            for page in document.raw.named_pages
            for item in page.objects
            if item.element_id == tag_set.element_id
        ),
    )
    assert tag_set == ezdgn.V8TagSet(
        element_id=110,
        name="Trees",
        definitions=(
            ezdgn.V8TagDefinition("Species", 1, "Enter value", 1, "Default Value"),
            ezdgn.V8TagDefinition("Tree Count", 2, "Enter value", 3, 0),
        ),
    )

    tags = [element for element in model.elements if element.kind == "TAG"]
    assert all(element.is_top_level and not element.is_drawable for element in tags)
    # only a tag carries the tag fields
    assert all(
        isinstance(element.data, ezdgn.V8TagData) == (element.kind == "TAG")
        for element in model.elements
    )
    assert Counter(
        (element.data.tag_id, element.data.value_type, type(element.data.value))
        for element in tags
    ) == {(1, 1, str): 6_904, (2, 3, int): 6_904}
    # every tag names the one set, and its ID is defined there with its type
    assert {element.data.tag_set_element_id for element in tags} == {110}
    assert all(
        tag_set.definition(element.data.tag_id).value_type == element.data.value_type
        for element in tags
    )
    first, second = tags[:2]
    assert (first.data.value, first.data.value_bytes) == (
        "Euchlora   X",
        b"Euchlora   X\x00",
    )
    assert (second.data.value, second.data.value_bytes) == (1, b"\x01\x00\x00\x00")
    texts = Counter(
        element.data.value for element in tags if element.data.value_type == 1
    )
    assert len(texts) == 903
    assert texts.most_common(3) == [
        ("Platanoides", 803),
        ("Serrulata", 662),
        ("Rubrum", 400),
    ]
    counts = [element.data.value for element in tags if element.data.value_type == 3]
    assert (min(counts), max(counts)) == (1, 58)

    # each text node hosts one tag of each definition
    by_id = {element.common.element_id: element for element in model.elements}
    hosts = [by_id[element.data.host_element_id] for element in tags]
    assert {host.kind for host in hosts} == {"TEXT_NODE"}
    assert Counter(Counter(host.index for host in hosts).values()) == {2: 6_904}
    # The first stored pair is the origin of the host. The second pair is a
    # fixed step for each definition. The stored range of every tag is the
    # single point (0, 0), so no origin is given.
    assert all(
        struct.unpack_from("<2d", element.raw.primary_bytes, 0xA0)
        == host.data.origin.uor[:2]
        for element, host in zip(tags, hosts, strict=True)
    )
    assert Counter(
        (
            element.data.tag_id,
            struct.unpack_from("<2d", element.raw.primary_bytes, 0xB8),
        )
        for element in tags
    ) == {(1, (0.0, -10.0)): 6_904, (2, (0.0, -20.0)): 6_904}
    assert {element.common.range_uor for element in tags} == {((0, 0, 0), (0, 0, 0))}
    assert all(element.data.origin is None for element in tags)
