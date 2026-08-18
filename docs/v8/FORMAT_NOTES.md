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
- `/Dgn^Ix/Dgn~Mix` indexes models. A storage index associates an entry with
  `/Dgn-Md/#NNNNNN`; this association is retained separately from IDs stored in
  the model index and model header.
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
but remain independently indexed and byte-exact.

## Model index and model header

Inflated `/Dgn^Ix/Dgn~Mix` begins with magic `0xaa00ba11`, version 4, and entry
count. Each variable-size entry has a 32-byte header:

| Offset | Type | Meaning |
| ---: | --- | --- |
| `0x00` | `u32` | low 16 bits storage index; high 16 bits model number |
| `0x04` | `u32` | flags |
| `0x08` | `u64` | index model ID |
| `0x10` | `u16` | complete entry byte length |
| `0x12` | `u16` | UTF-16LE model-name byte length |
| `0x14` | `u32` | UTF-16LE description byte length |
| `0x20` | bytes | model name followed by description |

Odd UTF-16 lengths, invalid UTF-16, undersized entries, overflow, trailing
bytes, and configured string/model limits are rejected.

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
| `0x38..0x67` | six `i64` | low/high XYZ range in UOR |

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
| 35 | shared-cell instance, origin, transform, linked name when present |
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
observed `ff fd` payloads are UTF-16LE. Unknown and incomplete linkages retain
their exact bytes.

## Deliberately unresolved semantics

The parser does not infer product-specific XAttributes, custom schemas, Item
Types, full dimensions, or complete B-spline/surface definitions from names or
isolated byte patterns. Those bytes remain accessible through raw objects,
linkages, and auxiliary records. New semantic mappings require another
controlled observation and a malformed-input regression before this ledger is
expanded.
