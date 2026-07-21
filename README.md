# ezdgn

`ezdgn` is a native V7 DGN reader and seed-based writer for Python,
implemented with a pure Rust core and PyO3 bindings.

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
| V7/ISFF 2D write | Seed-based creation of common primitive entities |
| V7/ISFF 3D | Signature, raw record, and common-header inspection only |
| V8 DGN | CFB container identification and directory inspection only |

The V7 reader decodes line, line string, shape, curve, ellipse, arc, text,
cell, text node, complex chain/shape, and B-spline records as native entities.
It restores parent/child relationships without flattening component records,
decodes typed attribute linkages while retaining their exact bytes, applies
high-precision sub-UOR coordinate corrections alongside the stored integers,
and resolves outline/fill colors through the active color table. Every record,
including unsupported control and application elements, retains its original
bytes.

The writer creates standalone V7 2D files from a caller-supplied seed while
preserving its units, origin, and design plane. It writes line, line string,
shape, curve, ellipse, arc, circle-as-ellipse, and raw-byte text entities with
basic symbology and shape fill linkage. Coordinates outside the seed's design
plane are rejected instead of silently clipped.

V8 entity semantics and V7 3D geometry are not supported. The writer does not
yet create cells, complex elements, B-splines, arbitrary linkages, or perform
in-place editing. The raw V7 record framing is shared by 2D and 3D files, so
`scan_records()` can inspect a 3D stream safely without implying 3D entity
support.

## 2D entity API

```python
import ezdgn

drawing = ezdgn.readfile("drawing.dgn")

# All records remain ordered and lossless. entities contains only top-level
# graphics; all_entities also includes drawable component records.
print(len(drawing.elements), len(drawing.entities), len(drawing.all_entities))

for entity in drawing:
    print(entity.dxftype(), entity.record.level, entity.style)

for line in drawing.query("LINE"):
    print(line.start_uor, line.end_uor)
    print(line.start_master, line.end_master)

for text in drawing.query("TEXT"):
    print(text.text_bytes)
    print(text.decode_text("cp932"))  # the caller selects the encoding

for cell in drawing.query("CELL"):
    print(cell.name, cell.origin_master, cell.transform)
    for component in drawing.children(cell):
        print("  ", component.dxftype())

for element in drawing.elements:
    for linkage in element.linkages:
        print(linkage.kind, linkage.linkage_type_name, linkage.raw_bytes)
```

Ellipse, arc, curve, and B-spline entities retain their native parameters or
control records; they are not flattened to polylines. `parent_index` and
`child_indices` refer to the lossless `drawing.elements` sequence, while
`drawing.parent()`, `children()`, and `descendants()` resolve the objects.
Stored integer UOR, sub-UOR-corrected floating UOR, and optional master-unit
coordinates coexist. `drawing.color_table` is the last type-5, level-1 color
table in file order; `entity.style.rgb` and `fill_rgb` are resolved from it
when present.

Known DMRS/database, association ID, shape fill, and high-precision linkages
have typed fields. Unknown user linkages and malformed trailing attribute bytes
remain accessible through read-only raw views. Shared-cell definition/instance
types 34/35 remain raw because the public ISFF chapter does not specify their
layout.

The high-level `read()`/`readfile()` API deliberately rejects V7 3D files.
`scan_records()` and `inspect_headers()` still support bounded inspection of
their shared record framing and metadata.

## Plotting parsed drawings

The optional renderer can display a parsed V7 2D drawing or save it as an
image without changing the native entity model:

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
for display. Native type-11 curves and B-spline curves are previewed from
their parsed control sequences; the source records and entity parameters are
never flattened or modified. V7 text does not store its code page, so the
caller must select the correct encoding for non-ASCII text. Geometry and text
with compatible display styles are batched to keep large previews practical.

## Seed-based V7 writer

```python
import ezdgn

doc = ezdgn.new("seed_2d.dgn")
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

By default `new()` copies the mandatory TCB, digitizer setup, level symbology,
and the last active color table from the seed. Set `copy_seed_elements=True`
to retain every existing seed record, including any graphics. Text encoding is
not recorded by V7 DGN, so `add_text()` accepts bytes directly or requires the
caller-selected encoding for `str` input.

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

The bounded container inspector verifies the known DGN root markers without
decoding proprietary V8 stream contents:

```python
container = ezdgn.inspect_v8_container("drawing-v8.dgn")
print(container.has_dgn_v8_markers)
print(container.model_storage_paths)
for entry in container.entries:
    print(entry.path, entry.kind, entry.size_bytes)
```

This is structural identification, not V8 entity support or a fidelity
guarantee. `ezdgn.read()`, `readfile()`, and `scan_records()` reject V8 input
instead of silently flattening or converting it. If a workflow converts V8 to
V7 outside `ezdgn`, validate the resulting geometry, text, levels, styles, and
complex/cell relationships before treating it as equivalent to the source.

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
cargo test --workspace
python -m pytest
```

Build a distributable wheel with:

```bash
maturin build --release --out dist
```

## License

`ezdgn` is released under the [MIT License](LICENSE). Test fixtures retain the
separate upstream terms documented in
[`tests/data/dgn/README.md`](tests/data/dgn/README.md).
