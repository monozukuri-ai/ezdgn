"""Command-line entry point for ezdgn."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Sequence

from . import (
    Arc,
    AttributeLinkage,
    BSplineCurve,
    BSplineKnot,
    BSplinePole,
    BSplineSurface,
    BSplineSurfaceBoundary,
    BSplineWeight,
    Cell,
    ColorTable,
    ComplexElement,
    Curve,
    DgnElement,
    DgnError,
    Ellipse,
    Line,
    LineString,
    Shape,
    Text,
    TextNode,
    __version__,
    detect_format,
    inspect_headers,
    inspect_v8_container,
    read,
)
from .metadata import CommonElementHeader, DesignSettings


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="ezdgn",
        description="Read, inspect, and preview DGN files.",
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"%(prog)s {__version__}",
    )
    commands = parser.add_subparsers(dest="command")
    inspect_parser = commands.add_parser(
        "inspect", help="Detect a DGN file and summarize its container or records"
    )
    inspect_parser.add_argument("path", type=Path)
    inspect_parser.add_argument(
        "--records",
        action="store_true",
        help="Include per-record metadata (raw bytes are omitted)",
    )
    inspect_parser.add_argument(
        "--headers",
        action="store_true",
        help="Include decoded common headers (implies --records)",
    )
    inspect_parser.add_argument(
        "--entities",
        action="store_true",
        help="Include decoded V7 2D entities and hierarchy (implies --headers)",
    )
    inspect_parser.add_argument(
        "--json",
        action="store_true",
        help="Emit machine-readable JSON",
    )
    plot_parser = commands.add_parser(
        "plot", help="Render a parsed V7 2D drawing with Matplotlib"
    )
    plot_parser.add_argument("path", type=Path)
    plot_parser.add_argument(
        "-o",
        "--output",
        type=Path,
        help="Write an image such as preview.png (show a window if omitted)",
    )
    plot_parser.add_argument(
        "--show",
        action="store_true",
        help="Show the preview window after writing --output",
    )
    plot_parser.add_argument(
        "--coordinate-space",
        choices=("master", "uor"),
        default="master",
        help="Plot master-unit or raw UOR coordinates (default: master)",
    )
    plot_parser.add_argument(
        "--encoding",
        default="cp1252",
        help="Decode text using this encoding (default: cp1252)",
    )
    plot_parser.add_argument(
        "--background",
        default="#111111",
        help="Matplotlib background color (default: #111111)",
    )
    plot_parser.add_argument(
        "--monochrome",
        action="store_true",
        help="Use a contrasting monochrome foreground",
    )
    plot_parser.add_argument(
        "--hide-text",
        action="store_true",
        help="Do not draw text elements",
    )
    plot_parser.add_argument(
        "--hide-axes",
        action="store_true",
        help="Hide axes and unit labels",
    )
    plot_parser.add_argument(
        "--dpi",
        type=_positive_int,
        default=150,
        help="Output resolution in dots per inch (default: 150)",
    )
    return parser


def _positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return parsed


def _inspect(
    path: Path,
    *,
    include_records: bool,
    include_headers: bool,
    include_entities: bool,
) -> dict[str, Any]:
    format_info = detect_format(path)
    result: dict[str, Any] = {
        "path": str(path),
        "format": format_info.kind,
        "dimension": format_info.dimension,
    }
    if not format_info.is_v7:
        container = inspect_v8_container(path)
        result["record_scan_supported"] = False
        result["v8_read_policy"] = "external_conversion_required"
        result["v8_container"] = {
            "cfb_version": container.cfb_version,
            "has_dgn_v8_markers": container.has_dgn_v8_markers,
            "missing_markers": container.missing_markers,
            "model_storage_paths": container.model_storage_paths,
            "entry_count": len(container.entries),
            "storage_count": container.storage_count,
            "stream_count": container.stream_count,
        }
        return result

    if include_entities:
        drawing = read(path)
        scan = drawing.raw_scan
        settings = drawing.design_settings
        elements = drawing.elements
    else:
        header_scan = inspect_headers(path)
        scan = header_scan.raw_scan
        settings = header_scan.design_settings
        elements = header_scan.elements
    result.update(
        {
            "record_scan_supported": True,
            "record_count": len(scan.records),
            "termination": scan.termination,
            "end_offset": scan.end_offset,
            "trailing_bytes": scan.trailing_bytes,
            "source_size": scan.source_size,
            "design_settings": _settings_payload(settings),
        }
    )
    if include_entities:
        result["entity_count"] = len(drawing.entities)
        result["active_color_table_index"] = drawing.active_color_table_index
    if include_records or include_headers or include_entities:
        records = []
        for element in elements:
            record = element.record
            record_payload: dict[str, Any] = {
                "index": record.index,
                "offset": record.offset,
                "level": record.level,
                "element_type": record.element_type,
                "complex_component": record.complex_component,
                "reserved": record.reserved,
                "deleted": record.deleted,
                "words_to_follow": record.words_to_follow,
                "size_bytes": record.size_bytes,
            }
            if include_headers or include_entities:
                record_payload["common_header"] = (
                    None
                    if element.common_header is None
                    else _common_header_payload(element.common_header)
                )
            if include_entities:
                record_payload["entity"] = _entity_payload(element)
                record_payload["parent_index"] = element.parent_index
                record_payload["child_indices"] = element.child_indices
                record_payload["linkages"] = [
                    _linkage_payload(linkage) for linkage in element.linkages
                ]
            records.append(record_payload)
        result["records"] = records
    return result


def _settings_payload(settings: DesignSettings) -> dict[str, Any]:
    return {
        "dimension": settings.dimension,
        "subunits_per_master": settings.subunits_per_master,
        "uor_per_subunit": settings.uor_per_subunit,
        "uor_per_master": settings.uor_per_master,
        "scale": settings.scale,
        "master_unit": settings.master_unit_name,
        "master_unit_bytes": settings.master_unit_label.hex(),
        "sub_unit": settings.sub_unit_name,
        "sub_unit_bytes": settings.sub_unit_label.hex(),
        "global_origin_uor": settings.global_origin_uor,
        "global_origin_master": settings.global_origin_master,
    }


def _common_header_payload(header: CommonElementHeader) -> dict[str, Any]:
    properties = header.properties
    return {
        "range": {
            "low_uor": header.range.low_uor,
            "high_uor": header.range.high_uor,
            "low_master": header.range.low_master,
            "high_master": header.range.high_master,
        },
        "graphic_group": header.graphic_group,
        "attribute_index": header.attribute_index,
        "properties": {
            "raw": properties.raw,
            "class": properties.element_class,
            "reserved": properties.reserved,
            "locked": properties.locked,
            "new": properties.is_new,
            "modified": properties.modified,
            "has_attributes": properties.has_attributes,
            "screen_relative": properties.screen_relative,
            "planar": properties.is_planar,
            "snappable": properties.is_snappable,
            "h_bit": properties.h_bit,
        },
        "symbology": {
            "raw": header.symbology.raw,
            "style": header.symbology.style,
            "weight": header.symbology.weight,
            "color": header.symbology.color,
        },
        "attribute_offset": header.attribute_offset,
        "attribute_length": header.attribute_length,
    }


def _entity_payload(element: DgnElement) -> dict[str, Any]:
    payload: dict[str, Any] = {"kind": element.kind}
    if element.style is not None:
        payload["style"] = {
            "color_index": element.style.color_index,
            "rgb": element.style.rgb,
            "line_style": element.style.line_style,
            "line_weight": element.style.line_weight,
            "fill_color_index": element.style.fill_color_index,
            "fill_rgb": element.style.fill_rgb,
        }
    if isinstance(element, Cell):
        payload.update(
            {
                "total_length_words": element.total_length_words,
                "name": element.name,
                "name_words": element.name_words,
                "cell_class": element.cell_class,
                "levels": element.levels,
                "range_low_uor": element.range_low_uor,
                "range_high_uor": element.range_high_uor,
                "range_low_master": element.range_low_master,
                "range_high_master": element.range_high_master,
                "transform_raw": element.transform_raw,
                "transform": element.transform,
                "origin_uor": element.origin_uor,
                "origin_master": element.origin_master,
            }
        )
    elif isinstance(element, Line):
        payload.update(
            {
                "start_uor": element.start_uor,
                "end_uor": element.end_uor,
                "start_uor_precise": element.start_uor_precise,
                "end_uor_precise": element.end_uor_precise,
                "start_master": element.start_master,
                "end_master": element.end_master,
            }
        )
    elif isinstance(element, (LineString, Shape, Curve, BSplinePole)):
        payload.update(
            {
                "vertices_uor": element.vertices_uor,
                "vertices_uor_precise": element.vertices_uor_precise,
                "vertices_master": element.vertices_master,
            }
        )
    elif isinstance(element, TextNode):
        payload.update(
            {
                "total_length_words": element.total_length_words,
                "num_text_strings": element.num_text_strings,
                "node_number": element.node_number,
                "max_length": element.max_length,
                "max_used": element.max_used,
                "font_id": element.font_id,
                "justification": element.justification,
                "line_spacing_raw": element.line_spacing_raw,
                "line_spacing_master": element.line_spacing_master,
                "length_multiplier_raw": element.length_multiplier_raw,
                "height_multiplier_raw": element.height_multiplier_raw,
                "length_multiplier_master": element.length_multiplier_master,
                "height_multiplier_master": element.height_multiplier_master,
                "rotation_raw": element.rotation_raw,
                "rotation_degrees": element.rotation_degrees,
                "origin_uor": element.origin_uor,
                "origin_master": element.origin_master,
            }
        )
    elif isinstance(element, ComplexElement):
        payload.update(
            {
                "total_length_words": element.total_length_words,
                "num_elements": element.num_elements,
            }
        )
    elif isinstance(element, Ellipse):
        payload.update(
            {
                "center_uor": element.center_uor,
                "center_master": element.center_master,
                "primary_axis_uor": element.primary_axis_uor,
                "secondary_axis_uor": element.secondary_axis_uor,
                "primary_axis_master": element.primary_axis_master,
                "secondary_axis_master": element.secondary_axis_master,
                "rotation_raw": element.rotation_raw,
                "rotation_degrees": element.rotation_degrees,
            }
        )
    elif isinstance(element, Arc):
        payload.update(
            {
                "center_uor": element.center_uor,
                "center_master": element.center_master,
                "primary_axis_uor": element.primary_axis_uor,
                "secondary_axis_uor": element.secondary_axis_uor,
                "primary_axis_master": element.primary_axis_master,
                "secondary_axis_master": element.secondary_axis_master,
                "rotation_raw": element.rotation_raw,
                "rotation_degrees": element.rotation_degrees,
                "start_angle_raw": element.start_angle_raw,
                "start_angle_degrees": element.start_angle_degrees,
                "sweep_angle_raw": element.sweep_angle_raw,
                "sweep_angle_degrees": element.sweep_angle_degrees,
            }
        )
    elif isinstance(element, Text):
        payload.update(
            {
                "font_id": element.font_id,
                "justification": element.justification,
                "length_multiplier_raw": element.length_multiplier_raw,
                "height_multiplier_raw": element.height_multiplier_raw,
                "length_multiplier_master": element.length_multiplier_master,
                "height_multiplier_master": element.height_multiplier_master,
                "rotation_raw": element.rotation_raw,
                "rotation_degrees": element.rotation_degrees,
                "origin_uor": element.origin_uor,
                "origin_master": element.origin_master,
                "editable_fields": element.editable_fields,
                "text_hex": element.text_bytes.hex(),
            }
        )
    elif isinstance(element, BSplineSurface):
        payload.update(
            {
                "description_words": element.description_words,
                "curve_type": element.curve_type,
                "u_order": element.u_order,
                "u_properties": element.u_properties,
                "num_poles_u": element.num_poles_u,
                "num_knots_u": element.num_knots_u,
                "rule_lines_u": element.rule_lines_u,
                "v_order": element.v_order,
                "v_properties": element.v_properties,
                "num_poles_v": element.num_poles_v,
                "num_knots_v": element.num_knots_v,
                "rule_lines_v": element.rule_lines_v,
                "num_bounds": element.num_bounds,
            }
        )
    elif isinstance(element, BSplineSurfaceBoundary):
        payload.update(
            {
                "number": element.number,
                "vertices_raw": element.vertices_raw,
                "vertices_raw_precise": element.vertices_raw_precise,
                "vertices_uv": element.vertices_uv,
            }
        )
    elif isinstance(element, (BSplineKnot, BSplineWeight)):
        payload.update(
            {"values_raw": element.values_raw, "values": element.values}
        )
    elif isinstance(element, BSplineCurve):
        payload.update(
            {
                "description_words": element.description_words,
                "order": element.order,
                "properties": element.properties,
                "curve_type": element.curve_type,
                "num_poles": element.num_poles,
                "num_knots": element.num_knots,
            }
        )
    elif isinstance(element, ColorTable):
        payload.update(
            {
                "screen_flag": element.screen_flag,
                "colors": element.colors,
            }
        )
    return payload


def _linkage_payload(linkage: AttributeLinkage) -> dict[str, Any]:
    return {
        "offset": linkage.offset,
        "size": len(linkage.raw_view),
        "declared_size": linkage.declared_size,
        "type": linkage.linkage_type,
        "type_name": linkage.linkage_type_name,
        "kind": linkage.kind,
        "entity_number": linkage.entity_number,
        "mslink": linkage.mslink,
        "color_index": linkage.color_index,
        "association_id": linkage.association_id,
        "delta_words": linkage.delta_words,
        "deltas": linkage.deltas,
        "complete": linkage.is_complete,
        "raw_hex": linkage.raw_bytes.hex(),
    }


def _print_human(result: dict[str, Any]) -> None:
    print(f"path: {result['path']}")
    print(f"format: {result['format']}")
    if result["dimension"] is not None:
        print(f"dimension: {result['dimension']}D")
    print(
        "record scan supported: "
        f"{str(result['record_scan_supported']).lower()}"
    )
    if "v8_container" in result:
        container = result["v8_container"]
        print(
            "DGN V8 markers: "
            f"{str(container['has_dgn_v8_markers']).lower()}"
        )
        print(f"CFB version: {container['cfb_version']}")
        print(
            "CFB entries: "
            f"{container['entry_count']} "
            f"({container['storage_count']} storages, "
            f"{container['stream_count']} streams)"
        )
        print(f"model storages: {container['model_storage_paths']}")
        print("V8 read policy: external conversion required")
    if result["record_scan_supported"]:
        print(f"records: {result['record_count']}")
        print(f"termination: {result['termination']}")
        print(f"end offset: {result['end_offset']}")
        print(f"trailing bytes: {result['trailing_bytes']}")
        settings = result["design_settings"]
        print(
            "units: "
            f"{settings['uor_per_master']} UOR/"
            f"{settings['master_unit'] or settings['master_unit_bytes']}"
        )
        print(f"global origin (master): {settings['global_origin_master']}")
    for record in result.get("records", []):
        common = record.get("common_header")
        common_summary = ""
        if common is not None:
            common_summary = (
                f" range={common['range']['low_master']}.."
                f"{common['range']['high_master']}"
                f" color={common['symbology']['color']}"
            )
        entity_summary = ""
        if "entity" in record:
            entity_summary = f" entity={record['entity']['kind']}"
        print(
            "record "
            f"{record['index']}: offset={record['offset']} "
            f"type={record['element_type']} level={record['level']} "
            f"size={record['size_bytes']}"
            f"{common_summary}"
            f"{entity_summary}"
        )


def main(argv: Sequence[str] | None = None) -> int:
    parser = _build_parser()
    args = parser.parse_args(list(argv) if argv is not None else None)
    if args.command is None:
        parser.print_help()
        return 0

    try:
        if args.command == "plot":
            return _plot_command(args)
        result = _inspect(
            args.path,
            include_records=args.records,
            include_headers=args.headers,
            include_entities=args.entities,
        )
    except (ImportError, LookupError, OSError, ValueError, DgnError) as error:
        print(f"ezdgn: {error}", file=sys.stderr)
        return 1

    if args.json:
        print(json.dumps(result, ensure_ascii=False, separators=(",", ":")))
    else:
        _print_human(result)
    return 0


def _plot_command(args: argparse.Namespace) -> int:
    from .plotting import _save_figure, _show, plot, save_plot

    drawing = read(args.path)
    options = {
        "coordinate_space": args.coordinate_space,
        "background": args.background,
        "monochrome": args.monochrome,
        "show_text": not args.hide_text,
        "text_encoding": args.encoding,
        "show_axes": not args.hide_axes,
    }
    if args.output is not None and not args.show:
        save_plot(drawing, args.output, dpi=args.dpi, **options)
        print(f"wrote: {args.output}")
        return 0

    figure, _ = plot(drawing, **options)
    if args.output is not None:
        _save_figure(figure, args.output, dpi=args.dpi)
        print(f"wrote: {args.output}")
    _show()
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
