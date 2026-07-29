# ezdgn

`ezdgn` is a native, read-only V8 and read/write V7 DGN toolkit for Python,
implemented with an independently written Rust core and PyO3 bindings. It has
no ODA SDK, binary, or runtime dependency.

## Installation

`ezdgn` requires Python 3.10 or newer.

```bash
python -m pip install ezdgn
```

Install the optional Matplotlib renderer when preview images are needed:

```bash
python -m pip install "ezdgn[plot]"
```

Building from source requires Rust 1.83 or newer. The extension uses Python's
stable ABI (`abi3`) with a Python 3.10 minimum.

## Supported scope

| Format and operation | Support |
| --- | --- |
| V7/ISFF 2D read | Native entities, hierarchy, metadata, linkages, and raw records |
| V7/ISFF 2D write | Standalone or custom-seed creation of common primitives |
| V7/ISFF 3D | Signature, raw record, and common-header inspection only |
| V8 DGN read | Native 2D/3D models, common geometry, hierarchy, linkages, auxiliary data, and lossless raw objects |
| V8 DGN write/edit | Not supported |

The V7 reader decodes line, line string, shape, curve, ellipse, arc, text,
cell, text node, complex chain/shape, and B-spline records as native entities.
It restores parent/child relationships without flattening component records,
decodes typed attribute linkages while retaining their exact bytes, applies
high-precision sub-UOR coordinate corrections alongside the stored integers,
and resolves outline/fill colors through the active color table. Every record,
including unsupported control and application elements, retains its original
bytes.

The writer creates standalone V7 2D files from a bundled empty seed. A custom
seed can be supplied to preserve its units, origin, and design plane. It writes
line, line string, shape, curve, ellipse, arc, circle-as-ellipse, and raw-byte
text entities with basic symbology and shape fill linkage. Coordinates outside
the selected seed's design plane are rejected instead of silently clipped.

The native V8 reader decodes model metadata, line, line string, shape, curve,
point string, text, ellipse, arc, text node, complex chain/shape, cell,
shared-cell instance, B-spline curve/poles, and a bounded dimension anchor.
Unknown graphical, control, name-table, linkage, and auxiliary records remain
addressable with exact bytes. Product-specific custom-object semantics, full
dimension annotation semantics, V8 writing/editing, and 3D screen projection
for plotting remain outside the supported scope.

V7 3D geometry is not supported. The V7 writer does not yet create cells,
complex elements, B-splines, arbitrary linkages, or perform in-place editing.
The raw V7 record framing is shared by 2D and 3D files, so `scan_records()` can
inspect a 3D stream safely without implying V7 3D entity support.

## 2D entity API

```python
import ezdgn

drawing = ezdgn.readfile("drawing.dgn")

# All records remain ordered and lossless. entities contains only top-level
# graphics; all_entities also includes drawable component records.
print(len(drawing.elements), len(drawing.entities), len(drawing.all_entities))

for entity in drawing:
    print(entity.dxftype(), entity.level, entity.style)

for line in drawing.query("LINE"):
    print(line.start_uor, line.end_uor)
    print(line.start_master, line.end_master)

for text in drawing.query("TEXT"):
    print(text.text_bytes)
    print(text.decode_text())  # ASCII with strict errors by default
    print(text.decode_text("cp932", errors="replace"))

for cell in drawing.query("CELL"):
    print(cell.name, cell.origin_master, cell.transform)
    for component in drawing.children(cell):
        print("  ", component.dxftype())

for element in drawing.elements:
    for linkage in element.linkages:
        print(linkage.kind, linkage.linkage_type_name, linkage.raw_bytes)
```

`drawing.entities` contains supported top-level drawable entities only.
`drawing.all_entities` is a flat tuple in original `drawing.elements` order:
it includes both `Cell`/`ComplexChain`/`ComplexShape` container headers and
their supported drawable descendants at every depth. It excludes control
records, `UnsupportedElement` instances, and non-drawable B-spline support
records; those remain available through the lossless `drawing.elements`
sequence. `drawing.unsupported_elements` provides a filtered diagnostic view
for unknown kinds. Use `parent()`, `children()`, or `descendants()` when a tree
view is needed.

Ellipse, arc, curve, and B-spline entities retain their native parameters or
control records; they are not flattened to polylines. `parent_index` and
`child_indices` refer to the lossless `drawing.elements` sequence, while
`drawing.parent()`, `children()`, and `descendants()` resolve the objects.
Stored integer UOR, sub-UOR-corrected floating UOR, and optional master-unit
coordinates coexist. `drawing.color_table` is the last type-5, level-1 color
table in file order. `drawing.resolve_color()`, `entity.style.rgb`, and
`fill_rgb` use that table when present and otherwise fall back to the standard
MicroStation V7 256-color palette.

V7 DGN does not record a text code page. `Text.decode_text()` therefore uses
`encoding="ascii", errors="strict"` as a deterministic default; pass the
project encoding explicitly for non-ASCII text.

Known DMRS/database, association ID, shape fill, and high-precision linkages
have typed fields. Unknown user linkages and malformed trailing attribute bytes
remain accessible through read-only raw views. Shared-cell definition/instance
types 34/35 remain raw because the public ISFF chapter does not specify their
layout.

The high-level `read()`/`readfile()` API deliberately rejects V7 3D files.
`scan_records()` and `inspect_headers()` still support bounded inspection of
their shared record framing and metadata.

## Native V8 API

Use `open_document()` when a caller may receive either format. The existing
`read()` and `readfile()` names remain V7-specific for compatibility.

```python
import ezdgn

document = ezdgn.open_document("drawing.dgn")
if isinstance(document, ezdgn.V8Document):
    for model in document.models:
        print(
            model.metadata.name,
            model.metadata.dimension,
            model.metadata.master_unit,
        )
        for entity in model.entities:
            print(entity.dxftype(), entity.level, entity.common.color_index)

        for text in model.query("TEXT"):
            print(text.data.text, text.data.text_bytes)

        for cell in model.query("CELL"):
            for component in model.children(cell):
                print("  ", component.dxftype())
```

`V8Model.elements` is the complete graphical-object sequence.
`V8Model.entities` is the feature-oriented view: complex/cell headers remain
native aggregate entities, while TextNode headers yield their text children.
`parent()`, `children()`, and `descendants()` navigate the original hierarchy.
2D and 3D coordinates coexist as `(x, y, z)` tuples; no Z coordinate is dropped.
For complex headers, `common.stored_dimension` retains the header bit and
`common.dimension` reports the effective dimension inherited from children.

The raw scanner is available independently of semantic decoding:

```python
raw = ezdgn.scan_v8_objects("drawing.dgn")
for model in raw.models:
    for obj in model.graphical_objects:
        print(
            obj.stream_path,
            obj.inflated_offset,
            obj.element_type,
            obj.role,
            obj.raw_bytes,
        )
```

Every extraction and decode stage is bounded. Override the defaults as one
immutable policy object when processing untrusted files:

```python
limits = ezdgn.V8ScanLimits(
    max_file_size=256 * 1024 * 1024,
    max_objects=250_000,
    max_total_inflated_bytes=512 * 1024 * 1024,
)
document = ezdgn.read_v8("drawing-v8.dgn", limits=limits)
```

Malformed structure raises `InvalidDgnError`; configured resource ceilings
raise `DgnLimitError`. The complete clean-room boundary, evidence ledger, and
known limitations are recorded in [docs/v8/SCOPE.md](docs/v8/SCOPE.md),
[docs/v8/FORMAT_NOTES.md](docs/v8/FORMAT_NOTES.md), and
[docs/v8/PROVENANCE.md](docs/v8/PROVENANCE.md).

## Plotting parsed drawings

The optional renderer can display a parsed V7 2D drawing or a V8 document and
save it as an image without changing the native entity model:

```python
import ezdgn

drawing = ezdgn.readfile("drawing.dgn")

figure, axes = ezdgn.plot(
    drawing,
    text_encoding="cp932",
    background="#111111",
)
figure.savefig("preview.png", dpi=150, bbox_inches="tight")

# Or render and save in one call.
ezdgn.save_plot(drawing, "preview.png", text_encoding="cp932")

# V8 uses already decoded native text and preserves Z in the object model;
# the preview is an explicit XY projection.
ezdgn.save_plot(ezdgn.read_v8("drawing-v8.dgn"), "preview-v8.png")
```

The equivalent CLI command is:

```bash
ezdgn plot drawing.dgn -o preview.png --encoding cp932
```

Omit `-o` to open an interactive Matplotlib window. Use `--monochrome` for a
high-contrast preview, `--hide-text` to suppress text, or
`--coordinate-space uor` when master-unit coordinates are unavailable. Run
`ezdgn plot --help` for the complete option list.

Lines, line strings, shapes, ellipses, arcs, text, and drawable components of
cells and complex elements are rendered. Ellipses and arcs are sampled only
for display. Native type-11 curves and B-spline curves are previewed from their
parsed control sequences; the source records and entity parameters are never
flattened or modified. V8 3D geometry uses an XY preview projection, and
orientation matrices are retained but not applied by the 2D renderer. V7 text
does not store its code page, so the caller must select the correct encoding
for non-ASCII text. Geometry and text with compatible display styles are
batched to keep large previews practical.

## V7 writer and optional custom seeds

```python
import ezdgn

doc = ezdgn.new()  # uses the bundled empty V7 2D seed
msp = doc.modelspace()

msp.add_line(
    (0, 0),
    (10, 5),
    dgnattribs={"level": 2, "color": 3, "line_weight": 2},
)
msp.add_line_string([(0, 10), (5, 15), (10, 10)])
msp.add_shape(
    [(20, 0), (30, 0), (30, 10), (20, 10)],
    fill_color=6,
)
msp.add_ellipse((25, 25), primary_axis=5, secondary_axis=3, rotation=30)
msp.add_arc(
    (40, 5),
    primary_axis=5,
    secondary_axis=3,
    start_angle=30,
    sweep_angle=120,
)
msp.add_text("日本語", (0, 30), height=2, encoding="cp932")

doc.saveas("drawing.dgn")
roundtrip = doc.readback()
```

Pass a custom seed as `ezdgn.new("project_seed.dgn")` to use its units, origin,
design plane, and active color table. `new()` copies the mandatory TCB,
digitizer setup, level symbology, and last active color table from the selected
seed. Set `copy_seed_elements=True` to retain every seed record, including any
graphics. Text encoding is not recorded by V7 DGN, so `add_text()` accepts
bytes directly or uses its `encoding="ascii", errors="strict"` defaults for
`str` input.

## Raw record API

```python
import ezdgn

info = ezdgn.detect_format("drawing.dgn")
print(info.kind, info.dimension)

scan = ezdgn.scan_records("drawing.dgn")
print(len(scan.records), scan.termination)

for record in scan.records:
    print(record.offset, record.element_type, record.level, record.raw_bytes)
```

The `V8_CFB` result means that the input has the generic CFB signature used by
V8 DGN files. It is intentionally described as a candidate because the outer
signature alone does not prove that DGN-specific streams are present.

The bounded container inspector is the structural-only entry point. It verifies
the known DGN root markers without decoding DGN stream contents:

```python
container = ezdgn.inspect_v8_container("drawing-v8.dgn")
print(container.has_dgn_v8_markers)
print(container.model_storage_paths)
for entry in container.entries:
    print(entry.path, entry.kind, entry.size_bytes)
```

`inspect_v8_container()` alone is structural identification, not an entity or
fidelity claim. Use `scan_v8_objects()` for bounded, exact raw objects and
`read_v8()` or `open_document()` for native semantic decoding. The legacy
`read()`, `readfile()`, and `scan_records()` contracts remain V7-specific and
therefore reject V8 input. No V8 path silently converts to V7 or flattens native
objects.

## Design settings and common headers

```python
import ezdgn

headers = ezdgn.inspect_headers("drawing.dgn")
settings = headers.design_settings

print(settings.master_unit_name, settings.uor_per_master)
print(settings.global_origin_master)

for element in headers.elements:
    common = element.common_header
    if common is not None:
        print(
            element.record.element_type,
            common.range.low_master,
            common.range.high_master,
            common.symbology.color,
        )
```

`read_design_settings()` decodes only the leading TCB. `inspect_headers()`
pairs every raw record with its standard common header when that element type
has one. Attribute bytes remain available as a read-only zero-copy
`ElementMetadata.attribute_view`.

The same inspection is available from the CLI:

```bash
ezdgn inspect drawing.dgn
ezdgn inspect drawing.dgn --records --json
ezdgn inspect drawing.dgn --headers --json
ezdgn inspect drawing.dgn --entities --json
ezdgn inspect drawing-v8.dgn --json
```

## Development

```bash
python -m venv .venv
. .venv/bin/activate
python -m pip install "maturin>=1.13,<2" "pytest>=8" "matplotlib>=3.8"
maturin develop
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
python -m pytest
sha256sum -c tests/data/dgn/SHA256SUMS
uv lock --check
```

Run the bounded V8 fuzz target and parser microbenchmark separately:

```bash
cargo +nightly fuzz run v8_read -- -max_len=16777216
EZDGN_BENCH_ITERATIONS=100 cargo bench -p ezdgn-core --bench v8_read
```

Build a distributable wheel with:

```bash
maturin build --release --out dist
```

## License

`ezdgn` is released under the [MIT License](LICENSE). The bundled empty seed
and test fixtures retain the separate upstream terms documented in
[`src/ezdgn/_data/README.md`](src/ezdgn/_data/README.md) and
[`tests/data/dgn/README.md`](tests/data/dgn/README.md).
