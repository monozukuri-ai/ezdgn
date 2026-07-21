"""Optional Matplotlib preview rendering for parsed V7 2D drawings.

The renderer deliberately lives outside the parser model.  Native curve and
B-spline records therefore remain lossless even where this module draws their
control sequences as preview approximations.
"""

from __future__ import annotations

import math
from pathlib import Path
from typing import Any, Literal, TypeAlias

from .entities import (
    Arc,
    BSplineCurve,
    BSplinePole,
    BasicStyle,
    Curve,
    Drawing,
    Ellipse,
    Line,
    LineString,
    Shape,
    Text,
)

CoordinateSpace: TypeAlias = Literal["master", "uor"]
Point2: TypeAlias = tuple[float, float]
RgbFloat: TypeAlias = tuple[float, float, float]
RgbaFloat: TypeAlias = tuple[float, float, float, float]
StrokeKey: TypeAlias = tuple[RgbFloat, Any, float]
PolygonKey: TypeAlias = tuple[RgbFloat, RgbaFloat, Any, float]

_LINE_STYLES: dict[int, Any] = {
    0: "-",
    1: (0, (1, 2)),
    2: (0, (6, 3)),
    3: (0, (10, 3)),
    4: (0, (10, 3, 2, 3)),
    5: (0, (10, 3, 2, 3, 2, 3)),
    6: (0, (5, 2)),
    7: (0, (1, 3)),
}


def plot(
    drawing: Drawing,
    *,
    ax: Any | None = None,
    coordinate_space: CoordinateSpace = "master",
    background: str = "#111111",
    monochrome: bool = False,
    show_text: bool = True,
    text_encoding: str = "cp1252",
    curve_steps: int = 128,
    show_axes: bool = True,
) -> tuple[Any, Any]:
    """Render a parsed drawing and return ``(figure, axes)``.

    Matplotlib is imported lazily.  Install the ``plot`` extra before calling
    this function::

        python -m pip install "ezdgn[plot]"

    Ellipses and arcs are sampled for display.  Native type-11 curves and
    B-splines are shown using their parsed control sequences; this affects only
    the preview and never mutates or flattens the source entities.
    """

    if not isinstance(drawing, Drawing):
        raise TypeError("plot() requires an ezdgn.Drawing")
    if coordinate_space not in ("master", "uor"):
        raise ValueError("coordinate_space must be 'master' or 'uor'")
    if coordinate_space == "master" and drawing.design_settings.scale is None:
        raise ValueError(
            "master-unit coordinates are unavailable; use "
            "coordinate_space='uor'"
        )
    if curve_steps < 8:
        raise ValueError("curve_steps must be at least 8")

    matplotlib = _load_matplotlib()
    pyplot = matplotlib["pyplot"]
    colors = matplotlib["colors"]
    if ax is None:
        figure, ax = pyplot.subplots()
    else:
        figure = ax.figure

    background_rgba = colors.to_rgba(background)
    foreground = _contrasting_color(background_rgba[:3])
    figure.patch.set_facecolor(background_rgba)
    ax.set_facecolor(background_rgba)

    renderer = _Renderer(
        drawing=drawing,
        ax=ax,
        coordinate_space=coordinate_space,
        monochrome=monochrome,
        foreground=foreground,
        show_text=show_text,
        text_encoding=text_encoding,
        curve_steps=curve_steps,
        matplotlib=matplotlib,
    )
    renderer.render()

    ax.set_aspect("equal", adjustable="datalim")
    ax.autoscale_view()
    ax.margins(0.05)
    _style_axes(ax, foreground, drawing, coordinate_space, show_axes)
    return figure, ax


def save_plot(
    drawing: Drawing,
    output: str | Path,
    *,
    dpi: int = 150,
    transparent: bool = False,
    **plot_options: Any,
) -> Path:
    """Render ``drawing`` to an image file and return its path."""

    if dpi <= 0:
        raise ValueError("dpi must be greater than zero")
    figure, _ = plot(drawing, **plot_options)
    try:
        return _save_figure(
            figure,
            output,
            dpi=dpi,
            transparent=transparent,
        )
    finally:
        _load_matplotlib()["pyplot"].close(figure)


def _save_figure(
    figure: Any,
    output: str | Path,
    *,
    dpi: int,
    transparent: bool = False,
) -> Path:
    if dpi <= 0:
        raise ValueError("dpi must be greater than zero")
    path = Path(output)
    figure.savefig(
        path,
        dpi=dpi,
        facecolor=figure.get_facecolor(),
        transparent=transparent,
        bbox_inches="tight",
    )
    return path


def _show() -> None:
    """Show all pending figures for the command-line interface."""

    _load_matplotlib()["pyplot"].show()


class _Renderer:
    def __init__(
        self,
        *,
        drawing: Drawing,
        ax: Any,
        coordinate_space: CoordinateSpace,
        monochrome: bool,
        foreground: RgbFloat,
        show_text: bool,
        text_encoding: str,
        curve_steps: int,
        matplotlib: dict[str, Any],
    ) -> None:
        self.drawing = drawing
        self.ax = ax
        self.coordinate_space = coordinate_space
        self.monochrome = monochrome
        self.foreground = foreground
        self.show_text = show_text
        self.text_encoding = text_encoding
        self.curve_steps = curve_steps
        self.PathPatch = matplotlib["PathPatch"]
        self.Path = matplotlib["Path"]
        self.LineCollection = matplotlib["LineCollection"]
        self.PolyCollection = matplotlib["PolyCollection"]
        self.TextPath = matplotlib["TextPath"]
        self.Affine2D = matplotlib["Affine2D"]
        self._line_batches: dict[
            StrokeKey, list[list[Point2] | tuple[Point2, ...]]
        ] = {}
        self._polygon_batches: dict[
            PolygonKey, list[tuple[Point2, ...]]
        ] = {}
        self._text_batches: dict[RgbFloat, list[Any]] = {}
        self._text_path_cache: dict[str, Any] = {}

    def render(self) -> None:
        for element in self.drawing.elements:
            if element.record.deleted:
                continue
            if isinstance(element, Line):
                self._line(element)
            elif isinstance(element, LineString):
                self._polyline(self._vertices(element), element.style)
            elif isinstance(element, Shape):
                self._shape(element)
            elif isinstance(element, Curve):
                self._polyline(self._vertices(element), element.style)
            elif isinstance(element, Ellipse):
                self._ellipse(element)
            elif isinstance(element, Arc):
                self._arc(element)
            elif isinstance(element, Text) and self.show_text:
                self._text(element)
            elif isinstance(element, BSplineCurve):
                self._bspline(element)
            elif isinstance(element, BSplinePole) and element.parent_index is None:
                self._polyline(self._vertices(element), element.style)
        self._flush_collections()

    def _line(self, element: Line) -> None:
        if self.coordinate_space == "master":
            start = element.start_master
            end = element.end_master
        else:
            start = element.start_uor_precise
            end = element.end_uor_precise
        if start is not None and end is not None:
            self._polyline((start, end), element.style)

    def _shape(self, element: Shape) -> None:
        vertices = self._vertices(element)
        if len(vertices) < 3:
            return
        style = element.style
        edge_color = self._color(None if style is None else style.rgb)
        face_color: RgbaFloat = (0.0, 0.0, 0.0, 0.0)
        if style is not None and style.fill_color_index is not None:
            face_color = (*self._color(style.fill_rgb), 0.32)
        self._polygon(
            vertices,
            style,
            edge_color=edge_color,
            face_color=face_color,
        )

    def _ellipse(self, element: Ellipse) -> None:
        center, primary, secondary = self._ellipse_geometry(element)
        if center is None or primary is None or secondary is None:
            return
        points = _sample_ellipse(
            center,
            primary,
            secondary,
            element.rotation_degrees,
            0.0,
            360.0,
            self.curve_steps + 1,
        )
        style = element.style
        face_color: RgbaFloat = (0.0, 0.0, 0.0, 0.0)
        if style is not None and style.fill_color_index is not None:
            face_color = (*self._color(style.fill_rgb), 0.32)
        if face_color[3] == 0.0:
            self._polyline(points, style)
        else:
            self._polygon(
                points,
                style,
                edge_color=self._color(None if style is None else style.rgb),
                face_color=face_color,
            )

    def _arc(self, element: Arc) -> None:
        center, primary, secondary = self._ellipse_geometry(element)
        if center is None or primary is None or secondary is None:
            return
        point_count = max(
            2,
            math.ceil(
                abs(element.sweep_angle_degrees) / 360.0 * self.curve_steps
            )
            + 1,
        )
        points = _sample_ellipse(
            center,
            primary,
            secondary,
            element.rotation_degrees,
            element.start_angle_degrees,
            element.sweep_angle_degrees,
            point_count,
        )
        self._polyline(points, element.style)

    def _text(self, element: Text) -> None:
        if self.coordinate_space == "master":
            origin = element.origin_master
            height = element.height_multiplier_master
            width = element.length_multiplier_master
        else:
            origin = tuple(float(value) for value in element.origin_uor)
            height = float(element.height_multiplier_raw)
            width = float(element.length_multiplier_raw)
        if origin is None or height is None or height <= 0:
            return

        value = element.decode_text(self.text_encoding, errors="replace")
        if not value:
            return
        path = self._text_path_cache.get(value)
        if path is None:
            path = self.TextPath((0.0, 0.0), value, size=1.0)
            self._text_path_cache[value] = path
        if len(path.vertices) == 0:
            return
        bounds = path.get_extents()
        reference_height = bounds.height if bounds.height > 0 else 1.0
        y_scale = height / reference_height
        x_scale = y_scale if width is None or width <= 0 else width / reference_height
        transform = (
            self.Affine2D()
            .scale(x_scale, y_scale)
            .rotate_deg(element.rotation_degrees)
            .translate(*origin)
        )
        transformed_path = transform.transform_path(path)
        color = self._color(None if element.style is None else element.style.rgb)
        self._text_batches.setdefault(color, []).append(transformed_path)

    def _bspline(self, element: BSplineCurve) -> None:
        points: list[Point2] = []
        for child in self.drawing.descendants(element):
            if isinstance(child, BSplinePole):
                points.extend(self._vertices(child))
        self._polyline(points, element.style)

    def _vertices(
        self, element: LineString | Shape | Curve | BSplinePole
    ) -> tuple[Point2, ...]:
        if self.coordinate_space == "master":
            return element.vertices_master or ()
        return element.vertices_uor_precise

    def _ellipse_geometry(
        self, element: Ellipse | Arc
    ) -> tuple[Point2 | None, float | None, float | None]:
        if self.coordinate_space == "master":
            return (
                element.center_master,
                element.primary_axis_master,
                element.secondary_axis_master,
            )
        return (
            element.center_uor,
            element.primary_axis_uor,
            element.secondary_axis_uor,
        )

    def _polyline(
        self, points: list[Point2] | tuple[Point2, ...], style: BasicStyle | None
    ) -> None:
        if len(points) < 2:
            return
        key: StrokeKey = (
            self._color(None if style is None else style.rgb),
            self._line_style(style),
            self._line_width(style),
        )
        self._line_batches.setdefault(key, []).append(points)

    def _polygon(
        self,
        points: tuple[Point2, ...],
        style: BasicStyle | None,
        *,
        edge_color: RgbFloat,
        face_color: RgbaFloat,
    ) -> None:
        key: PolygonKey = (
            edge_color,
            face_color,
            self._line_style(style),
            self._line_width(style),
        )
        self._polygon_batches.setdefault(key, []).append(points)

    def _flush_collections(self) -> None:
        for (
            edge_color,
            face_color,
            line_style,
            line_width,
        ), polygons in self._polygon_batches.items():
            collection = self.PolyCollection(
                polygons,
                closed=True,
                edgecolors=[edge_color],
                facecolors=[face_color],
                linestyles=[line_style],
                linewidths=[line_width],
                zorder=1,
            )
            self.ax.add_collection(collection, autolim=True)

        for (color, line_style, line_width), segments in self._line_batches.items():
            collection = self.LineCollection(
                segments,
                colors=[color],
                linestyles=[line_style],
                linewidths=[line_width],
                zorder=2,
            )
            self.ax.add_collection(collection, autolim=True)

        for color, paths in self._text_batches.items():
            patch = self.PathPatch(
                self.Path.make_compound_path(*paths),
                facecolor=color,
                edgecolor="none",
                zorder=3,
            )
            self.ax.add_patch(patch)

        self._polygon_batches.clear()
        self._line_batches.clear()
        self._text_batches.clear()
        self._text_path_cache.clear()

    def _color(self, rgb: tuple[int, int, int] | None) -> RgbFloat:
        if self.monochrome or rgb is None:
            return self.foreground
        return tuple(component / 255.0 for component in rgb)

    @staticmethod
    def _line_style(style: BasicStyle | None) -> Any:
        return _LINE_STYLES.get(0 if style is None else style.line_style, "-")

    @staticmethod
    def _line_width(style: BasicStyle | None) -> float:
        weight = 0 if style is None else style.line_weight
        return 0.75 + min(max(weight, 0), 31) * 0.12


def _sample_ellipse(
    center: Point2,
    primary_axis: float,
    secondary_axis: float,
    rotation_degrees: float,
    start_degrees: float,
    sweep_degrees: float,
    point_count: int,
) -> tuple[Point2, ...]:
    rotation = math.radians(rotation_degrees)
    cos_rotation = math.cos(rotation)
    sin_rotation = math.sin(rotation)
    points: list[Point2] = []
    denominator = max(point_count - 1, 1)
    for index in range(point_count):
        angle = math.radians(
            start_degrees + sweep_degrees * index / denominator
        )
        local_x = primary_axis * math.cos(angle)
        local_y = secondary_axis * math.sin(angle)
        points.append(
            (
                center[0] + local_x * cos_rotation - local_y * sin_rotation,
                center[1] + local_x * sin_rotation + local_y * cos_rotation,
            )
        )
    return tuple(points)


def _contrasting_color(background: tuple[float, float, float]) -> RgbFloat:
    luminance = (
        0.2126 * background[0]
        + 0.7152 * background[1]
        + 0.0722 * background[2]
    )
    return (0.08, 0.08, 0.08) if luminance > 0.5 else (0.92, 0.92, 0.92)


def _style_axes(
    ax: Any,
    foreground: RgbFloat,
    drawing: Drawing,
    coordinate_space: CoordinateSpace,
    show_axes: bool,
) -> None:
    if not show_axes:
        ax.set_axis_off()
        return
    if coordinate_space == "master":
        unit = drawing.design_settings.master_unit_name
        suffix = f" [{unit}]" if unit else " [master units]"
    else:
        suffix = " [UOR]"
    ax.set_xlabel(f"x{suffix}", color=foreground)
    ax.set_ylabel(f"y{suffix}", color=foreground)
    ax.tick_params(colors=foreground)
    for spine in ax.spines.values():
        spine.set_color(foreground)


def _load_matplotlib() -> dict[str, Any]:
    try:
        from matplotlib import colors
        from matplotlib import pyplot
        from matplotlib.collections import LineCollection, PolyCollection
        from matplotlib.path import Path
        from matplotlib.patches import PathPatch
        from matplotlib.textpath import TextPath
        from matplotlib.transforms import Affine2D
    except ImportError as error:
        raise ImportError(
            "plotting requires Matplotlib; install it with "
            "`python -m pip install 'ezdgn[plot]'`"
        ) from error
    return {
        "Affine2D": Affine2D,
        "LineCollection": LineCollection,
        "Path": Path,
        "PathPatch": PathPatch,
        "PolyCollection": PolyCollection,
        "TextPath": TextPath,
        "colors": colors,
        "pyplot": pyplot,
    }


__all__ = ["CoordinateSpace", "plot", "save_plot"]
