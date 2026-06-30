"""Editable formula input state."""

from __future__ import annotations

try:
    from .settings import DEFAULT_FORMULA
except ImportError:
    from settings import DEFAULT_FORMULA


class FormulaInput:
    def __init__(self, text: str = DEFAULT_FORMULA) -> None:
        self.text = text
        self.cursor = len(text)
        self.commit_requested = False

    def apply(self, data: bytes, *, active: bool = True, handle_arrows: bool = True) -> bool:
        self.commit_requested = False
        text = data.decode("utf-8", errors="ignore")
        changed = False
        i = 0
        while i < len(text):
            if text.startswith("\x1b[<", i):
                end = self._mouse_event_end(text, i)
                if end == -1:
                    break
                i = end + 1
                continue
            if text.startswith("\x1b[C", i):
                if handle_arrows:
                    before = self.cursor
                    self.move_cursor(1)
                    changed = changed or self.cursor != before
                i += 3
                continue
            if text.startswith("\x1b[D", i):
                if handle_arrows:
                    before = self.cursor
                    self.move_cursor(-1)
                    changed = changed or self.cursor != before
                i += 3
                continue
            if text.startswith(("\x1b[A", "\x1b[B"), i):
                i += 3
                continue

            ch = text[i]
            if not active:
                i += 1
                continue
            if ch in {"\x7f", "\b"}:
                if self.cursor > 0:
                    self.text = self.text[: self.cursor - 1] + self.text[self.cursor :]
                    self.cursor -= 1
                    changed = True
            elif ch == "\x15":
                if self.text:
                    self.text = ""
                    self.cursor = 0
                    changed = True
            elif ch in {"\r", "\n"}:
                self.commit_requested = True
            elif ch == "\t":
                pass
            elif ch >= " " and ch != "\x1b":
                self.text = self.text[: self.cursor] + ch + self.text[self.cursor :]
                self.cursor += len(ch)
                changed = True
            i += 1
        return changed

    def move_cursor(self, delta: int) -> None:
        self.cursor = max(0, min(len(self.text), self.cursor + delta))

    def set_cursor(self, index: int) -> None:
        self.cursor = max(0, min(len(self.text), index))

    def set_text(self, text: str) -> None:
        self.text = text
        self.cursor = len(text)
        self.commit_requested = False

    @staticmethod
    def _mouse_event_end(text: str, start: int) -> int:
        m_pos = text.find("M", start)
        up_pos = text.find("m", start)
        ends = [pos for pos in (m_pos, up_pos) if pos != -1]
        return min(ends) if ends else -1
