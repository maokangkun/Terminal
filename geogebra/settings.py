"""Shared constants for the terminal graph viewer."""

FALLBACK_CELL_WIDTH = 10
FALLBACK_CELL_HEIGHT = 20
MIN_REASONABLE_CELL_WIDTH = 6
MIN_REASONABLE_CELL_HEIGHT = 10
FOOTER_ROWS = 2
GRAPH_HEIGHT_RATIO = 0.7
GRAPH_WIDTH_RATIO = 0.7
HISTORY_ENTRY_ROWS = 4
HISTORY_FORMULA_ROWS = 3
HISTORY_PREVIEW_MATTE = "#282d35"
TMUX_IMAGE_REFRESH_SECONDS = 0.35
INPUT_PROMPT = "PLOT > "
DEFAULT_FORMULA = r"z=\cos(\sqrt{x^2+y^2})e^{\frac{-(x^2+y^2)}{18}}"

MOUSE_AZIM_DEGREES = 0.55
MOUSE_ELEV_DEGREES = 0.45
KEY_AZIM_DEGREES = 5.0
KEY_ELEV_DEGREES = 4.0
AUTO_ROTATE_DEGREES_PER_SECOND = 6.0
AUTO_ROTATE_FRAME_SECONDS = 0.5

QUALITY_SAMPLES = {
    "fast": 48,
    "balanced": 64,
    "smooth": 96,
}

LOGO_DOG = [
    ".....W...W.....",
    "....WWW.WWW....",
    "...WWWWWWWWW...",
    "..WWbWWWbWWW...",
    "..WWWWnWWWW....",
    "...sWWWWWs.TT..",
    "..ssWWWWWssT...",
    "...ss...ss.....",
]
LOGO_COLORS = {"W": "#f8fafc", "s": "#cbd5e1", "b": "#111318", "n": "#1f2937", "T": "#eef2f7"}
