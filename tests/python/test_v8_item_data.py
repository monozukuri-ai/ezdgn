from __future__ import annotations

import dataclasses
import gc
import json
import os
import struct
import tracemalloc
from collections import Counter
from pathlib import Path

import pytest

import ezdgn
from ezdgn.v8 import (
    V8_ITEM_INSTANCE_ENTRY_TYPE,
    V8_MAX_AUXILIARY_ENTRIES,
    V8_MAX_ITEM_BYTES,
    decode_auxiliary_entries,
    decode_item_instance,
    decode_item_instances,
)


DATA = Path(__file__).parents[1] / "data" / "dgn"
V8 = DATA / "v8" / "test_dgnv8.dgn"

HEAD = bytes.fromhex("0000010115")
SIGNATURE = b"\x01BECXML\x00\xff\x0a\x0d\x01\x00\x00\x03\x00\x00\x30"
ROOT, CHILD, END, ATTRIBUTE, CONTENT, VALUE = 0x03, 0x02, 0x04, 0x05, 0x06, 0x11
ATTRIBUTES = (("instanceId", ""), ("xmlns", "Sample__x0020__Library.01.00"))


def _table_string(text: str) -> bytes:
    if all(ord(character) < 0x100 for character in text):
        return bytes([0xFA, len(text)]) + text.encode("latin-1")
    encoded = text.encode("utf-16-le")
    return bytes([0xFB, len(encoded) // 2]) + encoded


def _item_data(
    schema_element_id: int,
    class_name: str,
    attributes: tuple[tuple[str, str], ...],
    properties: tuple[tuple[str, str], ...],
) -> bytes:
    """Build one instance by hand: equal strings share one table entry."""

    strings: list[str] = []

    def number(text: str) -> int:
        if text not in strings:
            strings.append(text)
        return strings.index(text)

    tokens = [ROOT, number(class_name)]
    for name, value in attributes:
        tokens += [ATTRIBUTE, number(name), VALUE, number(value)]
    tokens.append(CONTENT)
    for name, value in properties:
        tokens += [CHILD, number(name), VALUE, number(value), END]
    tokens.append(END)
    table = b"".join(_table_string(text) for text in strings)
    return (
        HEAD
        + struct.pack("<I", schema_element_id)
        + SIGNATURE
        + bytes([len(strings)])
        + table
        + bytes(tokens)
    )


def _entry(type_id: int, entry_id: int, data: bytes, *, flag: bool = True) -> bytes:
    size = len(data) | (0x8000_0000 if flag else 0)
    return struct.pack("<IIII", type_id, entry_id, size, 0) + data


def _payload(*entries: bytes) -> bytes:
    return b"".join(entries) + b"\x01\x00\x00\x00"


def test_auxiliary_payload_splits_into_entries() -> None:
    item = _item_data(900, "Pump", ATTRIBUTES, (("Tag", "P-101"),))
    payload = _payload(
        _entry(0x5709_0000, 0, b"\x03\x00\x00\x00", flag=False),
        _entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 2, item),
    )
    entries = decode_auxiliary_entries(payload, 2)
    assert entries is not None
    assert [(entry.index, entry.offset) for entry in entries] == [(0, 0), (1, 20)]
    first, second = entries
    assert (first.type_id, first.type_major, first.type_minor) == (0x5709_0000, 0x5709, 0)
    assert (first.entry_id, first.size_word, first.size_flag) == (0, 4, False)
    assert first.data == b"\x03\x00\x00\x00"
    assert (second.type_id, second.entry_id, second.size_flag) == (0x56FF_0000, 2, True)
    assert second.size_word == 0x8000_0000 | len(item)
    assert second.reserved == 0
    assert second.data == item
    assert decode_auxiliary_entries(b"\x01\x00\x00\x00", 0) == ()


def test_a_payload_with_more_entries_than_the_limit_stays_raw() -> None:
    assert V8_MAX_AUXILIARY_ENTRIES == 65_536
    empty = _entry(0x5709_0000, 0, b"", flag=False)
    at_limit = decode_auxiliary_entries(
        _payload(empty * V8_MAX_AUXILIARY_ENTRIES), V8_MAX_AUXILIARY_ENTRIES
    )
    assert at_limit is not None and len(at_limit) == V8_MAX_AUXILIARY_ENTRIES
    over = V8_MAX_AUXILIARY_ENTRIES + 1
    assert decode_auxiliary_entries(_payload(empty * over), over) is None


@pytest.mark.parametrize(
    ("payload", "count"),
    [
        (b"", 0),
        (b"", 1),
        (_payload(_entry(1, 1, b"abcd")), 0),
        (_payload(_entry(1, 1, b"abcd")), 2),
        (_payload(_entry(1, 1, b"abcd")), 0xFFFF_FFFF),
        (_payload(_entry(1, 1, b"abcd")), -1),
        (_payload(_entry(1, 1, b"abcd")), 1 << 32),
        (_entry(1, 1, b"abcd"), 1),
        (_payload(_entry(1, 1, b"abcd")) + b"\x00", 1),
        (_entry(1, 1, b"abcd") + b"\x02\x00\x00\x00", 1),
        (struct.pack("<IIII", 1, 1, 0x7FFF_FFFF, 0) + b"abcd\x01\x00\x00\x00", 1),
    ],
)
def test_auxiliary_payloads_that_do_not_match_the_layout_stay_raw(
    payload: bytes, count: int
) -> None:
    assert decode_auxiliary_entries(payload, count) is None


def test_item_instance_exposes_class_attributes_and_properties() -> None:
    properties = (
        ("Tag", "P-101"),
        ("Duty", ""),
        ("Standby", "P-101"),
        ("Bore", "1½\""),
        ("Wide", "2ƒ\" \U0001f600"),
    )
    data = _item_data(0x0001_02B3, "Pump", ATTRIBUTES, properties)
    # one-byte strings store a code point per byte; wide strings are UTF-16LE
    assert b"\xfa\x031\xbd\"" in data
    assert b"\xfb\x062\x00\x92\x01" in data
    (entry,) = decode_auxiliary_entries(
        _payload(_entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 7, data)), 1
    )
    item = decode_item_instance(entry)
    assert item == ezdgn.V8ItemInstance(
        entry_id=7,
        schema_element_id=0x0001_02B3,
        class_name="Pump",
        attributes=ATTRIBUTES,
        properties=properties,
    )
    assert item.schema_reference == "Sample__x0020__Library.01.00"
    assert item.attribute("instanceId") == ""
    assert item.attribute("missing") is None
    assert item.get("Standby") == "P-101"
    assert item.get("Missing") is None
    assert item.get("Missing", "-") == "-"
    assert item.as_dict() == dict(properties)


def test_only_item_entries_with_the_observed_layout_are_decoded() -> None:
    data = _item_data(1, "Pump", ATTRIBUTES, (("Tag", "P-101"),))

    def decode(type_id: int, blob: bytes) -> ezdgn.V8ItemInstance | None:
        (entry,) = decode_auxiliary_entries(_payload(_entry(type_id, 1, blob)), 1)
        return decode_item_instance(entry)

    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, data) is not None
    assert decode(0x5709_0000, data) is None
    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE | 1, data) is None
    # truncated at every length
    for end in range(len(data)):
        assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, data[:end]) is None, end
    # a changed head or signature byte
    for offset in (*range(5), *range(9, 27)):
        changed = bytearray(data)
        changed[offset] ^= 1
        assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, bytes(changed)) is None, offset
    # a second property is the observed layout; the cases after it are not
    second_property = data[:-1] + bytes([CHILD, 0, VALUE, 0, END, END])
    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, second_property) is not None
    # trailing bytes, a nested element, and a property without a value
    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, data + b"\x00") is None
    nested = data[:-2] + bytes([CHILD, 0, VALUE, 0, END, END, END])
    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, nested) is None
    no_value = data[:-1] + bytes([CHILD, 0, END, END])
    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, no_value) is None
    # a count, a length or a string number that needs more than seven bits
    wide_count = bytearray(data)
    wide_count[27] = 0x80
    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, bytes(wide_count)) is None
    long_string = (
        HEAD
        + struct.pack("<I", 1)
        + SIGNATURE
        + bytes([1, 0xFA, 0x80])
        + b"a" * 0x80
        + bytes([ROOT, 0, ATTRIBUTE, 0, VALUE, 0, CONTENT, END])
    )
    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, long_string) is None
    high_number = data[:-1] + bytes([CHILD, 0x80, VALUE, 0, END, END])
    assert decode(V8_ITEM_INSTANCE_ENTRY_TYPE, high_number) is None


def _record(*entries: bytes) -> ezdgn.V8AuxiliaryRecord:
    payload = _payload(*entries)
    return ezdgn.V8AuxiliaryRecord(
        index=0,
        stream_path="made-up",
        inflated_offset=0,
        magic=0,
        kind=170,
        reserved=0,
        element_id=1,
        flags=len(entries),
        raw_bytes=payload,
        payload=payload,
    )


# One table string of 127 three-byte characters. An attribute pair is four
# stored bytes that name it twice: 762 bytes of text.
LONG = "あ" * 127


def _amplifying(pairs: int) -> bytes:
    return _item_data(1, LONG, ((LONG, LONG),) * pairs, ())


def _decode_one(blob: bytes) -> ezdgn.V8ItemInstance | None:
    (entry,) = decode_auxiliary_entries(
        _payload(_entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, blob)), 1
    )
    return decode_item_instance(entry)


def test_a_table_string_is_one_str_that_every_use_shares() -> None:
    item = _decode_one(_amplifying(500))
    assert item is not None and len(item.attributes) == 500
    assert item.class_name == LONG
    assert all(
        name is item.class_name and value is item.class_name
        for name, value in item.attributes
    )

    # The same holds across the properties, and for one call that decodes
    # the records of an element. Two instances have two tables.
    data = _item_data(
        1, "Pump", ATTRIBUTES, (("Tag", "P-101"), ("Standby", "P-101"), ("xmlns", "Tag"))
    )
    record = _record(
        _entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, data),
        _entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 2, data),
    )
    first, second = decode_item_instances((record,)).instances
    assert first.properties == second.properties
    assert first.properties[0][1] is first.properties[1][1]
    assert first.properties[2][0] is first.attributes[1][0]
    assert first.properties[2][1] is first.properties[0][0]
    assert first.properties[0][1] is not second.properties[0][1]
    assert isinstance(first.attributes, tuple) and isinstance(first.attributes[0], tuple)
    assert isinstance(first.properties, tuple) and isinstance(first.properties[0], tuple)


def test_two_table_entries_with_equal_text_stay_two_str_objects() -> None:
    # The table holds "Standby" twice, as its entries 1 and 3. Sharing goes
    # by table entry, not by text: the two uses of entry 2 are one object,
    # and the uses of entries 1 and 3 are two equal objects.
    table = b"".join(_table_string(text) for text in ("Pump", "Standby", "P-101", "Standby"))
    tokens = bytes(
        [ROOT, 0, ATTRIBUTE, 1, VALUE, 2, CONTENT]
        + [CHILD, 1, VALUE, 2, END, CHILD, 3, VALUE, 2, END, END]
    )
    item = _decode_one(HEAD + struct.pack("<I", 1) + SIGNATURE + bytes([4]) + table + tokens)
    assert item is not None
    assert item.attributes == (("Standby", "P-101"),)
    assert item.properties == (("Standby", "P-101"), ("Standby", "P-101"))
    assert item.attributes[0][0] is item.properties[0][0]
    assert item.properties[0][1] is item.properties[1][1]
    assert item.properties[0][0] == item.properties[1][0]
    assert item.properties[0][0] is not item.properties[1][0]


def test_item_decoding_is_held_to_the_stored_byte_limit() -> None:
    assert V8_MAX_ITEM_BYTES == 1_048_576
    fixed = len(_amplifying(0))
    assert fixed == 288
    # exactly the limit: decoded. Four bytes more: the entry stays raw.
    pairs = (V8_MAX_ITEM_BYTES - fixed) // 4
    at_limit = _amplifying(pairs)
    assert len(at_limit) == V8_MAX_ITEM_BYTES
    item = _decode_one(at_limit)
    assert item is not None and len(item.attributes) == pairs
    assert _decode_one(_amplifying(pairs + 1)) is None

    # One call for all records of an element shares one limit. Each large
    # entry is more than half of it: the second is counted, not decoded.
    # The small entries after it still fit and are decoded.
    large = _amplifying(V8_MAX_ITEM_BYTES // 8 + 1)
    assert V8_MAX_ITEM_BYTES // 2 < len(large) < V8_MAX_ITEM_BYTES
    small = _item_data(1, "Pump", ATTRIBUTES, (("Tag", "P-101"),))
    item_type = V8_ITEM_INSTANCE_ENTRY_TYPE
    records = (
        _record(_entry(item_type, 1, large), _entry(0x5709_0000, 2, small)),
        _record(_entry(item_type, 3, large), _entry(item_type, 4, small)),
        _record(_entry(item_type, 5, small), _entry(item_type, 6, large)),
    )
    decoded = decode_item_instances(records)
    assert isinstance(decoded, ezdgn.V8ItemInstances)
    assert [item.entry_id for item in decoded.instances] == [1, 4, 5]
    assert decoded.over_limit == 2

    # without the limit in reach every item entry is listed, in stored order
    decoded = decode_item_instances(
        (_record(_entry(item_type, 1, small)), _record(), _record(_entry(item_type, 2, small)))
    )
    assert [item.entry_id for item in decoded.instances] == [1, 2]
    assert decoded.over_limit == 0
    assert decode_item_instances(()) == ezdgn.V8ItemInstances(instances=(), over_limit=0)


def test_a_damaged_item_entry_stays_raw_and_hides_no_entry_after_it() -> None:
    # Large entries whose layout fails only at their end.
    large = _amplifying(V8_MAX_ITEM_BYTES // 8 + 1)
    cut = large[:-1]
    longer = large + b"\x00"
    wrong_number = large[:-3] + b"\x01" + large[-2:]
    small = _item_data(1, "Pump", ATTRIBUTES, (("Tag", "P-101"),))
    item_type = V8_ITEM_INSTANCE_ENTRY_TYPE
    for damaged in (cut, longer, wrong_number):
        assert _decode_one(damaged) is None
    decoded = decode_item_instances(
        (
            _record(_entry(item_type, 1, cut), _entry(item_type, 2, small)),
            _record(_entry(item_type, 3, longer), _entry(item_type, 4, wrong_number)),
            _record(_entry(item_type, 5, large), _entry(item_type, 6, small)),
        )
    )
    assert [item.entry_id for item in decoded.instances] == [2, 5, 6]
    assert decoded.over_limit == 0


def test_an_item_entry_over_what_is_left_is_counted_also_when_it_is_damaged() -> None:
    # The stored size alone decides: the call does not read an entry that is
    # larger than what it has left, so it counts it, damaged or not.
    large = _amplifying(V8_MAX_ITEM_BYTES // 8 + 1)
    cut = large[:-1]
    assert _decode_one(cut) is None
    small = _item_data(1, "Pump", ATTRIBUTES, (("Tag", "P-101"),))
    item_type = V8_ITEM_INSTANCE_ENTRY_TYPE
    decoded = decode_item_instances(
        (
            _record(_entry(item_type, 1, large)),
            _record(_entry(item_type, 2, cut), _entry(item_type, 3, small)),
        )
    )
    assert [item.entry_id for item in decoded.instances] == [1, 3]
    assert decoded.over_limit == 1
    # The same damaged entry first: it fits, it fails, and it is neither
    # charged nor counted.
    decoded = decode_item_instances(
        (
            _record(_entry(item_type, 1, cut)),
            _record(_entry(item_type, 2, large), _entry(item_type, 3, small)),
        )
    )
    assert [item.entry_id for item in decoded.instances] == [2, 3]
    assert decoded.over_limit == 0


# Most bytes of Python objects that the tests allow for one stored byte of
# item data. The worked number for a 64-bit CPython is 16: an attribute is
# four stored bytes and one pair of two references. The rest is room for
# other builds. A copy of the text for each use would need more than 100.
# The tests count the bytes that Python requests (tracemalloc), not the
# heap: the overhead of the allocator is not in these numbers.
PYTHON_BYTES_PER_STORED_BYTE = 32


def test_item_data_of_many_hostile_elements_takes_memory_in_line_with_the_stored_bytes() -> None:
    pairs, elements = 300, 1_000
    hostile = _amplifying(pairs)
    records = [
        (_record(_entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, hostile)),) for _ in range(elements)
    ]
    stored = elements * len(hostile)
    named = elements * pairs * 2 * len(LONG.encode())
    assert named > 100 * stored

    gc.collect()
    tracemalloc.start()
    try:
        # the item data of every element, decoded and kept
        kept = [decode_item_instances(element_records) for element_records in records]
        current, _ = tracemalloc.get_traced_memory()
    finally:
        tracemalloc.stop()
    assert len(kept) == elements
    assert all(len(decoded.instances) == 1 and decoded.over_limit == 0 for decoded in kept)
    assert all(len(decoded.instances[0].attributes) == pairs for decoded in kept)
    assert current <= PYTHON_BYTES_PER_STORED_BYTE * stored, (current, stored)
    assert current < named // 8


def test_cli_item_output_takes_memory_in_line_with_the_stored_bytes(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    from ezdgn import __main__ as cli

    # The model of the checked-in file, 20 times, with one hostile item
    # record on every element. Only the file read is replaced.
    document = ezdgn.read_v8(V8)
    (model,) = document.models
    pairs = 300
    hostile = _amplifying(pairs)
    record = _record(_entry(V8_ITEM_INSTANCE_ENTRY_TYPE, 1, hostile))

    def run(records: tuple[ezdgn.V8AuxiliaryRecord, ...]) -> tuple[str, int, int]:
        elements = 20 * tuple(
            dataclasses.replace(element, auxiliary_records=records)
            for element in model.elements
        )
        changed = dataclasses.replace(
            document, models=(dataclasses.replace(model, elements=elements),)
        )
        monkeypatch.setattr(cli, "read_v8", lambda path: changed)
        gc.collect()
        tracemalloc.start()
        try:
            assert cli.main(["inspect", str(V8), "--entities", "--json"]) == 0
            _, peak = tracemalloc.get_traced_memory()
        finally:
            tracemalloc.stop()
        return capsys.readouterr().out, peak, len(elements)

    plain_output, plain_peak, _ = run(())
    output, peak, elements = run((record,))
    assert elements == 20 * 58
    stored = elements * len(hostile)
    named = elements * pairs * 2 * len(LONG.encode())
    assert named > 100 * stored

    # every string is written once; a use is the position of its string
    rows = json.loads(output)["v8_models"][0]["records"]
    assert len(rows) == elements
    assert all(
        row["item_instances"]
        == [
            {
                "entry_id": 1,
                "schema_element_id": 1,
                "strings": [LONG],
                "class_name": 0,
                "attributes": [[0, 0]] * pairs,
                "properties": [],
            }
        ]
        for row in rows
    )
    assert "item_instances" not in json.loads(plain_output)["v8_models"][0]["records"][0]
    # The whole run, in output bytes and in peak memory, stays within a
    # fixed multiple of the stored bytes.
    assert len(output.encode()) - len(plain_output.encode()) <= 8 * stored
    assert peak - plain_peak <= 2 * PYTHON_BYTES_PER_STORED_BYTE * stored, (peak, plain_peak)
    assert peak - plain_peak < named // 4


def test_cli_item_strings_are_listed_once_in_the_order_of_first_use() -> None:
    from ezdgn.__main__ import _v8_item_payload

    properties = (("Tag", "P-101"), ("Duty", ""), ("Standby", "P-101"), ("Tag", "Duty"))
    item = _decode_one(_item_data(900, "Pump", ATTRIBUTES, properties))
    assert item is not None
    payload = _v8_item_payload(item)
    strings = payload["strings"]
    assert strings == [
        "Pump",
        "instanceId",
        "",
        "xmlns",
        "Sample__x0020__Library.01.00",
        "Tag",
        "P-101",
        "Duty",
        "Standby",
    ]
    assert payload["entry_id"] == 1 and payload["schema_element_id"] == 900
    assert strings[payload["class_name"]] == "Pump"
    assert (
        tuple((strings[name], strings[value]) for name, value in payload["attributes"])
        == ATTRIBUTES
    )
    assert (
        tuple((strings[name], strings[value]) for name, value in payload["properties"])
        == properties
    )


def test_fixture_auxiliary_record_holds_one_entry_and_no_item_data() -> None:
    document = ezdgn.read_v8(V8)
    (page,) = document.raw.named_auxiliary_pages
    (record,) = page.records
    assert record.flags == 1
    assert record.entries is not None
    (entry,) = record.entries
    assert (entry.type_id, entry.entry_id, entry.size_word) == (0x5709_0000, 0, 102)
    assert not entry.size_flag
    assert entry.data[:10] == bytes.fromhex("03000000c0000000789c")
    assert record.payload == (
        struct.pack("<IIII", entry.type_id, 0, 102, 0) + entry.data + b"\x01\x00\x00\x00"
    )
    assert decode_item_instance(entry) is None
    assert all(
        element.item_instances == ()
        for model in document.models
        for element in model.elements
    )


# The checks below read a public file that the repository does not
# redistribute: the "Water distribution mains" support example recorded in
# docs/v8/PROVENANCE.md. Set EZDGN_PUBLIC_V8_DIR to the folder that holds it.
PUBLIC_DIR = os.environ.get("EZDGN_PUBLIC_V8_DIR")
WATER_MAINS = "Water_distribution_mains.dgn"
WATER_MAINS_SHA256 = "f34bbb1b408b583d9cfc9d280e4d5782559cfa824b4b51210a03cf5c88235b64"


@pytest.mark.skipif(
    not PUBLIC_DIR or not (Path(PUBLIC_DIR or ".") / WATER_MAINS).is_file(),
    reason="set EZDGN_PUBLIC_V8_DIR to a folder with the public water mains file",
)
def test_public_water_mains_item_instances() -> None:
    import hashlib

    path = Path(PUBLIC_DIR or ".") / WATER_MAINS
    assert hashlib.sha256(path.read_bytes()).hexdigest() == WATER_MAINS_SHA256
    document = ezdgn.read_v8(path)
    (model,) = document.models
    assert len(model.elements) == 66_053

    records = [record for element in model.elements for record in element.auxiliary_records]
    assert len(records) == 66_053
    assert all(record.entries is not None and len(record.entries) == 2 for record in records)
    assert Counter(
        (entry.type_id, entry.entry_id, entry.size_flag, entry.reserved)
        for record in records
        for entry in record.entries or ()
    ) == {(0x56FF_0000, 1, False, 0): 66_053, (0x56FF_0000, 2, False, 0): 66_053}

    assert Counter(
        tuple((item.entry_id, item.class_name) for item in element.item_instances)
        for element in model.elements
    ) == {((1, "WaterMain"), (2, "Metadata")): 66_053}
    items = [item for element in model.elements for item in element.item_instances]
    assert Counter(item.attributes[0] for item in items) == {("instanceId", ""): 2 * 66_053}
    assert Counter(item.schema_reference for item in items) == {
        "DgnCustomItemTypes_Default__x0020__Library.01.00": 2 * 66_053
    }
    assert Counter(
        tuple(name for name, _ in item.properties) for item in items
    ) == {
        ("GaugeMM", "LiningMaterial", "Material"): 66_053,
        ("InstallationDate",): 66_053,
    }
    assert len({item.schema_element_id for item in items}) == 1
    schema_holder = next(
        item
        for page in document.raw.named_pages
        for item in page.objects
        if item.element_id == items[0].schema_element_id
    )
    assert schema_holder.element_type == 39

    first = model.elements[0].item_instances
    assert first[0].as_dict() == {
        "GaugeMM": "150",
        "LiningMaterial": "Cement Lined",
        "Material": "Ductile Iron",
    }
    assert first[1].as_dict() == {"InstallationDate": "1986-01-01"}
