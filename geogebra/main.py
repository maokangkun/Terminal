#!/usr/bin/env python3
"""Terminal GRAPH demo entry point."""

from __future__ import annotations

import argparse
import sys
import time

try:
    from .formula import FormulaModel
    from .graph import InputState, View
    from .history import FormulaHistory
    from .input_panel import FormulaInput
    from .render_pipeline import ensure_sixrs, render_formula_preview, render_sixel_frame
    from .settings import (
        AUTO_ROTATE_DEGREES_PER_SECOND,
        AUTO_ROTATE_FRAME_SECONDS,
        HISTORY_PREVIEW_MATTE,
        QUALITY_SAMPLES,
        TMUX_IMAGE_REFRESH_SECONDS,
    )
    from .terminal_io import Terminal, TmuxStatusGuard, in_tmux
    from .ui import (
        Layout,
        draw_footer,
        draw_graph_image,
        draw_history_images,
        draw_history_panel,
        draw_input_panel,
        draw_preview_image,
        draw_static_frame,
        history_preview_size,
        history_index_from_position,
        make_layout,
        prompt_cursor_index_from_column,
        reveal_history_number,
        scroll_history,
        visible_history_entries,
    )
except ImportError:
    from formula import FormulaModel
    from graph import InputState, View
    from history import FormulaHistory
    from input_panel import FormulaInput
    from render_pipeline import ensure_sixrs, render_formula_preview, render_sixel_frame
    from settings import (
        AUTO_ROTATE_DEGREES_PER_SECOND,
        AUTO_ROTATE_FRAME_SECONDS,
        HISTORY_PREVIEW_MATTE,
        QUALITY_SAMPLES,
        TMUX_IMAGE_REFRESH_SECONDS,
    )
    from terminal_io import Terminal, TmuxStatusGuard, in_tmux
    from ui import (
        Layout,
        draw_footer,
        draw_graph_image,
        draw_history_images,
        draw_history_panel,
        draw_input_panel,
        draw_preview_image,
        draw_static_frame,
        history_preview_size,
        history_index_from_position,
        make_layout,
        prompt_cursor_index_from_column,
        reveal_history_number,
        scroll_history,
        visible_history_entries,
    )

FOCUS_GRAPH = "graph"
FOCUS_INPUT = "input"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Terminal 2D/3D graph viewer using matplotlib and sixrs.")
    parser.add_argument("--quality", choices=QUALITY_SAMPLES, default="fast", help="rendering quality preset")
    parser.add_argument("--samples", type=int, help="override surface samples per axis")
    parser.add_argument("--max-colors", type=int, default=96, help="sixrs palette size")
    parser.add_argument("--cell-width", type=int, help="fallback terminal cell width in pixels")
    parser.add_argument("--cell-height", type=int, help="fallback terminal cell height in pixels")
    parser.add_argument("--no-pixel-query", action="store_true", help="skip terminal pixel-size query")
    parser.add_argument("--camera-controls", action="store_true", help="rotate the camera instead of dragging the object")
    parser.add_argument("--tmux-keep-status", action="store_true", help="keep tmux status line visible")
    return parser.parse_args()


def apply_input(
    data: bytes,
    *,
    graph_input: InputState,
    formula_input: FormulaInput,
    history: FormulaHistory,
    graph_formula: FormulaModel,
    view: View,
    layout: Layout,
    focus: str,
    history_scroll_start: int,
) -> tuple[bool, bool, bool, bool, str, int | None, int, bool]:
    if has_escape_key(data):
        return True, False, False, False, focus, None, 0, False
    data, auto_rotate_toggled = consume_space_toggle(data)
    old_focus = focus
    old_cursor = formula_input.cursor
    focus, selected_history, selected_history_number, history_scroll_delta = update_focus_from_mouse(
        data,
        layout,
        formula_input,
        history,
        focus,
        history_scroll_start,
    )
    before = (view.azim, view.elev, view.span, view.center_x, view.center_y)
    quit_requested = False
    if focus == FOCUS_GRAPH:
        quit_requested = graph_input.apply(data, view, layout.image_width, layout.image_height)
    after = (view.azim, view.elev, view.span, view.center_x, view.center_y)
    graph_formula_changed = False
    input_changed = formula_input.apply(data, active=focus == FOCUS_INPUT, handle_arrows=focus == FOCUS_INPUT)
    history_changed = False
    if formula_input.commit_requested:
        history_changed = history.add(formula_input.text)
        graph_formula_changed = graph_formula.set_formula(formula_input.text)
        if history.entries:
            selected_history_number = history.entries[-1].number
    if selected_history is not None:
        formula_input.set_text(selected_history)
        graph_formula_changed = graph_formula.set_formula(selected_history)
        input_changed = True
    input_changed = input_changed or focus != old_focus or formula_input.cursor != old_cursor
    return (
        quit_requested,
        before != after or graph_formula_changed,
        input_changed,
        history_changed,
        focus,
        selected_history_number,
        history_scroll_delta,
        auto_rotate_toggled,
    )


def apply_pending_input(
    term: Terminal,
    *,
    graph_input: InputState,
    formula_input: FormulaInput,
    history: FormulaHistory,
    graph_formula: FormulaModel,
    view: View,
    layout: Layout,
    focus: str,
    history_scroll_start: int,
) -> tuple[bool, bool, bool, bool, str, int | None, int, bool]:
    graph_changed = False
    formula_changed = False
    history_changed = False
    selected_history_number = None
    history_scroll_delta = 0
    auto_rotate_toggled = False
    while True:
        data = term.read_available(timeout=0)
        if not data:
            return (
                False,
                graph_changed,
                formula_changed,
                history_changed,
                focus,
                selected_history_number,
                history_scroll_delta,
                auto_rotate_toggled,
            )
        (
            quit_requested,
            changed_graph,
            changed_formula,
            changed_history,
            focus,
            changed_selected_number,
            changed_scroll_delta,
            changed_auto_rotate,
        ) = apply_input(
            data,
            graph_input=graph_input,
            formula_input=formula_input,
            history=history,
            graph_formula=graph_formula,
            view=view,
            layout=layout,
            focus=focus,
            history_scroll_start=history_scroll_start,
        )
        graph_changed = graph_changed or changed_graph
        formula_changed = formula_changed or changed_formula
        history_changed = history_changed or changed_history
        if changed_selected_number is not None:
            selected_history_number = changed_selected_number
        history_scroll_delta += changed_scroll_delta
        auto_rotate_toggled ^= changed_auto_rotate
        if quit_requested:
            return (
                True,
                graph_changed,
                formula_changed,
                history_changed,
                focus,
                selected_history_number,
                history_scroll_delta,
                auto_rotate_toggled,
            )


def resolve_layout(args: argparse.Namespace, pixel_size: tuple[int, int] | None) -> Layout:
    return make_layout(pixel_size=pixel_size, cell_width=args.cell_width, cell_height=args.cell_height)


def consume_space_toggle(data: bytes) -> tuple[bytes, bool]:
    text = data.decode("utf-8", errors="ignore")
    output: list[str] = []
    toggled = False
    i = 0
    while i < len(text):
        if text.startswith("\x1b[<", i):
            end = mouse_sequence_end(text, i)
            if end == -1:
                break
            output.append(text[i : end + 1])
            i = end + 1
            continue
        if text.startswith(("\x1b[A", "\x1b[B", "\x1b[C", "\x1b[D"), i):
            output.append(text[i : i + 3])
            i += 3
            continue
        if text[i] == " ":
            toggled = not toggled
        else:
            output.append(text[i])
        i += 1
    return "".join(output).encode("utf-8"), toggled


def mouse_sequence_end(text: str, start: int) -> int:
    m_pos = text.find("M", start)
    up_pos = text.find("m", start)
    ends = [pos for pos in (m_pos, up_pos) if pos != -1]
    return min(ends) if ends else -1


def formula_can_auto_rotate(formula: FormulaModel) -> bool:
    return not formula.is_2d


def tick_auto_rotation(
    *,
    enabled: bool,
    graph_input: InputState,
    view: View,
    formula: FormulaModel,
    last_time: float,
    last_frame: float,
) -> tuple[bool, float, float]:
    now = time.monotonic()
    if not enabled or not formula_can_auto_rotate(formula):
        return False, now, now
    if now - last_frame < AUTO_ROTATE_FRAME_SECONDS:
        return False, last_time, last_frame
    elapsed = max(0.0, now - last_time)
    graph_input.rotate_degrees(view, AUTO_ROTATE_DEGREES_PER_SECOND * elapsed, 0.0)
    return True, now, now


def update_focus_from_mouse(
    data: bytes,
    layout: Layout,
    formula_input: FormulaInput,
    history: FormulaHistory,
    focus: str,
    history_scroll_start: int,
) -> tuple[str, str | None, int | None, int]:
    selected_history = None
    selected_history_number = None
    history_scroll_delta = 0
    for button, x, y, final in mouse_events(data):
        if final != "M":
            continue
        in_history = layout.history_left < x < layout.history_right and layout.graph_top < y < layout.graph_bottom
        if button in {64, 65}:
            if in_history:
                history_scroll_delta += -1 if button == 64 else 1
            continue
        if button & 32:
            continue
        history_index = history_index_from_position(layout, y, x, history, scroll_start=history_scroll_start)
        if history_index is not None:
            entry = history.get(history_index)
            if entry is not None:
                selected_history = entry.formula
                selected_history_number = entry.number
                focus = FOCUS_INPUT
        elif layout.graph_left < x < layout.graph_right and layout.graph_top < y < layout.graph_bottom:
            focus = FOCUS_GRAPH
        elif y == layout.input_prompt_row:
            focus = FOCUS_INPUT
            formula_input.set_cursor(prompt_cursor_index_from_column(layout, formula_input, x))
        elif layout.input_top < y < layout.input_bottom:
            focus = FOCUS_INPUT
    return focus, selected_history, selected_history_number, history_scroll_delta


def has_escape_key(data: bytes) -> bool:
    text = data.decode("utf-8", errors="ignore")
    i = 0
    while i < len(text):
        if text.startswith("\x1b[<", i):
            m_pos = text.find("M", i)
            up_pos = text.find("m", i)
            ends = [pos for pos in (m_pos, up_pos) if pos != -1]
            if not ends:
                return False
            i = min(ends) + 1
            continue
        if text.startswith(("\x1b[A", "\x1b[B", "\x1b[C", "\x1b[D"), i):
            i += 3
            continue
        if text[i] == "\x1b":
            return True
        i += 1
    return False


def mouse_events(data: bytes) -> list[tuple[int, int, int, str]]:
    text = data.decode("utf-8", errors="ignore")
    events: list[tuple[int, int, int, str]] = []
    i = 0
    while i < len(text):
        if not text.startswith("\x1b[<", i):
            i += 1
            continue
        m_pos = text.find("M", i)
        up_pos = text.find("m", i)
        ends = [pos for pos in (m_pos, up_pos) if pos != -1]
        if not ends:
            break
        end = min(ends)
        final = text[end]
        try:
            button_s, x_s, y_s = text[i + 3 : end].split(";")
            events.append((int(button_s), int(x_s), int(y_s), final))
        except ValueError:
            pass
        i = end + 1
    return events


def render_history_previews(
    layout: Layout,
    history: FormulaHistory,
    *,
    max_colors: int,
    cache: dict[tuple[int, int, int, bool], tuple[bytes, int, int]],
    scroll_start: int | None = None,
) -> dict[int, tuple[bytes, int, int]]:
    width, height = history_preview_size(layout)
    previews: dict[int, tuple[bytes, int, int]] = {}
    for entry in visible_history_entries(layout, history, scroll_start):
        key = (entry.number, width, height, True)
        if key not in cache:
            cache[key] = render_formula_preview(
                entry.formula,
                width=width,
                height=height,
                max_colors=max_colors,
                transparent=True,
            )
        previews[entry.number] = cache[key]
    return previews


def protocol_label(*, tmux_status_hidden: bool = False) -> str:
    if not in_tmux():
        return "sixel"
    return "tmux+sixel/status-off" if tmux_status_hidden else "tmux+sixel"


def formula_mode_label(formula: FormulaModel) -> str:
    if formula.is_implicit_2d:
        return "implicit-2d"
    return "explicit-2d" if formula.is_2d else "explicit-3d"


def redraw_tmux_images(
    layout: Layout,
    history: FormulaHistory,
    *,
    graph_image: bytes,
    graph_width: int,
    preview_image: bytes,
    preview_width: int,
    history_previews: dict[int, tuple[bytes, int, int]],
    history_scroll_start: int,
) -> None:
    if not in_tmux():
        return
    if graph_image:
        draw_graph_image(layout, graph_image, graph_width)
    if preview_image:
        draw_preview_image(layout, preview_image, preview_width)
    if history_previews:
        draw_history_images(layout, history, history_previews, scroll_start=history_scroll_start)


def update_history_view_state(
    layout: Layout,
    history: FormulaHistory,
    scroll_start: int,
    selected_number: int | None,
    changed_selected_number: int | None,
    scroll_delta: int,
) -> tuple[int, int | None, bool]:
    dirty = False
    if scroll_delta:
        new_scroll_start = scroll_history(layout, history, scroll_start, scroll_delta)
        dirty = dirty or new_scroll_start != scroll_start
        scroll_start = new_scroll_start
    if changed_selected_number is not None:
        dirty = dirty or changed_selected_number != selected_number
        selected_number = changed_selected_number
        new_scroll_start = reveal_history_number(layout, history, scroll_start, selected_number)
        dirty = dirty or new_scroll_start != scroll_start
        scroll_start = new_scroll_start
    return scroll_start, selected_number, dirty


def main() -> int:
    args = parse_args()
    if not ensure_sixrs():
        print("sixrs not found in PATH. Install or build the sixrs binary first.", file=sys.stderr)
        return 1

    view = View()
    graph_input = InputState(camera_controls=args.camera_controls)
    formula_input = FormulaInput()
    history = FormulaHistory.load()
    if history.entries:
        formula_input.set_text(history.entries[-1].formula)
    else:
        history.add(formula_input.text)
    graph_formula = FormulaModel(formula_input.text)
    selected_history_number = history.entries[-1].number if history.entries else None
    focus = FOCUS_INPUT
    samples = max(24, args.samples or QUALITY_SAMPLES[args.quality])
    quality_label = "custom" if args.samples is not None else args.quality

    with TmuxStatusGuard(enabled=not args.tmux_keep_status) as tmux_status, Terminal() as term:
        protocol = protocol_label(tmux_status_hidden=tmux_status.hidden)
        pixel_size = None if args.no_pixel_query else term.query_window_pixels()
        terminal_background = term.query_background_color() or HISTORY_PREVIEW_MATTE
        layout = resolve_layout(args, pixel_size)
        history_scroll_start = reveal_history_number(layout, history, 0, selected_history_number)
        draw_static_frame(layout)
        graph_dirty = True
        preview_dirty = True
        history_dirty = True
        graph_image = b""
        graph_width = graph_height = 1
        preview_image = b""
        preview_width = preview_height = 1
        history_previews: dict[int, tuple[bytes, int, int]] = {}
        history_preview_cache: dict[tuple[int, int, int, bool], tuple[bytes, int, int]] = {}
        last_tmux_image_refresh = 0.0
        auto_rotate_enabled = True
        last_auto_rotate_time = time.monotonic()
        last_auto_rotate_frame = last_auto_rotate_time

        while True:
            visual_changed = False
            auto_graph_changed, last_auto_rotate_time, last_auto_rotate_frame = tick_auto_rotation(
                enabled=auto_rotate_enabled,
                graph_input=graph_input,
                view=view,
                formula=graph_formula,
                last_time=last_auto_rotate_time,
                last_frame=last_auto_rotate_frame,
            )
            graph_dirty = graph_dirty or auto_graph_changed
            if graph_dirty or preview_dirty:
                (
                    quit_requested,
                    graph_changed,
                    formula_changed,
                    changed_history,
                    focus,
                    changed_selected_number,
                    history_scroll_delta,
                    auto_rotate_toggled,
                ) = apply_pending_input(
                    term,
                    graph_input=graph_input,
                    formula_input=formula_input,
                    history=history,
                    graph_formula=graph_formula,
                    view=view,
                    layout=layout,
                    focus=focus,
                    history_scroll_start=history_scroll_start,
                )
                if quit_requested:
                    return 0
                if auto_rotate_toggled:
                    auto_rotate_enabled = not auto_rotate_enabled
                    last_auto_rotate_time = time.monotonic()
                    last_auto_rotate_frame = last_auto_rotate_time
                history_scroll_start, selected_history_number, history_view_dirty = update_history_view_state(
                    layout,
                    history,
                    history_scroll_start,
                    selected_history_number,
                    changed_selected_number,
                    history_scroll_delta,
                )
                graph_dirty = graph_dirty or graph_changed
                preview_dirty = preview_dirty or formula_changed or auto_rotate_toggled
                history_dirty = history_dirty or changed_history or history_view_dirty

            if graph_dirty:
                graph_image, graph_width, graph_height = render_sixel_frame(
                    view,
                    width=layout.image_width,
                    height=layout.image_height,
                    samples=samples,
                    max_colors=args.max_colors,
                    formula=graph_formula,
                    background_color=terminal_background,
                )
                draw_graph_image(layout, graph_image, graph_width)
                draw_footer(
                    layout,
                    graph_width=graph_width,
                    graph_height=graph_height,
                    view=view,
                    protocol=protocol,
                    quality=quality_label,
                    samples=samples,
                    max_colors=args.max_colors,
                    formula_mode=formula_mode_label(graph_formula),
                    history_count=len(history.entries),
                    auto_rotate=auto_rotate_enabled,
                )
                graph_dirty = False
                visual_changed = True

            if preview_dirty:
                preview_image, preview_width, preview_height = render_formula_preview(
                    formula_input.text,
                    width=layout.preview_width,
                    height=layout.preview_height,
                    max_colors=args.max_colors,
                    transparent=True,
                )
                draw_input_panel(
                    layout,
                    formula=formula_input.text,
                    preview=preview_image,
                    preview_width=preview_width,
                    graph_width=graph_width,
                    graph_height=graph_height,
                    view=view,
                    formula_input=formula_input,
                    input_focused=focus == FOCUS_INPUT,
                    protocol=protocol,
                    quality=quality_label,
                    samples=samples,
                    max_colors=args.max_colors,
                    formula_mode=formula_mode_label(graph_formula),
                    history_count=len(history.entries),
                    auto_rotate=auto_rotate_enabled,
                )
                preview_dirty = False
                visual_changed = True

            if history_dirty:
                history_previews = render_history_previews(
                    layout,
                    history,
                    max_colors=args.max_colors,
                    cache=history_preview_cache,
                    scroll_start=history_scroll_start,
                )
                draw_history_panel(
                    layout,
                    history,
                    history_previews,
                    scroll_start=history_scroll_start,
                    selected_number=selected_history_number,
                )
                history_dirty = False
                visual_changed = True

            if visual_changed:
                redraw_tmux_images(
                    layout,
                    history,
                    graph_image=graph_image,
                    graph_width=graph_width,
                    preview_image=preview_image,
                    preview_width=preview_width,
                    history_previews=history_previews,
                    history_scroll_start=history_scroll_start,
                )
                last_tmux_image_refresh = time.monotonic()

            data = term.read_available(timeout=0.1)
            now = time.monotonic()
            if (
                in_tmux()
                and not tmux_status.hidden
                and now - last_tmux_image_refresh >= TMUX_IMAGE_REFRESH_SECONDS
            ):
                redraw_tmux_images(
                    layout,
                    history,
                    graph_image=graph_image,
                    graph_width=graph_width,
                    preview_image=preview_image,
                    preview_width=preview_width,
                    history_previews=history_previews,
                    history_scroll_start=history_scroll_start,
                )
                last_tmux_image_refresh = now
            new_layout = resolve_layout(args, pixel_size)
            if (new_layout.columns, new_layout.rows) != (layout.columns, layout.rows):
                pixel_size = None if args.no_pixel_query else term.query_window_pixels()
                new_layout = resolve_layout(args, pixel_size)
            if new_layout != layout:
                layout = new_layout
                history_scroll_start = reveal_history_number(
                    layout,
                    history,
                    history_scroll_start,
                    selected_history_number,
                )
                draw_static_frame(layout)
                graph_dirty = True
                preview_dirty = True
                history_dirty = True
            if not data:
                continue
            (
                quit_requested,
                graph_changed,
                formula_changed,
                changed_history,
                focus,
                changed_selected_number,
                history_scroll_delta,
                auto_rotate_toggled,
            ) = apply_input(
                data,
                graph_input=graph_input,
                formula_input=formula_input,
                history=history,
                graph_formula=graph_formula,
                view=view,
                layout=layout,
                focus=focus,
                history_scroll_start=history_scroll_start,
            )
            if quit_requested:
                return 0
            if auto_rotate_toggled:
                auto_rotate_enabled = not auto_rotate_enabled
                last_auto_rotate_time = time.monotonic()
                last_auto_rotate_frame = last_auto_rotate_time
            history_scroll_start, selected_history_number, history_view_dirty = update_history_view_state(
                layout,
                history,
                history_scroll_start,
                selected_history_number,
                changed_selected_number,
                history_scroll_delta,
            )
            (
                pending_quit,
                pending_graph_changed,
                pending_formula_changed,
                pending_history_changed,
                focus,
                pending_selected_number,
                pending_scroll_delta,
                pending_auto_rotate_toggled,
            ) = apply_pending_input(
                term,
                graph_input=graph_input,
                formula_input=formula_input,
                history=history,
                graph_formula=graph_formula,
                view=view,
                layout=layout,
                focus=focus,
                history_scroll_start=history_scroll_start,
            )
            if pending_quit:
                return 0
            if pending_auto_rotate_toggled:
                auto_rotate_enabled = not auto_rotate_enabled
                last_auto_rotate_time = time.monotonic()
                last_auto_rotate_frame = last_auto_rotate_time
            history_scroll_start, selected_history_number, pending_history_view_dirty = update_history_view_state(
                layout,
                history,
                history_scroll_start,
                selected_history_number,
                pending_selected_number,
                pending_scroll_delta,
            )
            graph_dirty = graph_dirty or graph_changed or pending_graph_changed
            preview_dirty = (
                True
                if formula_changed
                or pending_formula_changed
                or auto_rotate_toggled
                or pending_auto_rotate_toggled
                else preview_dirty
            )
            history_dirty = (
                history_dirty
                or changed_history
                or pending_history_changed
                or history_view_dirty
                or pending_history_view_dirty
            )


if __name__ == "__main__":
    raise SystemExit(main())
