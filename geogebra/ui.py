"""Overall terminal style, layout, GRAPH frame, and INPUT panel drawing."""

from __future__ import annotations

import fcntl
import shutil
import struct
import subprocess
import sys
import termios
from dataclasses import dataclass

try:
    from .graph import View
    from .history import FormulaHistory
    from .input_panel import FormulaInput
    from .settings import (
        FALLBACK_CELL_HEIGHT,
        FALLBACK_CELL_WIDTH,
        FOOTER_ROWS,
        GRAPH_HEIGHT_RATIO,
        GRAPH_WIDTH_RATIO,
        HISTORY_ENTRY_ROWS,
        HISTORY_FORMULA_ROWS,
        INPUT_PROMPT,
        LOGO_COLORS,
        LOGO_DOG,
        MIN_REASONABLE_CELL_HEIGHT,
        MIN_REASONABLE_CELL_WIDTH,
    )
    from .terminal_io import in_tmux, tmux_wrap_sixel
except ImportError:
    from graph import View
    from history import FormulaHistory
    from input_panel import FormulaInput
    from settings import (
        FALLBACK_CELL_HEIGHT,
        FALLBACK_CELL_WIDTH,
        FOOTER_ROWS,
        GRAPH_HEIGHT_RATIO,
        GRAPH_WIDTH_RATIO,
        HISTORY_ENTRY_ROWS,
        HISTORY_FORMULA_ROWS,
        INPUT_PROMPT,
        LOGO_COLORS,
        LOGO_DOG,
        MIN_REASONABLE_CELL_HEIGHT,
        MIN_REASONABLE_CELL_WIDTH,
    )
    from terminal_io import in_tmux, tmux_wrap_sixel


@dataclass(frozen=True)
class Layout:
    columns: int
    rows: int
    cell_width: int
    cell_height: int
    terminal_pixel_width: int
    terminal_pixel_height: int
    pixel_source: str
    graph_top: int
    graph_bottom: int
    graph_left: int
    graph_right: int
    history_left: int
    history_right: int
    input_top: int
    input_bottom: int
    image_width: int
    image_height: int
    image_row: int
    image_col: int
    input_prompt_row: int
    preview_row: int
    preview_col: int
    preview_width: int
    preview_height: int
    controls_row: int
    status_row: int
    logo_row: int
    logo_col: int

    @property
    def inner_columns(self) -> int:
        return max(0, self.columns - 2)


def ioctl_terminal_pixels() -> tuple[int, int] | None:
    try:
        packed = fcntl.ioctl(sys.stdout.fileno(), termios.TIOCGWINSZ, struct.pack("HHHH", 0, 0, 0, 0))
        rows, columns, pixel_width, pixel_height = struct.unpack("HHHH", packed)
    except OSError:
        return None
    if rows <= 0 or columns <= 0 or pixel_width <= 0 or pixel_height <= 0:
        return None
    return pixel_width, pixel_height


def cell_size_is_reasonable(pixel_width: int, pixel_height: int, columns: int, rows: int) -> bool:
    cell_width = pixel_width / max(columns, 1)
    cell_height = pixel_height / max(rows, 1)
    return cell_width >= MIN_REASONABLE_CELL_WIDTH and cell_height >= MIN_REASONABLE_CELL_HEIGHT


def tmux_client_cell_size() -> tuple[int, int] | None:
    if not in_tmux() or shutil.which("tmux") is None:
        return None

    try:
        proc = subprocess.run(
            ["tmux", "display-message", "-p", "#{client_cell_width} #{client_cell_height}"],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            timeout=0.2,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return None
    if proc.returncode != 0:
        return None

    try:
        cell_width, cell_height = (int(value) for value in proc.stdout.strip().split())
    except ValueError:
        return None
    if cell_width < MIN_REASONABLE_CELL_WIDTH or cell_height < MIN_REASONABLE_CELL_HEIGHT:
        return None
    return cell_width, cell_height


def make_layout(
    *,
    pixel_size: tuple[int, int] | None = None,
    cell_width: int | None = None,
    cell_height: int | None = None,
) -> Layout:
    size = shutil.get_terminal_size((100, 36))
    columns = max(32, size.columns)
    rows = max(18, size.lines)
    terminal_pixels = pixel_size or ioctl_terminal_pixels()

    if terminal_pixels is not None and cell_size_is_reasonable(*terminal_pixels, columns, rows):
        terminal_pixel_width, terminal_pixel_height = terminal_pixels
        resolved_cell_width = max(1, round(terminal_pixel_width / columns))
        resolved_cell_height = max(1, round(terminal_pixel_height / rows))
        pixel_source = "terminal"
    else:
        tmux_cell = tmux_client_cell_size()
        resolved_cell_width = cell_width or (tmux_cell[0] if tmux_cell else FALLBACK_CELL_WIDTH)
        resolved_cell_height = cell_height or (tmux_cell[1] if tmux_cell else FALLBACK_CELL_HEIGHT)
        terminal_pixel_width = columns * resolved_cell_width
        terminal_pixel_height = rows * resolved_cell_height
        pixel_source = "tmux" if tmux_cell else "fallback"

    if cell_width is not None:
        resolved_cell_width = max(1, cell_width)
        terminal_pixel_width = columns * resolved_cell_width
        pixel_source = "manual"
    if cell_height is not None:
        resolved_cell_height = max(1, cell_height)
        terminal_pixel_height = rows * resolved_cell_height
        pixel_source = "manual"

    graph_rows = round(rows * GRAPH_HEIGHT_RATIO)
    graph_rows = min(max(7, graph_rows), rows - 9)
    input_rows = rows - graph_rows
    graph_top, graph_bottom = 1, graph_rows
    input_top, input_bottom = graph_bottom + 1, rows
    graph_left = 1
    graph_right = min(columns - 18, max(24, round(columns * GRAPH_WIDTH_RATIO)))
    history_left = graph_right + 1
    history_right = columns

    input_prompt_row = input_top + 2
    preview_row = input_top + 4
    controls_row = input_bottom - 2
    status_row = input_bottom - 1
    logo_lines = (len(LOGO_DOG) + 1) // 2
    logo_width = max((len(row) for row in LOGO_DOG), default=0)
    logo_row = max(input_top + 2, controls_row - logo_lines - 2)
    logo_col = max(2, columns - logo_width - 1)

    graph_inner_rows = max(1, graph_bottom - graph_top - 1)
    graph_inner_columns = max(1, graph_right - graph_left - 1)
    graph_pixel_width = max(1, graph_inner_columns * resolved_cell_width)
    graph_pixel_height = max(1, graph_inner_rows * resolved_cell_height)

    preview_rows = max(1, controls_row - preview_row - 2)
    preview_columns = max(12, logo_col - 4)
    preview_pixel_width = max(1, preview_columns * resolved_cell_width)
    preview_pixel_height = max(1, preview_rows * resolved_cell_height)

    return Layout(
        columns=columns,
        rows=rows,
        cell_width=resolved_cell_width,
        cell_height=resolved_cell_height,
        terminal_pixel_width=terminal_pixel_width,
        terminal_pixel_height=terminal_pixel_height,
        pixel_source=pixel_source,
        graph_top=graph_top,
        graph_bottom=graph_bottom,
        graph_left=graph_left,
        graph_right=graph_right,
        history_left=history_left,
        history_right=history_right,
        input_top=input_top,
        input_bottom=input_bottom,
        image_width=graph_pixel_width,
        image_height=graph_pixel_height,
        image_row=graph_top + 1,
        image_col=graph_left + 1,
        input_prompt_row=input_prompt_row,
        preview_row=preview_row,
        preview_col=2,
        preview_width=preview_pixel_width,
        preview_height=preview_pixel_height,
        controls_row=controls_row,
        status_row=status_row,
        logo_row=logo_row,
        logo_col=logo_col,
    )


def move_to(row: int, column: int) -> str:
    return f"\x1b[{row};{column}H"


def draw_static_frame(layout: Layout) -> None:
    sys.stdout.write("\x1b[?2026h\x1b[H\x1b[2J")
    _draw_box(layout.graph_top, layout.graph_bottom, layout.graph_left, layout.graph_right, " GRAPH ")
    _draw_box(layout.graph_top, layout.graph_bottom, layout.history_left, layout.history_right, " HISTORY ")
    _draw_box(layout.input_top, layout.input_bottom, 1, layout.columns, " INPUT ")
    sys.stdout.write("\x1b[?2026l")
    sys.stdout.flush()


def _draw_box(top: int, bottom: int, left: int, right: int, title: str) -> None:
    width = max(2, right - left + 1)
    inner = max(0, width - 2)
    rule = "─" * max(width - len(title) - 2, 0)
    blank = " " * inner
    sys.stdout.write(f"{move_to(top, left)}┌{title}{rule}┐")
    for row in range(top + 1, bottom):
        sys.stdout.write(f"{move_to(row, left)}│{blank}│")
    sys.stdout.write(f"{move_to(bottom, left)}└{'─' * inner}┘")


def draw_graph_image(layout: Layout, image: bytes, width: int) -> None:
    image_cells = max(1, round(width / layout.cell_width))
    graph_inner_columns = max(1, layout.graph_right - layout.graph_left - 1)
    left_padding = max(0, (graph_inner_columns - image_cells) // 2)
    image_col = layout.image_col + left_padding

    if not in_tmux():
        sys.stdout.write("\x1b[?2026h")
    sys.stdout.write(move_to(layout.image_row, image_col))
    sys.stdout.flush()
    sys.stdout.buffer.write(tmux_wrap_sixel(image) if in_tmux() else image)
    sys.stdout.buffer.flush()
    if not in_tmux():
        sys.stdout.write("\x1b[?2026l")
    sys.stdout.flush()


HistoryPreviewMap = dict[int, tuple[bytes, int, int]]


def draw_history_panel(
    layout: Layout,
    history: FormulaHistory,
    previews: HistoryPreviewMap | None = None,
    *,
    scroll_start: int | None = None,
    selected_number: int | None = None,
) -> None:
    if not in_tmux():
        sys.stdout.write("\x1b[?2026h")
    _clear_history_body(layout)
    _write_history_entries(
        layout,
        history,
        previews or {},
        scroll_start=scroll_start,
        selected_number=selected_number,
    )
    if not in_tmux():
        sys.stdout.write("\x1b[?2026l")
    sys.stdout.flush()


def history_index_from_position(
    layout: Layout,
    row: int,
    column: int,
    history: FormulaHistory,
    *,
    scroll_start: int | None = None,
) -> int | None:
    if not (layout.history_left < column < layout.history_right):
        return None
    first_row = layout.graph_top + 1
    last_row = layout.graph_bottom - 1
    if row < first_row or row > last_row:
        return None
    visible_start = clamp_history_scroll(layout, history, scroll_start)
    index = visible_start + (row - first_row) // HISTORY_ENTRY_ROWS
    return index if 0 <= index < len(history.entries) else None


def history_preview_size(layout: Layout) -> tuple[int, int]:
    inner = max(1, layout.history_right - layout.history_left - 1)
    width = max(1, (inner - 1) * layout.cell_width)
    height = max(1, HISTORY_FORMULA_ROWS * layout.cell_height)
    return width, height


def visible_history_entries(layout: Layout, history: FormulaHistory, scroll_start: int | None = None):
    visible_start = clamp_history_scroll(layout, history, scroll_start)
    return history.entries[visible_start : visible_start + history_capacity(layout)]


def history_capacity(layout: Layout) -> int:
    rows = max(0, layout.graph_bottom - layout.graph_top - 1)
    return max(1, rows // HISTORY_ENTRY_ROWS)


def clamp_history_scroll(layout: Layout, history: FormulaHistory, scroll_start: int | None) -> int:
    if scroll_start is None:
        return _history_visible_start(layout, history)
    max_start = max(0, len(history.entries) - history_capacity(layout))
    return min(max(0, scroll_start), max_start)


def scroll_history(layout: Layout, history: FormulaHistory, scroll_start: int, delta: int) -> int:
    return clamp_history_scroll(layout, history, scroll_start + delta)


def reveal_history_number(
    layout: Layout,
    history: FormulaHistory,
    scroll_start: int,
    selected_number: int | None,
) -> int:
    if selected_number is None:
        return clamp_history_scroll(layout, history, scroll_start)
    index = selected_number - 1
    if index < 0 or index >= len(history.entries):
        return clamp_history_scroll(layout, history, scroll_start)
    capacity = history_capacity(layout)
    if index < scroll_start:
        return clamp_history_scroll(layout, history, index)
    if index >= scroll_start + capacity:
        return clamp_history_scroll(layout, history, index - capacity + 1)
    return clamp_history_scroll(layout, history, scroll_start)


def draw_input_panel(
    layout: Layout,
    *,
    formula: str,
    preview: bytes,
    preview_width: int,
    graph_width: int,
    graph_height: int,
    view: View,
    formula_input: FormulaInput | None = None,
    input_focused: bool = True,
    protocol: str = "sixel",
    quality: str = "fast",
    samples: int = 0,
    max_colors: int = 96,
    formula_mode: str = "explicit-3d",
    history_count: int = 0,
    auto_rotate: bool = True,
) -> None:
    if not in_tmux():
        sys.stdout.write("\x1b[?2026h")
    _clear_input_body(layout)
    _write_prompt(layout, formula_input or FormulaInput(formula), input_focused=input_focused)
    _write_preview(layout, preview, preview_width)
    _write_logo(layout)
    _write_footer(
        layout,
        graph_width,
        graph_height,
        view,
        protocol=protocol,
        quality=quality,
        samples=samples,
        max_colors=max_colors,
        formula_mode=formula_mode,
        history_count=history_count,
        auto_rotate=auto_rotate,
    )
    if not in_tmux():
        sys.stdout.write("\x1b[?2026l")
    sys.stdout.flush()


def draw_frame(
    layout: Layout,
    graph_image: bytes,
    graph_width: int,
    graph_height: int,
    preview_image: bytes,
    preview_width: int,
    formula: str,
    view: View,
) -> None:
    draw_graph_image(layout, graph_image, graph_width)
    draw_input_panel(
        layout,
        formula=formula,
        preview=preview_image,
        preview_width=preview_width,
        graph_width=graph_width,
        graph_height=graph_height,
        view=view,
    )


def draw_footer(
    layout: Layout,
    *,
    graph_width: int,
    graph_height: int,
    view: View,
    protocol: str,
    quality: str,
    samples: int,
    max_colors: int,
    formula_mode: str,
    history_count: int,
    auto_rotate: bool = True,
) -> None:
    if not in_tmux():
        sys.stdout.write("\x1b[?2026h")
    _write_footer(
        layout,
        graph_width,
        graph_height,
        view,
        protocol=protocol,
        quality=quality,
        samples=samples,
        max_colors=max_colors,
        formula_mode=formula_mode,
        history_count=history_count,
        auto_rotate=auto_rotate,
    )
    if not in_tmux():
        sys.stdout.write("\x1b[?2026l")
    sys.stdout.flush()


def _clear_history_body(layout: Layout) -> None:
    blank = " " * max(0, layout.history_right - layout.history_left - 1)
    for row in range(layout.graph_top + 1, layout.graph_bottom):
        sys.stdout.write(f"{move_to(row, layout.history_left + 1)}{blank}")


def _write_history_entries(
    layout: Layout,
    history: FormulaHistory,
    previews: HistoryPreviewMap,
    *,
    scroll_start: int | None,
    selected_number: int | None,
) -> None:
    inner = max(0, layout.history_right - layout.history_left - 1)
    first_row = layout.graph_top + 1
    entries = visible_history_entries(layout, history, scroll_start)
    for offset, entry in enumerate(entries):
        row = first_row + offset * HISTORY_ENTRY_ROWS
        marker = ">" if entry.number == selected_number else " "
        title = f"{marker} {entry.number}. {entry.timestamp}"
        line = title[:inner].ljust(inner)
        if entry.number == selected_number:
            line = f"\x1b[7m{line}\x1b[0m"
        sys.stdout.write(f"{move_to(row, layout.history_left + 1)}{line}")
        preview = previews.get(entry.number)
        if preview is None:
            fallback = entry.formula.replace("\n", " ")
            sys.stdout.write(f"{move_to(row + 1, layout.history_left + 1)}{fallback[:inner].ljust(inner)}")
            continue
        _write_history_image(layout, row, preview)


def draw_history_images(
    layout: Layout,
    history: FormulaHistory,
    previews: HistoryPreviewMap,
    *,
    scroll_start: int | None = None,
) -> None:
    for offset, entry in enumerate(visible_history_entries(layout, history, scroll_start)):
        preview = previews.get(entry.number)
        if preview is None:
            continue
        row = layout.graph_top + 1 + offset * HISTORY_ENTRY_ROWS
        _write_history_image(layout, row, preview)
    sys.stdout.flush()


def _write_history_image(layout: Layout, row: int, preview: tuple[bytes, int, int]) -> None:
    inner = max(0, layout.history_right - layout.history_left - 1)
    image, image_width, _image_height = preview
    image_cells = max(1, round(image_width / layout.cell_width))
    left_padding = max(0, (inner - image_cells) // 2)
    sys.stdout.write(move_to(row + 1, layout.history_left + 1 + left_padding))
    sys.stdout.flush()
    sys.stdout.buffer.write(tmux_wrap_sixel(image) if in_tmux() else image)
    sys.stdout.buffer.flush()


def _history_visible_start(layout: Layout, history: FormulaHistory) -> int:
    return max(0, len(history.entries) - history_capacity(layout))


def _clear_input_body(layout: Layout) -> None:
    blank = " " * layout.inner_columns
    for row in range(layout.input_top + 1, layout.input_bottom):
        sys.stdout.write(f"{move_to(row, 2)}{blank}")


def _write_prompt(layout: Layout, formula_input: FormulaInput, *, input_focused: bool) -> None:
    prompt_col = 3
    max_formula_width = max(0, layout.inner_columns - len(INPUT_PROMPT) - 2)
    start = _prompt_window_start(formula_input.text, formula_input.cursor, max_formula_width)
    visible = formula_input.text[start : start + max_formula_width]
    prefix = INPUT_PROMPT + visible
    sys.stdout.write(f"{move_to(layout.input_prompt_row, prompt_col)}{prefix[:layout.inner_columns - 1].ljust(layout.inner_columns - 1)}")
    if input_focused:
        cursor_col = prompt_col + len(INPUT_PROMPT) + formula_input.cursor - start
        cursor_col = min(1 + layout.inner_columns, max(prompt_col + len(INPUT_PROMPT), cursor_col))
        cursor_char = formula_input.text[formula_input.cursor] if formula_input.cursor < len(formula_input.text) else " "
        sys.stdout.write(f"{move_to(layout.input_prompt_row, cursor_col)}\x1b[7m{cursor_char}\x1b[0m")


def prompt_cursor_index_from_column(layout: Layout, formula_input: FormulaInput, column: int) -> int:
    prompt_col = 3
    max_formula_width = max(0, layout.inner_columns - len(INPUT_PROMPT) - 2)
    start = _prompt_window_start(formula_input.text, formula_input.cursor, max_formula_width)
    prompt_start = prompt_col + len(INPUT_PROMPT)
    return start + max(0, column - prompt_start)


def _prompt_window_start(text: str, cursor: int, width: int) -> int:
    if width <= 0:
        return cursor
    if cursor < width:
        return 0
    if cursor >= len(text):
        return max(0, len(text) - width)
    return cursor - width + 1


def _write_preview(layout: Layout, preview: bytes, preview_width: int) -> None:
    sys.stdout.write(f"{move_to(layout.preview_row, layout.preview_col + 1)}PREVIEW:")
    draw_preview_image(layout, preview, preview_width)


def draw_preview_image(layout: Layout, preview: bytes, preview_width: int) -> None:
    preview_cells = max(1, round(preview_width / layout.cell_width))
    left_padding = max(0, (layout.logo_col - layout.preview_col - preview_cells - 2) // 2)
    sys.stdout.write(move_to(layout.preview_row + 1, layout.preview_col + left_padding))
    sys.stdout.flush()
    sys.stdout.buffer.write(tmux_wrap_sixel(preview) if in_tmux() else preview)
    sys.stdout.buffer.flush()


def _write_footer(
    layout: Layout,
    width: int,
    height: int,
    view: View,
    *,
    protocol: str,
    quality: str,
    samples: int,
    max_colors: int,
    formula_mode: str,
    history_count: int,
    auto_rotate: bool,
) -> None:
    auto_label = "rotate" if auto_rotate else "pause"
    runtime = (
        f"protocol={protocol} quality={quality} samples={samples} colors={max_colors} "
        f"mode={formula_mode} history={history_count} auto={auto_label} | space pause/play | esc quit"
    )
    status = (
        f"azim={view.azim:6.1f} elev={view.elev:5.1f} span={view.span:4.1f} "
        f"center=({view.center_x:5.2f}, {view.center_y:5.2f}) rgba={width}x{height} "
        f"cell={layout.cell_width}x{layout.cell_height} src={layout.pixel_source}"
    )
    inner = layout.inner_columns
    sys.stdout.write(f"{move_to(layout.controls_row, 3)}{runtime[:inner - 1].ljust(inner - 1)}")
    sys.stdout.write(f"{move_to(layout.status_row, 3)}{status[:inner - 1].ljust(inner - 1)}")


def _write_logo(layout: Layout) -> None:
    for line_index in range(0, len(LOGO_DOG), 2):
        row = layout.logo_row + line_index // 2
        if row >= layout.controls_row:
            break
        upper = LOGO_DOG[line_index]
        lower = LOGO_DOG[line_index + 1] if line_index + 1 < len(LOGO_DOG) else ""
        sys.stdout.write(move_to(row, layout.logo_col))
        sys.stdout.write(_logo_block_line(upper, lower))
        sys.stdout.write("\x1b[0m")


def _logo_block_line(upper: str, lower: str) -> str:
    width = max(len(upper), len(lower))
    cells = []
    for index in range(width):
        top = upper[index] if index < len(upper) else "."
        bottom = lower[index] if index < len(lower) else "."
        cells.append(_logo_cell(top, bottom))
    return "".join(cells)


def _logo_cell(top: str, bottom: str) -> str:
    top_color = LOGO_COLORS.get(top)
    bottom_color = LOGO_COLORS.get(bottom)
    if not top_color and not bottom_color:
        return "\x1b[0m "
    if top_color and bottom_color and top_color == bottom_color:
        return f"\x1b[0m{_fg(top_color)}█"
    if top_color and bottom_color:
        return f"\x1b[0m{_fg(top_color)}{_bg(bottom_color)}▀"
    if top_color:
        return f"\x1b[0m{_fg(top_color)}▀"
    return f"\x1b[0m{_fg(bottom_color)}▄"


def _fg(color: str) -> str:
    r, g, b = _hex_rgb(color)
    return f"\x1b[38;2;{r};{g};{b}m"


def _bg(color: str) -> str:
    r, g, b = _hex_rgb(color)
    return f"\x1b[48;2;{r};{g};{b}m"


def _hex_rgb(color: str) -> tuple[int, int, int]:
    value = color.lstrip("#")
    return int(value[0:2], 16), int(value[2:4], 16), int(value[4:6], 16)
