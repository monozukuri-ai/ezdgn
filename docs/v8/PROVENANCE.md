# Native V8 research provenance

This log records the evidence allowed to influence the native V8 reader. It is
an engineering provenance record, not legal advice.

## Implementation policy

The parser is independently implemented in Rust. ODA source code, SDK headers,
generated wrappers, binaries, debugger output, and decompiled material are not
used. No ODA component is a build, test, or runtime dependency.

Fixed output that was already published with an open-source upstream fixture
may be used as a black-box expected result when its origin is recorded. Such
output can validate observable geometry and attributes, but it cannot establish
the layout or meaning of bytes in a DGN stream.

## Current evidence

### GDAL fixture pair

- Binary: `tests/data/dgn/v8/test_dgnv8.dgn`
- Expected output: `tests/data/dgn/v8/test_dgnv8_ref.csv`
- Upstream: OSGeo/GDAL commit
  `18e7cceb43a0dd58be474c9fdd5384baa3cde7c9`
- Acquisition and license record: `tests/data/dgn/README.md` and
  `tests/data/dgn/LICENSE.GDAL.txt`
- Integrity record: `tests/data/dgn/SHA256SUMS` and
  `tests/data/dgn/v8/manifest.json`

The CSV was produced upstream through a GDAL build with a DGN V8 backend. It is
used only to check externally visible feature geometry and attributes. It does
not preserve every control object, native curve parameter, cell component, or
complex-element relationship and therefore is not a source-semantic oracle.

### Constructed tag file

- Binary: `tests/data/dgn/v8/constructed_tags.dgn`, 7,680 bytes, SHA-256
  `407f7dd8f5093646d4f27735d92b45c13319e08c8b1471a49d09e693160f2261`
- Origin: written by this repository's own test builder from made-up values
  (`write_the_constructed_tag_fixture` in
  `crates/ezdgn-core/src/v8/entity.rs`). It is synthetic. It is not derived
  from any real file, and it was not made or read by an authoring
  application.
- Content: one 2D model with a shared-cell instance, three tag elements, and
  type-37 objects that are not read as tags; one tag set and one shared-cell
  definition with a tag in the named pages.
- Use: the Python tests and the stream tests read it, so that the path from
  the Rust reader to the Python objects is tested for a tag in every test
  run. The expected values are the inputs of the builder. It is not evidence
  for any format rule.
- Reproducible: the bytes depend on the input of the builder alone. Each
  stream is a zlib stream of stored blocks that the builder writes by hand,
  so no compression library takes part. Each storage carries the fixed time
  1970-01-01 00:00:00 UTC, and the root and the streams carry no time, so no
  clock takes part. The default test run builds the file in memory and
  compares it byte for byte with the checked-in file
  (`the_checked_in_tag_file_is_what_the_builder_writes_byte_for_byte`). The
  layout of the container comes from the `cfb` crate at the version in
  `Cargo.lock` (0.14.0). A later version of that crate can lay the same
  content out in another way; that test then fails and says how to write the
  file again.

### Public CFB structure

The CFB container is read through the Rust `cfb` crate. Generic CFB sector,
directory, and stream behavior is treated separately from inferred DGN stream
semantics.

### Independent public V8 verification files

Two public support-example attachments were downloaded directly from Safe
Software on 2026-07-29. They are used only as non-committed interoperability
inputs and are not redistributed by this repository.

#### Water distribution mains

- Context page: [Handling MicroStation DGN Item Types with FME](https://support.safe.com/hc/en-us/articles/36770363401613-Handling-Microstation-DGN-Item-Types-with-FME)
- Direct attachment: [Water_distribution_mains.dgn](https://support.safe.com/hc/article_attachments/36769975369869)
- Bytes: 3,840,000
- SHA-256: `f34bbb1b408b583d9cfc9d280e4d5782559cfa824b4b51210a03cf5c88235b64`
- Native observation: one 2D `Default` / `Master Model`, 10 UOR/master,
  master/sub units `m` / `m`, 66,053 graphical objects and feature entities,
  comprising 18 lines and 66,035 line strings. All 66,053 associated auxiliary
  records remain addressable.
- Item data observation (2026-11-10, same bytes and hash): each of the 66,053
  records holds two entries of type `0x56ff0000`, and all 132,106 decode as
  item instances (`WaterMain` and `Metadata`). The file holds nine more
  auxiliary records, on named objects, with 12 entries of other types;
  `FORMAT_NOTES.md` counts all 66,062 records and 132,118 entries. This file
  is the public evidence for the entry layout and the item instance layout in
  `FORMAT_NOTES.md`. It does not exercise the UTF-16 string form, one-byte
  strings above `0x7f`, or a set bit 31 in the entry length word;
  `FORMAT_NOTES.md` states the evidence for those.
  `tests/python/test_v8_item_data.py` reads the file when
  `EZDGN_PUBLIC_V8_DIR` names a folder that holds it and skips otherwise.

#### Tree text-node labels with tags

- Context page: [Reading MicroStation DGN Tags with FME](https://support.safe.com/hc/en-us/articles/25407424644749-Reading-MicroStation-DGN-Tags-with-FME)
- Direct attachment: [TreeTextNodeLabelsWithTags.dgn](https://support.safe.com/hc/article_attachments/36260727163917)
- Bytes: 847,360
- SHA-256: `8439e6d3a18227122c10c5367799561ebe94737d9d27932f4eb89d743b108270`
- Native observation: one 2D `Default` / `Master Model`, 10 UOR/master,
  master/sub units `m` / `m`, 33,335 graphical objects, 6,904 text-node
  headers, 12,623 text children in the feature view, and 13,808 unknown raw
  objects.
- Tag observation (2026-11-10, same bytes and hash): the 13,808 objects are
  type-37 tag elements and all decode, 6,904 with a text value and 6,904 with
  an integer value. The one tag set of the file decodes with two definitions,
  and every tag names it, an ID defined in it, and a text-node header as its
  host. This file is the public evidence for the tag element and tag set
  layouts in `FORMAT_NOTES.md`. It does not exercise text bytes above `0x7f`,
  tags without a host, tags inside shared-cell definitions, or the origin of
  a tag; `FORMAT_NOTES.md` states the evidence for those.
  `tests/python/test_v8_tags.py` reads the file when `EZDGN_PUBLIC_V8_DIR`
  names a folder that holds it and skips otherwise.

The descriptions and supplied application context are not treated as byte
layout documentation. The files demonstrate that the model/page/object rules
and text-node feature policy work beyond the GDAL baseline. Unknown custom
data remains raw unless independently characterized.

### Real-world files that are not redistributed

Every layout that this log adds on 2026-11-10 was also run on 90 real-world
production files held by a contributor (85 2D files and five 3D files
converted from DWG): the auxiliary entries, the item instances, the stored
quaternion of 3D text, the shared-cell definitions with the match and the
placement of an instance, and the tag elements and tag sets.

These files are highly confidential. They cannot be shared, they are not
part of this repository, and no byte, name or text from them is recorded
here. Only counts are reported.

Most rules have public evidence too, from the checked-in fixture or from the
two public files above. These rules have none:

- item data: the `fb` UTF-16 string form, and bytes `0x80` and above in the
  `fa` form;
- auxiliary entries: bit 31 of the length word being set;
- 3D text: the order of the four quaternion values for a text that is turned;
- shared cells: the row order of the nine matrix values of an instance, and
  the match by name and range values where definitions share a name; and
- tags: text bytes above `0x7f` with the code page word, bytes after the
  value, empty values and prompts, tags in shared-cell definitions, tags
  without a host or with a shared-cell instance as host, and the origin.

Each of them is listed in `FORMAT_NOTES.md` under "Rules that rest on
confidential files", with how the rule was found, the counts, and the
constructed test that covers it. The method was the same as for the public
files: read the bytes of lawfully held files, state a rule, and count where
it holds and where it does not. Most rules were also checked against
something that the reader already decodes in the same file; the table says
which rows have no such check. A reviewer cannot reproduce those counts.
Each such rule can be taken out, and its form left raw, if that is
preferred.

### Repository observations

Facts produced by `inspect_v8_container()` and the repository-owned dump,
object-scan, and semantic-read examples may be recorded as observations. Field
names and meanings stay raw or provisional until reproduced with independent
fixtures and protected by malformed-input tests.

## Fixture admission checklist

Before adding another V8 binary, record:

- who supplied or generated it and under what redistribution terms;
- the creating application and version when known;
- whether it is synthetic, sanitized, or production-derived;
- expected model count, dimensions, and deliberately varied properties;
- file size and SHA-256; and
- whether expected semantic output came from manual inspection, an authoring
  application, or another parser.

Production files containing customer or project information must not be added
without explicit permission and sanitization.
