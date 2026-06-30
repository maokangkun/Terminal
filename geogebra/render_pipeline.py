"""Matplotlib RGBA rendering and sixrs encoding."""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
from pathlib import Path

import numpy as np

try:
    from .formula import FormulaModel
    from .graph import View, curve_points, implicit_grid, surface_grid
    from .settings import HISTORY_PREVIEW_MATTE
except ImportError:
    from formula import FormulaModel
    from graph import View, curve_points, implicit_grid, surface_grid
    from settings import HISTORY_PREVIEW_MATTE

CONFIG_DIR = Path(tempfile.gettempdir()) / "geogebra-mpl"
CACHE_DIR = Path(tempfile.gettempdir()) / "geogebra-cache"
CONFIG_DIR.mkdir(parents=True, exist_ok=True)
CACHE_DIR.mkdir(parents=True, exist_ok=True)
os.environ.setdefault("MPLBACKEND", "Agg")
os.environ.setdefault("MPLCONFIGDIR", str(CONFIG_DIR))
os.environ.setdefault("XDG_CACHE_HOME", str(CACHE_DIR))

from matplotlib.backends.backend_agg import FigureCanvasAgg
from matplotlib.figure import Figure


def ensure_sixrs() -> bool:
    return shutil.which("sixrs") is not None


def render_rgba(
    view: View,
    width: int,
    height: int,
    samples: int,
    formula: FormulaModel | None = None,
    transparent: bool = False,
    background_color: str = "#080a0f",
) -> tuple[bytes, int, int]:
    formula = formula or FormulaModel()
    if formula.is_implicit_2d:
        return render_implicit_rgba(
            view, width, height, samples, formula, transparent=transparent, background_color=background_color
        )
    if formula.is_2d:
        return render_curve_rgba(
            view, width, height, samples, formula, transparent=transparent, background_color=background_color
        )
    return render_surface_rgba(
        view, width, height, samples, formula, transparent=transparent, background_color=background_color
    )


def render_surface_rgba(
    view: View,
    width: int,
    height: int,
    samples: int,
    formula: FormulaModel,
    *,
    transparent: bool = False,
    background_color: str = "#080a0f",
) -> tuple[bytes, int, int]:
    fig = Figure(figsize=(width / 100, height / 100), dpi=100)
    canvas = FigureCanvasAgg(fig)
    ax = fig.add_subplot(111, projection="3d")
    facecolor = (0, 0, 0, 0) if transparent else background_color
    fig.patch.set_facecolor(facecolor)
    ax.set_facecolor(facecolor)

    xx, yy, zz = surface_grid(view, samples, formula)
    ax.plot_surface(
        xx,
        yy,
        zz,
        cmap="viridis",
        linewidth=0,
        antialiased=False,
        rcount=samples,
        ccount=samples,
        shade=True,
    )
    half = view.span / 2.0
    ax.set_xlim(view.center_x - half, view.center_x + half)
    ax.set_ylim(view.center_y - half, view.center_y + half)
    ax.set_zlim(*z_limits(zz))
    ax.view_init(elev=view.elev, azim=view.azim)
    x_label, y_label = axis_labels(formula)
    ax.set_xlabel(x_label, color="#c8d0dc")
    ax.set_ylabel(y_label, color="#c8d0dc")
    ax.set_zlabel(formula.target_label, color="#c8d0dc")
    ax.tick_params(colors="#aab4c3", labelsize=8)
    ax.grid(True, color="#263242")
    for axis in (ax.xaxis, ax.yaxis, ax.zaxis):
        axis.line.set_color("#7c8797")
        axis.pane.set_facecolor((0, 0, 0, 0) if transparent else hex_to_rgba(background_color))
        axis.pane.set_edgecolor("#4b5565")

    fig.subplots_adjust(left=0.02, right=0.96, bottom=0.02, top=0.92)
    canvas.draw()
    actual_width, actual_height = canvas.get_width_height()
    return bytes(canvas.buffer_rgba()), actual_width, actual_height


def render_curve_rgba(
    view: View,
    width: int,
    height: int,
    samples: int,
    formula: FormulaModel,
    *,
    transparent: bool = False,
    background_color: str = "#080a0f",
) -> tuple[bytes, int, int]:
    fig = Figure(figsize=(width / 100, height / 100), dpi=100)
    canvas = FigureCanvasAgg(fig)
    ax = fig.add_subplot(111)
    facecolor = (0, 0, 0, 0) if transparent else background_color
    fig.patch.set_facecolor(facecolor)
    ax.set_facecolor(facecolor)

    x, y = curve_points(view, samples, formula)
    ax.plot(x, y, color="#5eead4", linewidth=2.2)
    ax.axhline(0, color="#64748b", linewidth=0.9, alpha=0.75)
    ax.axvline(0, color="#64748b", linewidth=0.9, alpha=0.75)
    half = view.span / 2.0
    ax.set_xlim(view.center_x - half, view.center_x + half)
    ax.set_ylim(*curve_y_limits(y, view.center_y))
    x_label, y_label = axis_labels(formula)
    ax.set_xlabel(x_label, color="#c8d0dc")
    ax.set_ylabel(y_label, color="#c8d0dc")
    ax.tick_params(colors="#aab4c3", labelsize=8)
    ax.grid(True, color="#263242", linewidth=0.8)
    for spine in ax.spines.values():
        spine.set_color("#7c8797")
    fig.subplots_adjust(left=0.08, right=0.97, bottom=0.10, top=0.96)
    canvas.draw()
    actual_width, actual_height = canvas.get_width_height()
    return bytes(canvas.buffer_rgba()), actual_width, actual_height


def render_implicit_rgba(
    view: View,
    width: int,
    height: int,
    samples: int,
    formula: FormulaModel,
    *,
    transparent: bool = False,
    background_color: str = "#080a0f",
) -> tuple[bytes, int, int]:
    fig = Figure(figsize=(width / 100, height / 100), dpi=100)
    canvas = FigureCanvasAgg(fig)
    ax = fig.add_subplot(111)
    facecolor = (0, 0, 0, 0) if transparent else background_color
    fig.patch.set_facecolor(facecolor)
    ax.set_facecolor(facecolor)

    xx, yy, values = implicit_grid(view, samples, formula)
    ax.contour(xx, yy, values, levels=[0], colors=["#5eead4"], linewidths=2.2)
    ax.axhline(0, color="#64748b", linewidth=0.9, alpha=0.75)
    ax.axvline(0, color="#64748b", linewidth=0.9, alpha=0.75)
    half = view.span / 2.0
    ax.set_xlim(view.center_x - half, view.center_x + half)
    ax.set_ylim(view.center_y - half, view.center_y + half)
    x_label, y_label = axis_labels(formula)
    ax.set_xlabel(x_label, color="#c8d0dc")
    ax.set_ylabel(y_label, color="#c8d0dc")
    ax.tick_params(colors="#aab4c3", labelsize=8)
    ax.grid(True, color="#263242", linewidth=0.8)
    ax.set_aspect("equal", adjustable="box")
    for spine in ax.spines.values():
        spine.set_color("#7c8797")
    fig.subplots_adjust(left=0.08, right=0.97, bottom=0.10, top=0.96)
    canvas.draw()
    actual_width, actual_height = canvas.get_width_height()
    return bytes(canvas.buffer_rgba()), actual_width, actual_height


def z_limits(values: np.ndarray) -> tuple[float, float]:
    finite = values[np.isfinite(values)]
    if finite.size == 0:
        return -1.0, 1.0
    z_min, z_max = np.percentile(finite, [2, 98])
    if not np.isfinite(z_min) or not np.isfinite(z_max):
        return -1.0, 1.0
    if abs(z_max - z_min) < 1e-9:
        center = float(z_min)
        return center - 1.0, center + 1.0
    padding = float((z_max - z_min) * 0.12)
    return float(z_min - padding), float(z_max + padding)


def axis_labels(formula: FormulaModel) -> tuple[str, str]:
    labels = formula.input_labels
    if len(labels) >= 2:
        return labels[0], labels[1]
    if len(labels) == 1:
        return labels[0], formula.target_label
    return "x", formula.target_label


def curve_y_limits(values: np.ndarray, center_y: float) -> tuple[float, float]:
    finite = values[np.isfinite(values)]
    if finite.size == 0:
        return center_y - 1.0, center_y + 1.0
    y_min, y_max = np.percentile(finite, [2, 98])
    if not np.isfinite(y_min) or not np.isfinite(y_max):
        return center_y - 1.0, center_y + 1.0
    if abs(y_max - y_min) < 1e-9:
        y_min -= 1.0
        y_max += 1.0
    padding = float((y_max - y_min) * 0.15)
    auto_min = float(y_min - padding)
    auto_max = float(y_max + padding)
    if abs(center_y) < 1e-9:
        return auto_min, auto_max
    span = max(auto_max - auto_min, 1.0)
    return center_y - span / 2.0, center_y + span / 2.0


def sixel_encode(
    rgba: bytes,
    width: int,
    height: int,
    max_colors: int,
    *,
    transparent_background: bool = False,
) -> bytes:
    expected_bytes = width * height * 4
    if len(rgba) != expected_bytes:
        raise RuntimeError(
            f"RGBA buffer has {len(rgba)} bytes, but {width}x{height} requires {expected_bytes}"
        )

    proc = subprocess.run(
        [
            "sixrs",
            "--raw-rgba",
            str(width),
            str(height),
            "--max-colors",
            str(max_colors),
            "--protocol",
            "sixel",
            "--cursor-mode",
            "none",
        ],
        input=rgba,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(proc.stderr.decode("utf-8", errors="replace").strip())
    if transparent_background:
        return make_sixel_background_transparent(proc.stdout)
    return proc.stdout


def make_sixel_background_transparent(image: bytes) -> bytes:
    if image.startswith(b"\x1bPq"):
        return b"\x1bP0;1q" + image[3:]
    return image


def render_formula_preview(
    formula: str,
    *,
    width: int,
    height: int,
    max_colors: int,
    transparent: bool = False,
) -> tuple[bytes, int, int]:
    rgba, image_width, image_height = render_formula_rgba(formula, width, height, transparent=transparent)
    if transparent:
        rgba = matte_antialias_rgba(rgba, image_width, image_height, HISTORY_PREVIEW_MATTE)
    image = sixel_encode(
        rgba,
        image_width,
        image_height,
        max_colors,
        transparent_background=transparent,
    )
    return image, image_width, image_height


def render_formula_rgba(
    formula: str,
    width: int,
    height: int,
    *,
    transparent: bool = False,
) -> tuple[bytes, int, int]:
    fig = Figure(figsize=(width / 100, height / 100), dpi=100)
    canvas = FigureCanvasAgg(fig)
    fig.patch.set_facecolor((0, 0, 0, 0) if transparent else "#080a0f")

    text = formula.strip() or r"\mathrm{empty}"
    math_text = text if text.startswith("$") and text.endswith("$") else f"${text}$"
    try:
        fig.text(0.03, 0.55, math_text, color="#f2f4f8", fontsize=24, va="center")
    except Exception:
        fig.clear()
        fig.patch.set_facecolor((0, 0, 0, 0) if transparent else "#080a0f")
        fig.text(0.03, 0.58, "Invalid mathtext", color="#f87171", fontsize=13, va="center")
        fig.text(0.03, 0.32, text[:120], color="#cbd5e1", fontsize=11, va="center")

    try:
        canvas.draw()
    except Exception:
        fig.clear()
        fig.patch.set_facecolor((0, 0, 0, 0) if transparent else "#080a0f")
        fig.text(0.03, 0.58, "Invalid mathtext", color="#f87171", fontsize=13, va="center")
        fig.text(0.03, 0.32, text[:120], color="#cbd5e1", fontsize=11, va="center")
        canvas.draw()

    actual_width, actual_height = canvas.get_width_height()
    return bytes(canvas.buffer_rgba()), actual_width, actual_height


def matte_antialias_rgba(rgba: bytes, width: int, height: int, matte: str) -> bytes:
    pixels = np.frombuffer(rgba, dtype=np.uint8).reshape((height, width, 4)).copy()
    alpha = pixels[:, :, 3].astype(np.float32) / 255.0
    visible = alpha > 0
    if not np.any(visible):
        return rgba

    matte_rgb = np.array(hex_rgb(matte), dtype=np.float32)
    rgb = pixels[:, :, :3].astype(np.float32)
    blended = rgb * alpha[:, :, None] + matte_rgb * (1.0 - alpha[:, :, None])
    pixels[:, :, :3][visible] = np.clip(np.rint(blended[visible]), 0, 255).astype(np.uint8)
    pixels[:, :, 3][visible] = 255
    return pixels.tobytes()


def hex_rgb(color: str) -> tuple[int, int, int]:
    value = color.lstrip("#")
    return int(value[0:2], 16), int(value[2:4], 16), int(value[4:6], 16)


def hex_to_rgba(color: str, alpha: float = 1.0) -> tuple[float, float, float, float]:
    red, green, blue = hex_rgb(color)
    return red / 255.0, green / 255.0, blue / 255.0, alpha


def render_sixel_frame(
    view: View,
    *,
    width: int,
    height: int,
    samples: int,
    max_colors: int,
    formula: FormulaModel | None = None,
    transparent: bool = False,
    background_color: str = "#080a0f",
) -> tuple[bytes, int, int]:
    rgba, image_width, image_height = render_rgba(
        view,
        width,
        height,
        samples,
        formula,
        transparent=transparent,
        background_color=background_color,
    )
    if transparent:
        rgba = matte_antialias_rgba(rgba, image_width, image_height, HISTORY_PREVIEW_MATTE)
    image = sixel_encode(rgba, image_width, image_height, max_colors, transparent_background=transparent)
    return image, image_width, image_height
