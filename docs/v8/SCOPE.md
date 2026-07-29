# Native DGN V8 reader scope

This document defines the support claim and release gates for the native DGN
V8 reader. The implementation is independently written Rust code and does not
link to, load, or invoke the Open Design Alliance SDK or binaries.

## Implemented read boundary

The reader is a bounded, read-only pipeline:

1. validate the Compound File Binary (CFB) container and DGN root markers;
2. extract logical streams under per-stream and aggregate limits;
3. inflate complete zlib payloads with a hard output ceiling;
4. parse the model index, model storages, object pages, and auxiliary pages;
5. retain every recognized raw object as an addressable exact byte sequence;
6. decode model metadata, standard graphical elements, hierarchy, linkages,
   and auxiliary-record associations; and
7. expose the result through Rust, Python, CLI inspection, and an explicit XY
   plotting adapter.

The semantic model currently covers:

- 2D and 3D line, line string, shape, curve, and point string geometry;
- text with its decoded value, original encoded bytes, origin, orientation,
  height, and width;
- ellipse and arc parameters without flattening the source object;
- text node, complex chain, complex shape, cell, and shared-cell instance
  hierarchy;
- B-spline curve headers and pole sequences;
- a bounded anchor for dimension objects;
- common element identity, range, level, graphic group, properties,
  symbology, and stored/effective dimension;
- model identity, name, description, dimension, units, UOR scale, origin, and
  extents;
- property and unknown linkages with exact bytes; and
- unknown graphical, control, named, linkage, and auxiliary records through
  the raw model.

`V8Model.elements` preserves the complete graphical-object order.
`V8Model.entities` is a feature-oriented view: aggregate headers remain one
feature, component objects remain navigable as children, and a text-node header
yields its drawable text child. No 3D coordinate is silently projected or
dropped by the reader.

## Explicit exclusions

This milestone does not support:

- V8 creation, writing, editing, or round-trip serialization;
- complete dimension annotation semantics;
- full B-spline knot/weight/surface semantics;
- shared-cell definition lookup and transformed instance expansion;
- product-specific custom objects, schemas, Item Types, business properties,
  or arbitrary XAttribute payload semantics;
- font resource resolution or application-specific display fidelity; or
- camera-aware 3D rendering. The optional plotter is documented as an XY
  preview and leaves the native Z coordinates unchanged.

Unsupported semantics remain visible as typed partial data or exact raw bytes.
They are never represented as a more specific supported primitive merely to
make a file appear fully decoded.

## Clean-room constraints

- ODA source, headers, generated bindings, SDK binaries, runtime tools,
  debugger output, and decompiled material are not implementation inputs.
- The checked-in GDAL/ODA-derived CSV is a fixed black-box output oracle. It
  validates observable feature results but does not describe the on-disk
  layout.
- Format rules must be reproducible from legally obtained files, public
  container documentation, and repository-owned inspection tools.
- `FORMAT_NOTES.md` records the byte-level evidence used by the decoder.
- A rule observed in only one file is not silently generalized. Ambiguous
  records remain raw or partially typed.

## Public APIs and compatibility

Rust callers use `scan_v8_objects()` for the raw model and `read_v8()` for the
semantic model. Python exposes the same operations plus `read_v8file()`.
`open_document()` and `openfile()` dispatch between V7 and V8 without changing
the legacy contracts.

`read()`, `readfile()`, `scan_records()`, and `inspect_headers()` remain V7
record APIs and intentionally reject V8/CFB input. This preserves existing V7
types, iteration behavior, and error boundaries.

## Release gates

A release claiming this scope must pass all of the following:

- the checked-in V8 fixture reproduces its stream manifest, raw counts, model
  metadata, Unicode text, hierarchy, feature type set, and 19 2D / 15 3D
  feature distribution;
- at least one independently sourced V8 file parses without fixture-specific
  offsets, with the source and hash recorded in `PROVENANCE.md`;
- malformed page, object, model-index, auxiliary-record, compressed-stream,
  hierarchy, vertex, string, and resource-limit cases fail deterministically
  without panic;
- the bounded fuzz target builds and the parser benchmark runs;
- all Rust and Python tests, fixture integrity checks, formatting, and clippy
  checks pass;
- a release wheel contains the Python package and native extension, advertises
  the intended Python/ABI metadata, installs into a clean environment, and
  reads both the checked-in V8 fixture and a V7 fixture; and
- the V7 read/write surface remains backward compatible.

The evidence inventory and byte-level rules are maintained in
[`PROVENANCE.md`](PROVENANCE.md) and [`FORMAT_NOTES.md`](FORMAT_NOTES.md).
