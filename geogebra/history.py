"""Formula history state."""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path


HISTORY_CACHE_PATH = Path(__file__).with_name(".history.json")


@dataclass(frozen=True)
class HistoryEntry:
    number: int
    formula: str
    timestamp: str


class FormulaHistory:
    def __init__(
        self,
        entries: list[HistoryEntry] | None = None,
        *,
        cache_path: Path | None = None,
    ) -> None:
        self.entries: list[HistoryEntry] = entries or []
        self.cache_path = cache_path

    @classmethod
    def load(cls, cache_path: Path = HISTORY_CACHE_PATH) -> "FormulaHistory":
        try:
            raw_entries = json.loads(cache_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return cls(cache_path=cache_path)
        if not isinstance(raw_entries, list):
            return cls(cache_path=cache_path)

        entries: list[HistoryEntry] = []
        for raw_entry in raw_entries:
            if not isinstance(raw_entry, dict):
                continue
            formula = str(raw_entry.get("formula", "")).strip()
            timestamp = str(raw_entry.get("timestamp", "")).strip()
            if not formula or not timestamp:
                continue
            entries.append(
                HistoryEntry(
                    number=len(entries) + 1,
                    formula=formula,
                    timestamp=timestamp,
                )
            )
        return cls(entries, cache_path=cache_path)

    def add(self, formula: str) -> bool:
        text = formula.strip()
        if not text:
            return False
        if self.entries and self.entries[-1].formula == text:
            return False
        self.entries.append(
            HistoryEntry(
                number=len(self.entries) + 1,
                formula=text,
                timestamp=datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
            )
        )
        self.save()
        return True

    def get(self, index: int) -> HistoryEntry | None:
        if 0 <= index < len(self.entries):
            return self.entries[index]
        return None

    def save(self) -> None:
        if self.cache_path is None:
            return
        payload = [
            {
                "formula": entry.formula,
                "timestamp": entry.timestamp,
            }
            for entry in self.entries
        ]
        self.cache_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
