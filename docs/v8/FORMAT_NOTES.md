# DGN V8 format notes

This is the evidence ledger for the native V8 reader. `Confirmed` means that a
rule is either defined by the public CFB format or reproduced by the parser on
the checked-in fixture and independently sourced files listed in
[`PROVENANCE.md`](PROVENANCE.md). A decoder remains conservative when a field
does not meet that bar.

## Reproducible fixture observations

The checked-in baseline can be inspected at every layer:

```bash
cargo run -p ezdgn-core --example v8_stream_dump -- \
  tests/data/dgn/v8/test_dgnv8.dgn
cargo run -p ezdgn-core --example v8_object_scan -- \
  tests/data/dgn/v8/test_dgnv8.dgn
cargo run -p ezdgn-core --example v8_read -- \
  tests/data/dgn/v8/test_dgnv8.dgn
```

Its fixed observations are:

- CFB major version 3, 9 non-root storages, 15 streams, and 24 entries;
- 21,535 logical stream bytes, verified against
  `tests/data/dgn/v8/manifest.json`;
- model storage `/Dgn-Md/#000000`, model name `my_model`, description
  `my_description`, 3D, 10,000 UOR/master, and units `m` / `mm`;
- 58 graphical raw objects, 83 total raw objects, and 31,942 inflated bytes;
- 34 feature entities with the CSV type set
  `2,3,4,6,11,12,14,15,16,17,22,27,35,36`;
- 19 effective 2D and 15 effective 3D features; and
- the decoded text `myTéxt`, its exact encoded bytes, common symbology, and
  complex/cell parent-child relationships.

The same scanner and semantic reader were run without code changes on two
public, non-committed V8 files on 2026-07-29:

| SHA-256 prefix | Bytes | Observed model | Graphical objects | Feature entities | Notable population |
| --- | ---: | --- | ---: | ---: | --- |
| `f34bbb1b408b` | 3,840,000 | `Default` / `Master Model`, 2D, 10 UOR/master, `m` / `m` | 66,053 | 66,053 | 18 lines, 66,035 line strings, 66,053 associated auxiliary records |
| `8439e6d3a182` | 847,360 | `Default` / `Master Model`, 2D, 10 UOR/master, `m` / `m` | 33,335 | 12,623 | 6,904 text-node headers, 12,623 text children, 13,808 unknown raw objects |

These files broaden structural coverage but do not make their application
schemas part of the supported semantic contract. Full hashes and acquisition
links are in `PROVENANCE.md`.

## Container and compression

- The outer file is CFB. Generic sector, directory, storage, and logical-stream
  behavior is delegated to the Rust `cfb` crate.
- Required DGN markers are `/Dgn~H`, `/Dgn~S`, and `/Dgn-Md`.
- `/Dgn^Ix/Dgn~Mix` indexes models. The model ID at entry offset 4 names the
  entry's storage `/Dgn-Md/#nnnnnn` in lowercase hex; this association is
  retained separately from IDs stored in the model index and model header.
- Known compressed streams use zlib. A decode is valid only when the decoder
  reaches `StreamEnd`, consumes the complete compressed payload, and stays
  within both per-stream and aggregate inflated-byte limits. A truncated stream
  with a missing checksum is rejected.
- Numbered object and auxiliary streams contain a 16-byte uncompressed page
  header followed by a zlib payload, unless an empty page has no payload.

The baseline compressed-stream observations retained from the structure spike
are:

| Stream | zlib offset | Inflated bytes | Inflated SHA-256 |
| --- | ---: | ---: | --- |
| `/Dgn~H` | `0x14` | 1,576 | `8012a6233f6bf0128a918b2dd34436e0d273eecec9da3441f196363e22aaedb7` |
| `/Dgn~Mf` | `0x00` | 276 | `9cf88ba5ead54c333f844ee67f6a881b9fdb4a5fc4d9534afb10fe6bee7b297a` |
| `/Dgn-Md/#000000/Dgn~Mh` | `0x00` | 4,716 | `5a1d167aeb9238afd50c61ae90bf10124785e1471bcbcc4ae9209528915a5665` |
| `/Dgn-Md/#000000/Dgn^G/$1` | `0x10` | 10,868 | `3aa5d6e58f14b43f43cebeb78c1f1462e514db0b030922900b21bf9e64645547` |
| `/Dgn^Ix/Dgn~Mix` | `0x00` | 92 | `fc11d4a66c282e92c4c207a653bd35a1bd63147a3377737401e0ad668af8652c` |
| `/Dgn^Nm/$1` | `0x10` | 16,116 | `596832db9fdaab85985bf9737078721d7435e00c2ce81412d54c8510919be908` |
| `/Dgn^NmA/$1` | `0x10` | 150 | `afce43b6c5df783ff68f9923613c1ea48bfe9e16a147dfbd0166ad46b7907e90` |

## Page and raw-object framing

The 16-byte little-endian page header is:

| Offset | Type | Meaning |
| ---: | --- | --- |
| `0x00` | `u32` | record count |
| `0x04` | `u32` | page format version, accepted values 2 and 3 |
| `0x08` | `u32` | page number; zero is advisory/unspecified in observed format-3 control/name pages |
| `0x0c` | `u32` | population; zero is advisory/unspecified in the same cases |

An inflated object page repeats a four-byte framing prefix followed by one
object. The object begins with:

| Offset | Type | Meaning |
| ---: | --- | --- |
| `0x00` | `u32` | element type in the low 16 bits plus flags |
| `0x04` | `u32` | total object length in 16-bit words |
| `0x08` | `u32` | attribute boundary in 16-bit words |

The complete raw object is exactly `2 * words` bytes. Primary bytes precede
`2 * attribute_words`; linkage bytes follow it. Role flags `0x20000000` and
`0x40000000` identify a complex header and component respectively. Parsing
requires every declared object to fit, consumes exactly the page payload, and
rejects trailing bytes.

An inflated auxiliary page instead repeats a 28-byte record header plus its
payload. The confirmed fields are magic `0x0000a11b` at `+0x00`, payload length
at `+0x04`, kind at `+0x08`, a retained reserved value at `+0x0c`, element ID at
`+0x10`, and flags at `+0x18`. Auxiliary records are associated by element ID
but remain independently indexed and byte-exact. The payload layout is
described under [Auxiliary record payload entries](#auxiliary-record-payload-entries).

## Model index and model header

Inflated `/Dgn^Ix/Dgn~Mix` begins with magic `0xaa00ba11`, version 4, and entry
count. Each variable-size entry has a 32-byte header:

| Offset | Type | Meaning |
| ---: | --- | --- |
| `0x00` | `u32` | flags word: bit 16 marks a 3D model, bit 0 was seen on a sheet model |
| `0x04` | `u32` | model ID, which names the model storage |
| `0x08` | `u64` | index model ID |
| `0x10` | `u16` | complete entry byte length |
| `0x12` | `u16` | UTF-16LE model-name byte length |
| `0x14` | `u32` | UTF-16LE description byte length |
| `0x20` | bytes | model name followed by description |

Odd UTF-16 lengths, invalid UTF-16, undersized entries, overflow, trailing
bytes, and configured string/model limits are rejected.

The storage is `/Dgn-Md/#` plus the model ID as six lowercase hex digits. In
MicroStation CONNECT and OpenRail files a second model with ID 1 or 2 lives in
`#000001` or `#000002` while its flags word holds `0x00000000` or
`0x00010000`, so the low half of the flags word does not name the storage.
Hex is corroborated by hidden models in `/Dgn-Nd`, which share the ID sequence
and are named `#00000d` through `#000033`. A decimal name and the low half of
the flags word are accepted as fallbacks, and no storage is assigned to two
entries; storages no entry names are still loaded, with an empty name.
Canonical hex paths are assigned for all entries before decimal and legacy
fallbacks, so a stale entry cannot claim another model's canonical storage.
Path lookup uses the full 32-bit ID; the historical `storage_index` field
retains its 16-bit type, while `flags` and `storage_path` retain the full ID.

Bit 16 of the flags word is set exactly on the models with 3D content in the
files observed, including an ODA-written 3D model, and clear on 2D design and
sheet models whose type-66 header does not carry the 2D flag. A model named by
an index entry takes its dimension from that bit; `model_number` (the high half
of the flags word) therefore equals the 3D bit in those files.

The per-model `Dgn~Mh` stream contains one complete type-66 object ending at
the end of the inflated stream. Confirmed decoded offsets within that object
are:

- header model ID at `0x18`;
- the 2D model flag `0x00800000` in the type/flags word (clear means 3D in the
  observed files);
- signed 64-bit UOR extents from `0x90` through `0xbf`;
- a three-`f64` global-origin candidate at `0xc8`; and
- positive finite `f64` UOR/master at `0xe0`.

Master coordinates are `(uor - global_origin_uor) / uor_per_master`. Model IDs
from the index and header can differ in older produced files, so the storage
path is the association key and both IDs are exposed.

## Common graphical header and element families

Standard graphical objects share a 0x68-byte prefix:

| Offset | Type | Meaning |
| ---: | --- | --- |
| `0x0c` | `u32` | level |
| `0x10` | `u64` | element ID |
| `0x18` | `u64` | model ID |
| `0x20` | `u32` | graphic group |
| `0x24` | `u32` | properties |
| `0x28` | `u32` | geometry flags; `0x00000800` indicates stored 3D |
| `0x2c` | `u32` | line style |
| `0x30` | `u32` | line weight |
| `0x34` | `u32` | color index |
| `0x38..0x67` | six `i64` | low XYZ corner, then XYZ extent, in UOR |

The range's second triple is the extent, not the high corner: in files written
by MicroStation CONNECT, OpenRail and ODA, low plus extent equals the vertex
maximum for every element checked. The decoded range reports the high corner.

Point payloads use little-endian finite `f64`: XY for a 2D object and XYZ for
a 3D object. Complex headers in the baseline can store a 2D bit while owning
3D children; `stored_dimension` preserves that bit and `dimension` is resolved
from descendants for the aggregate feature.

The decoder maps standard element types as follows:

| Type | Native result |
| ---: | --- |
| 2 | cell header, origin, transform, child count |
| 3 | line |
| 4 | line string |
| 6 | shape |
| 7 | text-node header |
| 11 | native curve point sequence |
| 12 / 14 | complex chain / complex shape |
| 15 / 16 | ellipse / arc native parameters |
| 17 | text |
| 21 / 27 | B-spline pole sequence / curve header |
| 22 | point string with per-point orientation |
| 35 | shared-cell instance, origin, transform, linked name when present; the origin is the last three doubles of the record whether or not the 3D flag is set |
| 36 | bounded dimension anchor |

An unrecognized object carrying a header role becomes `UnknownComplex` with a
bounded child count; any other unrecognized object becomes `Unknown`. Both
retain the full raw object.

For type-17 text, the finite `f64` values at `0x70` and `0x78` are the raw
width and height multipliers. Their UOR distances are
`raw_multiplier * 6 / 1000`; master-unit distances apply the model scale once
more. The semantic model retains both raw multipliers and both converted
distances. This conversion reproduces the checked-in GDAL oracle's
`s:1.000000g` labels and the independently observed public text-node fixture.

The type-17 text payload starts right after the fixed header (editable-field
word at `0xa8` for 2D, `0xc8` for 3D; payload at `0xaa` / `0xca`), its byte
length being the `u16` at `0x6e`. The object may be padded to an even or a
4-byte length after the payload, so the payload position is anchored on the
header layout, not derived from the object length (a real-world file padded
some texts to 4 bytes and previously failed with "text payload begins at
0xac"). Complex-header child counts are treated as best effort: a standalone
object closes still-open headers, a component without an open header is kept
standalone, and headers still open at the end of a model keep the children
actually seen.

Text type 17 supports valid UTF-8, Windows-1252 fallback, and the observed
`ff fe 01 00` escaped Windows-1252 marker. The baseline value `myTéxt` is
decoded while its original bytes `ff fe 01 00 6d 79 54 e9 78 74` remain
available. Inline linkages are word-framed with byte length
`2 * (low_byte(signature) + 1)`. Property linkage kind `0x0056d210` stores the
property ID at `+0x04`, payload length at `+0x08`, and payload at `+0x0c`;
observed `ff fd` payloads are UTF-16LE, and `ff fe 01 00` payloads are escaped
Windows-1252 like text type 17. MicroStation CONNECT files use the escaped form
for unit labels (properties 19 and 20) and cell names, and plain 8-bit text for
other names in the same file. Unknown and incomplete linkages retain their
exact bytes.

## Meshes

Type 105 is a mesh header. Its component count is at `0x68` and its style at
`0x6c`; style 1 is an indexed face-loop mesh. In the observed files (an OpenRail
cable-duct model, 261 meshes) every mesh has four components in the order
101, 103, 101, 102:

| Type | Role | Observed layout |
| ---: | --- | --- |
| 101 | matrix header | component count at `0x68` (0), column count at `0x6c`: 3 before the points, 1 before the indices |
| 103 | double data | scalar count at `0x68` (repeated at `0x6c`), constant tag 49 at `0x70`, `f64` values from `0x78`, padded to four bytes |
| 102 | integer data | scalar count at `0x68` (repeated at `0x6c`), constant tag 4386 at `0x70`, `i32` values from `0x78`, padded to four bytes |

The double data holds XYZ points in UOR, three scalars per point. The integer
data holds one-based point indices; zero ends a face loop and a negative index
hides the edge that starts at that point. Loops of 3 to 144 points were seen,
mostly quadrilaterals, and every index was in range. The decoded `Mesh` keeps
the points (UOR and master) and the loops as zero-based indices with edge
visibility. A mesh of another style, or whose components do not form a valid
indexed face-loop mesh, stays an unknown complex element.
The supported layout is the four-component XYZ/point-index matrix pair above;
other column counts and missing or extra components remain unknown. Empty
meshes and face loops with fewer than three indices also remain unknown.
The decoded point count is bounded by `max_vertices` before allocating points.
The current mesh tests use constructed records; the private OpenRail corpus
reported by the contributor is not included in the regression fixtures.

## Level table entries

Levels are defined by named objects (`/Dgn^Nm` pages) of element type 95 whose
header `level` word (`0x0c`) is 1; the other table kinds observed in that type
are 2 (fonts), 7 and 8 (line style definitions). The fields reproduced by the
reader on the checked-in fixture (`Default`) and on two real-world files that
are not checked in (one written by MicroStation with 2,666 levels, one
converted from DWG with 15) are:

| Offset | Type | Meaning |
| ---: | --- | --- |
| `0x20` | `u32` | level ID: the number graphical headers store at `0x0c` |
| `0x40` | `i32` | line style of the level (ByLevel symbology) |
| `0x44` | `u32` | line weight of the level, 0..=31 |
| `0x48` | `u32` | color index of the level |
| `0x5c` | `u32` | user-visible level number |

The name is a `0x56d2` linkage in the attribute bytes: version at `+0x04`,
payload byte length at `+0x08`, payload at `+0x0c`. Payloads were `ff fd` +
UTF-16LE (ODA), `ff fe 01 00` + 8-bit text (MicroStation) and plain 8-bit text
(DWG conversion). The weight field was corroborated by elements that carry the
ByLevel sentinel `0xffffffff` in their header weight: in the DWG conversion
every element on the two levels whose entry stores weight 2 uses the sentinel,
and the only other weights in that file are explicit 0 and 2. `0x4c` repeated
the color index on the MicroStation file and was 0 on the DWG conversion;
`0x28`, `0x2c` and `0x60` vary by file and remain unresolved. Element headers
use `0xffffffff` (ByLevel) and `0xfffffffe` (ByCell) in their symbology words.

## Auxiliary record payload entries

The payload of an auxiliary record is a sequence of entries. The record header
word at `+0x18`, which the raw model exposes under its historical name `flags`,
is the entry count; the word at `+0x08` (`kind`) was 170 in every record seen.
Each entry is a 16-byte little-endian header followed by its data:

| Offset | Type | Meaning |
| ---: | --- | --- |
| `0x00` | `u32` | type word: the high half groups related entry types, the low half numbers the types of one group |
| `0x04` | `u32` | entry ID, which tells entries of one type on one element apart |
| `0x08` | `u32` | data length in the low 31 bits; bit 31 is a writer flag |
| `0x0c` | `u32` | zero in every entry seen |
| `0x10` | bytes | data |

The four bytes `01 00 00 00` follow the last entry and end the payload. A
record without entries is those four bytes alone.

This layout splits every auxiliary record of the files observed, with no byte
left over:

| Source | Records | Entries | Entries per record | Bit 31 of the length word |
| --- | ---: | ---: | --- | --- |
| checked-in fixture | 1 | 1 | 1 | clear |
| public water mains file (`f34bbb1b408b`) | 66,062 | 132,118 | 1, 2, 4 | clear |
| 90 confidential real-world files | 783 | 1,024 | 0 to 8 | set |

The table counts every auxiliary record of a file. In the public file 66,053
records are on the graphical elements, with the 132,106 item entries that
`PROVENANCE.md` reports, and 9 records are on named objects, with 12 entries
of other types.

The fixture's one record is on a named object: type word `0x57090000`, entry
ID 0, 102 data bytes that begin `03 00 00 00 c0 00 00 00 78 9c` (a `u32` 3, the
inflated size 192, then a zlib stream). The meaning of bit 31 is unresolved;
the length is correct with the bit masked off in all three sources. That the
bit can be set rests on confidential files alone (see
"Rules that rest on confidential files" below). Entry data
other than the item instances below is not interpreted. A payload that does
not hold exactly the declared number of entries followed by the closing word
is not split at all and stays a raw payload. The entry count comes from the
file and an entry can be an empty header, so a payload that declares more than
65,536 entries is not split either. The largest count seen in one record is 4
in the public files and 8 in the confidential files.

## Item instances

An entry with type word `0x56ff0000` on a graphical element holds one item
instance: the values of one item type (item class) for that element. The data
is a small XML element in a tokenized binary form:

| Offset | Bytes | Meaning |
| ---: | --- | --- |
| `0x00` | `00 00 01 01 15` | constant |
| `0x05` | `u32` | element ID of the named object that holds the schema text |
| `0x09` | `01` | constant |
| `0x0a` | `42 45 43 58 4d 4c 00` | `BECXML` and a zero byte |
| `0x11` | `ff 0a 0d` | constant |
| `0x14` | `01 00 00 03 00 00 30` | constant |
| `0x1b` | `u8` | string count |
| `0x1c` | | string table, then the token stream to the end of the data |

A string table entry is `fa`, a `u8` length and that many bytes, one byte for
each character with the byte value as the code point; or `fb`, a `u8` count of
UTF-16 code units and that many little-endian units. Equal strings share one
entry, and the entries are in the order of first use. A token is one byte,
followed by a `u8` string number where it names a string:

| Token | Operand | Meaning |
| ---: | --- | --- |
| `03` | string | start of the root element; the string is the item class name |
| `05` | string | attribute name of the root element |
| `11` | string | value of the attribute or property just named |
| `06` | | end of the root attributes |
| `02` | string | start of a property element; the string is the property name |
| `04` | | end of the open element |

Every instance observed is `03 class (05 name 11 value)+ 06 (02 name 11 value
04)* 04`: a root element with attributes, then one child element with one text
value for each property, and nothing after the closing `04`. An empty value is
a reference to the empty string, not a missing `11`. A made-up instance
`<Pump instanceId="" xmlns="Lib.01.00"><Tag>P-101</Tag></Pump>` with schema
holder 42 is stored as:

```text
00 00 01 01 15  2a 00 00 00  01  42 45 43 58 4d 4c 00  ff 0a 0d
01 00 00 03 00 00 30  07
fa 04 "Pump"  fa 0a "instanceId"  fa 00  fa 05 "xmlns"
fa 09 "Lib.01.00"  fa 03 "Tag"  fa 05 "P-101"
03 00  05 01 11 02  05 03 11 04  06  02 05 11 06 04  04
```

The public water mains file listed in `PROVENANCE.md` carries two instances on
each of its 66,053 elements, 132,106 in total, and every one decodes with this
layout: entry ID 1 is class `WaterMain` with the properties `GaugeMM`,
`LiningMaterial` and `Material`, entry ID 2 is class `Metadata` with
`InstallationDate`. The root attributes are `instanceId` (empty) and `xmlns`
(`DgnCustomItemTypes_Default__x0020__Library.01.00`) in all of them. The
`u32` at `0x05` is the element ID of a named object of type 39 whose bytes hold
`fLMX` at `0x28`, `01 00 ff 56` at `0x2c`, and the schema as UTF-16LE XML text
(`<ECSchema schemaName="DgnCustomItemTypes_Default__x0020__Library"
version="01.00" ...>`). The `xmlns` value is that schema's name and version,
the root name is an `ECClass` `typeName` of it, and the property names are the
`propertyName` values of that class in the same order. The schema's
`displayLabel` is `Default Library`, so `__x0020__` in a stored name stands for
a space; names are returned as stored. Every property in that schema has
`typeName="string"`, and values such as `150` and `1986-01-01` are stored as
text. In that file the entry ID equals the `ID` the schema records for the
class.

Five confidential real-world files (drawings converted from DWG, with block
attributes turned into item instances) agree: 140 instances on
shared-cell instances, all decoded, each with the same two root attributes and
with the properties of a class in a schema held by a type-39 object (15
properties in 139 instances, one in the other). In those 140 instances the
first property value equals the text that the file draws next to the cell.
They also give the only evidence for three parts of the layout. A reviewer
cannot reproduce these three from public files; how each was found is in
"Rules that rest on confidential files" below:

- the `fb` UTF-16 string form (11 strings in 11 instances), used where a
  string has a character above U+00FF (U+0192 in those files). Each of the 11
  strings equals the text of a text element in the same file;
- bytes `0x80` and above in the `fa` form (36 strings in 36 instances: `81` in
  19, `bd` in 16, `be` in 1), read as the code points U+0081, U+00BD and
  U+00BE. Each of the 36 strings equals the text of a text element in the
  same file, and the writer used the `fb` form for U+0192, which a
  Windows-1252 byte could have held, so the `fa` form is taken as one code
  point per byte and not as a code page. No other byte from `0x80` to `0x9f`
  was seen in the `fa` form; and
- bit 31 of the entry length word being set (in all 140 item entries, and in
  all 1,024 entries of those files).

In those files the entry ID was 1 for a class whose schema `ID` is not 1 in 34
instances, so the entry ID is exposed but is not treated as the class ID.

The decoder accepts only what was observed and decodes nothing else:

- the constants above must match byte for byte;
- the string count, each string length and each string number must be below
  `0x80` (the largest values seen were 23 strings, a length of 66 and string
  number 22); a larger byte may belong to a wider encoding that was not seen;
- a string tag other than `fa` and `fb`, invalid UTF-16, a string number
  outside the table, any other token, a root without attributes, a property
  without exactly one value, a nested element, or any byte after the closing
  token leaves the entry undecoded;
- a table string is never copied. A string number can name a long table
  string any number of times: one attribute is four stored bytes and can name
  762 bytes of text. The table is therefore decoded once, and the class name,
  each name and each value is a reference to one of its strings (a shared
  handle in Rust, one `str` object in Python). The memory of a decoded
  instance is then at most a fixed multiple of its stored bytes, whatever the
  string numbers are. The worked worst case, on a 64-bit target, in the
  bytes that the decoder requests:

  | Part of an instance | Stored bytes | Rust, bytes | CPython 3.12, bytes |
  |---|---:|---:|---:|
  | one attribute (two string numbers) | 4 | 32: two handles | 64: one pair of two references and its place in the list |
  | one property | 5 | 32 | 64 |
  | the class name | 2 | 16: one handle | 8: one reference |
  | an empty table string | 2 | 16: the two counters of its block | 0: the shared empty string |
  | a table string of one wide character | 4 | 19 | 60 |
  | the longest table string, 127 wide units | 256 | 397 | 312 |
  | head, signature, counts and marks | 30 | 72: the fixed part | 216 at most: the fixed part |

  That is at most 8 requested bytes for one stored byte in Rust and at most
  16 in Python. These figures, and the tests, count requested bytes: the
  sizes that the decoder asks the allocator for. An allocator rounds a block
  up and keeps its own bookkeeping, and that overhead can add up to about 2
  times for small blocks (a block of 16 bytes takes a chunk of 32 bytes in
  glibc). The Rust tests assert the 8. The Python tests assert 32, to leave
  room for other builds; a run in Python on nine constructed shapes gave 1.3
  to 15.7 (the script of that run is not in the repository). On a 32-bit
  target the Rust number is 4 (worked out, not run);
- one call returns at most 1,048,576 stored bytes of item data as decoded
  instances, so one call returns at most 8 MiB of requested bytes in Rust
  and 16 MiB of objects in Python. The data of each returned instance
  counts. An item entry whose data is larger than what the call has left is
  not read, and the call reports how many entries it left raw for that
  reason; such an entry is counted also when it is damaged, because the call
  does not look at its data. The entries after it are still read. An entry
  that fits and does not match the layout is read up to the byte where it
  fails and is then dropped: it costs nothing of the limit, it hides no
  entry after it, and its memory is free again before the next entry is
  read. One call therefore reads every item entry of the element that fits,
  in time in line with the stored bytes. One entry decoded alone has the
  same limit. The largest item data seen on one element is 388 stored bytes
  in the public file (408 in the confidential files); and
- the sum over a document needs no second limit. For one read, an auxiliary
  record belongs to one element, so the item data of all elements together
  is at most the inflated bytes of the file, which
  `max_total_inflated_bytes` holds. A caller that keeps the item data of
  every element of one read holds at most 8 times (Rust) or 16 times
  (Python) those bytes, as requested bytes.
  `ezdgn inspect --entities --json` writes each string of an instance once
  and gives every use as a position in that list, so its output and its
  memory follow the same rule. In a test with 1,160 elements whose item data
  names 154 bytes of text for each stored byte, a run gave under 4 bytes of
  output and under 33 bytes of peak memory for each stored byte; the test
  asserts 8 and 64.

An undecoded entry is not an error: the element is read as before and the
entry keeps its exact bytes. The schema text of the type-39 object is not
decoded by the reader; the remaining words of that object (`0x30` to the start
of the text) and the meaning of the constants at `0x00`, `0x09`, `0x11` and
`0x14` of the instance data are unresolved. The item tests use constructed
bytes; the public file is read by an optional test when it is present, and the
five confidential files are not included in the regression fixtures.

## Rules that rest on confidential files

Some rules above have no public evidence. Their evidence is a set of 90
real-world production files, 85 2D files and five 3D files, that are highly
confidential. They cannot be shared. No byte, name or text of them is in this
repository, and a reviewer cannot reproduce the counts below.

Each rule was found in the same way as the rules with public evidence: the
bytes of lawfully held files were read, a rule was stated, and it was counted
where the rule holds and where it does not. Most rules were also checked
against something that the reader already decodes in the same file. A row
that has no such check says so. The table gives the check, the counts, and
the constructed test that covers the rule. The tests use made-up bytes only.
Each rule can be taken out, and its form left raw, if that is preferred.

| Rule | How it was found | Counts | Constructed test |
| --- | --- | --- | --- |
| Item data: the `fb` string form holds UTF-16LE units | Read as UTF-16LE, the string equals the text of a text element in the same file | 11 strings in 11 of 140 instances; each holds a character above U+00FF; 11 of 11 equal a text element | `one_byte_strings_are_code_points_and_wide_strings_are_utf16` |
| Item data: a byte `0x80` or above in the `fa` form is one code point | Read as one code point per byte, the string equals the text of a text element in the same file; the writer uses the `fb` form for a character that a Windows-1252 byte could hold | 36 strings in 36 of 140 instances (`81` in 19, `bd` in 16, `be` in 1); 36 of 36 equal a text element | the same test |
| Auxiliary entries: bit 31 of the length word is a flag and not part of the length | A check of structure only, not against other decoded data: with the bit masked off the lengths split every record with no byte left over, and every item entry decodes; with the bit kept each length is longer than its record. What the flag means is not known | set in 1,024 of 1,024 entries of 783 records; all 140 item entries among them decode | `splits_entries_and_keeps_every_header_word` |

## Deliberately unresolved semantics

The parser does not infer other product-specific XAttributes, custom schemas,
typed Item Type values, full dimensions, or complete B-spline/surface
definitions from names or isolated byte patterns. Those bytes remain
accessible through raw objects, linkages, and auxiliary records. New semantic
mappings require another controlled observation and a malformed-input
regression before this ledger is expanded.
