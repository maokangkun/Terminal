"""Terminal input, raw mode, and tmux passthrough helpers."""

from __future__ import annotations

import os
import re
import select
import subprocess
import sys
import termios
import tty

PIXEL_QUERY_RE = re.compile(rb"\x1b\[4;(\d+);(\d+)t")
BACKGROUND_QUERY_RE = re.compile(rb"\x1b\]11;rgb:([0-9A-Fa-f]+)/([0-9A-Fa-f]+)/([0-9A-Fa-f]+)(?:\x1b\\|\x07)")


class Terminal:
    def __init__(self) -> None:
        self.fd = sys.stdin.fileno()
        self.original_attrs: list[int | bytes] | None = None

    def __enter__(self) -> "Terminal":
        self.original_attrs = termios.tcgetattr(self.fd)
        tty.setcbreak(self.fd)
        attrs = termios.tcgetattr(self.fd)
        attrs[3] &= ~termios.ISIG
        termios.tcsetattr(self.fd, termios.TCSADRAIN, attrs)
        sys.stdout.write("\x1b[?1049h\x1b[?25l\x1b[?1003h\x1b[?1006h")
        sys.stdout.flush()
        return self

    def __exit__(self, *_: object) -> None:
        sys.stdout.write("\x1b[?1006l\x1b[?1003l\x1b[?25h\x1b[?1049l")
        sys.stdout.flush()
        if self.original_attrs is not None:
            termios.tcsetattr(self.fd, termios.TCSADRAIN, self.original_attrs)

    def read_available(self, timeout: float = 0.08) -> bytes:
        readable, _, _ = select.select([sys.stdin], [], [], timeout)
        if not readable:
            return b""
        return os.read(self.fd, 65536)

    def query_window_pixels(self, timeout: float = 0.12) -> tuple[int, int] | None:
        sys.stdout.write("\x1b[14t")
        sys.stdout.flush()

        chunks: list[bytes] = []
        deadline = timeout
        while deadline > 0:
            readable, _, _ = select.select([sys.stdin], [], [], min(0.03, deadline))
            deadline -= 0.03
            if not readable:
                continue
            chunks.append(os.read(self.fd, 4096))
            match = PIXEL_QUERY_RE.search(b"".join(chunks))
            if match:
                height, width = int(match.group(1)), int(match.group(2))
                return width, height
        return None

    def query_background_color(self, timeout: float = 0.12) -> str | None:
        sys.stdout.write("\x1b]11;?\x1b\\")
        sys.stdout.flush()

        chunks: list[bytes] = []
        deadline = timeout
        while deadline > 0:
            readable, _, _ = select.select([sys.stdin], [], [], min(0.03, deadline))
            deadline -= 0.03
            if not readable:
                continue
            chunks.append(os.read(self.fd, 4096))
            match = BACKGROUND_QUERY_RE.search(b"".join(chunks))
            if match:
                return _osc_rgb_to_hex(match.group(1), match.group(2), match.group(3))
        return None


def in_tmux() -> bool:
    return bool(os.environ.get("TMUX"))


def _osc_rgb_to_hex(red: bytes, green: bytes, blue: bytes) -> str:
    channels = []
    for raw in (red, green, blue):
        value = int(raw, 16)
        max_value = (16 ** len(raw)) - 1
        channels.append(round(value * 255 / max(max_value, 1)))
    return "#{:02x}{:02x}{:02x}".format(*channels)


class TmuxStatusGuard:
    def __init__(self, *, enabled: bool = True) -> None:
        self.enabled = enabled
        self.original_status: str | None = None
        self.hidden = False

    def __enter__(self) -> "TmuxStatusGuard":
        if not self.enabled or not in_tmux():
            return self
        try:
            proc = subprocess.run(
                ["tmux", "show-option", "-qv", "status"],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
                timeout=0.2,
                check=False,
            )
            if proc.returncode != 0:
                return self
            self.original_status = proc.stdout.strip() or "on"
            off = subprocess.run(
                ["tmux", "set-option", "-q", "status", "off"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=0.2,
                check=False,
            )
            self.hidden = off.returncode == 0
        except (OSError, subprocess.TimeoutExpired):
            self.hidden = False
        return self

    def __exit__(self, *_: object) -> None:
        if not self.hidden or self.original_status is None:
            return
        try:
            subprocess.run(
                ["tmux", "set-option", "-q", "status", self.original_status],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=0.2,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            pass


def tmux_wrap_sixel(image: bytes) -> bytes:
    return b"\x1bPtmux;" + image.replace(b"\x1b", b"\x1b\x1b") + b"\x1b\\"
