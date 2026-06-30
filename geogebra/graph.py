"""GRAPH view state, user controls, and function sampling."""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

try:
    from .formula import FormulaModel
    from .settings import KEY_AZIM_DEGREES, KEY_ELEV_DEGREES, MOUSE_AZIM_DEGREES, MOUSE_ELEV_DEGREES
except ImportError:
    from formula import FormulaModel
    from settings import KEY_AZIM_DEGREES, KEY_ELEV_DEGREES, MOUSE_AZIM_DEGREES, MOUSE_ELEV_DEGREES


@dataclass
class View:
    azim: float = -45.0
    elev: float = 28.0
    span: float = 15.0
    center_x: float = 0.0
    center_y: float = 0.0

    def zoom(self, factor: float) -> None:
        self.span = float(np.clip(self.span * factor, 4.0, 36.0))

    def rotate_object_degrees(self, azim_delta: float, elev_delta: float) -> None:
        self.azim = (self.azim - azim_delta) % 360.0
        self.elev = float(np.clip(self.elev + elev_delta, -5.0, 85.0))

    def rotate_camera_degrees(self, azim_delta: float, elev_delta: float) -> None:
        self.azim = (self.azim + azim_delta) % 360.0
        self.elev = float(np.clip(self.elev + elev_delta, -5.0, 85.0))

    def pan_pixels(self, dx: float, dy: float, width: int, height: int) -> None:
        scale = self.span / max(min(width, height), 1)
        self.center_x -= dx * scale
        self.center_y += dy * scale

    def reset(self) -> None:
        self.azim = -45.0
        self.elev = 28.0
        self.span = 15.0
        self.center_x = 0.0
        self.center_y = 0.0


class InputState:
    def __init__(self, *, camera_controls: bool = False) -> None:
        self.drag_button: int | None = None
        self.last_mouse: tuple[int, int] | None = None
        self.camera_controls = camera_controls

    def rotate(self, view: View, dx: float, dy: float) -> None:
        self.rotate_degrees(view, dx * MOUSE_AZIM_DEGREES, -dy * MOUSE_ELEV_DEGREES)

    def rotate_degrees(self, view: View, azim_delta: float, elev_delta: float) -> None:
        if self.camera_controls:
            view.rotate_camera_degrees(azim_delta, elev_delta)
        else:
            view.rotate_object_degrees(azim_delta, elev_delta)

    def apply(self, data: bytes, view: View, width: int, height: int) -> bool:
        text = data.decode("utf-8", errors="ignore")
        quit_requested = False
        i = 0
        while i < len(text):
            if text.startswith("\x1b[<", i):
                end = self._mouse_event_end(text, i)
                if end == -1:
                    break
                self._apply_mouse(text[i : end + 1], view, width, height)
                i = end + 1
                continue
            if text.startswith("\x1b[A", i):
                self.rotate_degrees(view, 0, KEY_ELEV_DEGREES)
                i += 3
                continue
            if text.startswith("\x1b[B", i):
                self.rotate_degrees(view, 0, -KEY_ELEV_DEGREES)
                i += 3
                continue
            if text.startswith("\x1b[C", i):
                self.rotate_degrees(view, KEY_AZIM_DEGREES, 0)
                i += 3
                continue
            if text.startswith("\x1b[D", i):
                self.rotate_degrees(view, -KEY_AZIM_DEGREES, 0)
                i += 3
                continue

            ch = text[i]
            if ch == "\x1b":
                quit_requested = True
            i += 1
        return quit_requested

    @staticmethod
    def _mouse_event_end(text: str, start: int) -> int:
        m_pos = text.find("M", start)
        up_pos = text.find("m", start)
        ends = [pos for pos in (m_pos, up_pos) if pos != -1]
        return min(ends) if ends else -1

    def _apply_mouse(self, seq: str, view: View, width: int, height: int) -> None:
        final = seq[-1]
        try:
            button_s, x_s, y_s = seq[3:-1].split(";")
            button, x, y = int(button_s), int(x_s), int(y_s)
        except ValueError:
            return

        if button == 64:
            view.zoom(0.9)
            return
        if button == 65:
            view.zoom(1.1)
            return

        base_button = button & 0b11
        is_motion = bool(button & 32)
        if final == "m":
            self.drag_button = None
            self.last_mouse = None
            return
        if not is_motion:
            self.drag_button = base_button
            self.last_mouse = (x, y)
            return
        if self.last_mouse is None or self.drag_button is None:
            self.last_mouse = (x, y)
            return

        last_x, last_y = self.last_mouse
        dx, dy = x - last_x, y - last_y
        if self.drag_button == 0:
            self.rotate(view, dx, dy)
        else:
            view.pan_pixels(dx * 8, dy * 16, width, height)
        self.last_mouse = (x, y)


def surface_grid(view: View, samples: int, formula: FormulaModel | None = None) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    half = view.span / 2.0
    x = np.linspace(view.center_x - half, view.center_x + half, samples)
    y = np.linspace(view.center_y - half, view.center_y + half, samples)
    xx, yy = np.meshgrid(x, y)
    if formula is None:
        formula = FormulaModel()
    zz = formula.evaluate(xx, yy)
    return xx, yy, zz


def curve_points(view: View, samples: int, formula: FormulaModel | None = None) -> tuple[np.ndarray, np.ndarray]:
    half = view.span / 2.0
    x = np.linspace(view.center_x - half, view.center_x + half, max(samples * 4, 200))
    if formula is None:
        formula = FormulaModel()
    y = formula.evaluate_curve(x)
    return x, y


def implicit_grid(view: View, samples: int, formula: FormulaModel | None = None) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    half = view.span / 2.0
    sample_count = max(samples * 3, 180)
    x = np.linspace(view.center_x - half, view.center_x + half, sample_count)
    y = np.linspace(view.center_y - half, view.center_y + half, sample_count)
    xx, yy = np.meshgrid(x, y)
    if formula is None:
        formula = FormulaModel()
    values = formula.evaluate_implicit(xx, yy)
    return xx, yy, values
