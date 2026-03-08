"""
SQLite Bookmark Cache — prevents re-evaluation of already-processed bookmarks.

Each stage (classification, plan, script) is cached independently so partial
pipeline runs can resume from the last completed stage.

Thread-safe: all operations are serialized with a threading lock.
"""
from __future__ import annotations

import json
import sqlite3
import threading
from dataclasses import asdict
from pathlib import Path
from typing import Optional

from src.classifiers.finance_classifier import ClassificationResult
from src.planners.strategy_planner import StrategyPlan


_SCHEMA = """
CREATE TABLE IF NOT EXISTS bookmark_cache (
    tweet_id TEXT PRIMARY KEY,
    classification_json TEXT,
    plan_json TEXT,
    pine_script TEXT,
    validation_passed INTEGER,
    validation_errors TEXT,
    chart_data_json TEXT,
    completed INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);
"""

_MIGRATIONS = [
    "ALTER TABLE bookmark_cache ADD COLUMN chart_data_json TEXT",
    "ALTER TABLE bookmark_cache ADD COLUMN completed INTEGER DEFAULT 0",
]


class BookmarkCache:
    """SQLite-backed cache for pipeline results. Thread-safe."""

    def __init__(self, db_path: str | Path = "cache/bookmarks.db") -> None:
        self.db_path = Path(db_path)
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._conn = sqlite3.connect(str(self.db_path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._conn.execute(_SCHEMA)
        self._conn.commit()
        self._migrate()

    def _migrate(self) -> None:
        """Add new columns if they don't exist (safe for existing DBs)."""
        for stmt in _MIGRATIONS:
            try:
                with self._lock:
                    self._conn.execute(stmt)
                    self._conn.commit()
            except sqlite3.OperationalError:
                pass  # Column already exists

    def close(self) -> None:
        with self._lock:
            self._conn.close()

    # ------------------------------------------------------------------
    # Read
    # ------------------------------------------------------------------

    def get(self, tweet_id: str) -> Optional[dict]:
        """Get the full cache row for a tweet_id, or None."""
        with self._lock:
            row = self._conn.execute(
                "SELECT * FROM bookmark_cache WHERE tweet_id = ?", (tweet_id,)
            ).fetchone()
        if row is None:
            return None
        return dict(row)

    def has_classification(self, tweet_id: str) -> bool:
        row = self.get(tweet_id)
        return row is not None and row.get("classification_json") is not None

    def has_plan(self, tweet_id: str) -> bool:
        row = self.get(tweet_id)
        return row is not None and row.get("plan_json") is not None

    def has_script(self, tweet_id: str) -> bool:
        row = self.get(tweet_id)
        return row is not None and row.get("pine_script") is not None

    def has_chart_data(self, tweet_id: str) -> bool:
        row = self.get(tweet_id)
        return row is not None and row.get("chart_data_json") is not None

    def has_completed(self, tweet_id: str) -> bool:
        row = self.get(tweet_id)
        return row is not None and bool(row.get("completed"))

    def get_classification(self, tweet_id: str) -> Optional[ClassificationResult]:
        row = self.get(tweet_id)
        if row is None or row.get("classification_json") is None:
            return None
        data = json.loads(row["classification_json"])
        return ClassificationResult(**data)

    def get_plan(self, tweet_id: str) -> Optional[StrategyPlan]:
        row = self.get(tweet_id)
        if row is None or row.get("plan_json") is None:
            return None
        data = json.loads(row["plan_json"])
        return StrategyPlan(**data)

    def get_script(self, tweet_id: str) -> Optional[str]:
        row = self.get(tweet_id)
        if row is None:
            return None
        return row.get("pine_script")

    def get_chart_data(self, tweet_id: str) -> Optional[dict]:
        row = self.get(tweet_id)
        if row is None or row.get("chart_data_json") is None:
            return None
        return json.loads(row["chart_data_json"])

    # ------------------------------------------------------------------
    # Write
    # ------------------------------------------------------------------

    def save_classification(self, classification: ClassificationResult) -> None:
        data = asdict(classification)
        json_str = json.dumps(data)
        with self._lock:
            self._conn.execute(
                """INSERT INTO bookmark_cache (tweet_id, classification_json, updated_at)
                   VALUES (?, ?, datetime('now'))
                   ON CONFLICT(tweet_id) DO UPDATE SET
                     classification_json = excluded.classification_json,
                     updated_at = datetime('now')""",
                (classification.tweet_id, json_str),
            )
            self._conn.commit()

    def save_plan(self, plan: StrategyPlan) -> None:
        data = asdict(plan)
        json_str = json.dumps(data)
        with self._lock:
            self._conn.execute(
                """INSERT INTO bookmark_cache (tweet_id, plan_json, updated_at)
                   VALUES (?, ?, datetime('now'))
                   ON CONFLICT(tweet_id) DO UPDATE SET
                     plan_json = excluded.plan_json,
                     updated_at = datetime('now')""",
                (plan.tweet_id, json_str),
            )
            self._conn.commit()

    def save_script(
        self,
        tweet_id: str,
        pine_script: str,
        validation_passed: bool,
        validation_errors: list[str] | None = None,
    ) -> None:
        errors_json = json.dumps(validation_errors or [])
        with self._lock:
            self._conn.execute(
                """INSERT INTO bookmark_cache (tweet_id, pine_script, validation_passed, validation_errors, updated_at)
                   VALUES (?, ?, ?, ?, datetime('now'))
                   ON CONFLICT(tweet_id) DO UPDATE SET
                     pine_script = excluded.pine_script,
                     validation_passed = excluded.validation_passed,
                     validation_errors = excluded.validation_errors,
                     updated_at = datetime('now')""",
                (tweet_id, pine_script, int(validation_passed), errors_json),
            )
            self._conn.commit()

    def save_chart_data(self, tweet_id: str, chart_data: dict) -> None:
        json_str = json.dumps(chart_data)
        with self._lock:
            self._conn.execute(
                """INSERT INTO bookmark_cache (tweet_id, chart_data_json, updated_at)
                   VALUES (?, ?, datetime('now'))
                   ON CONFLICT(tweet_id) DO UPDATE SET
                     chart_data_json = excluded.chart_data_json,
                     updated_at = datetime('now')""",
                (tweet_id, json_str),
            )
            self._conn.commit()

    def mark_completed(self, tweet_id: str) -> None:
        with self._lock:
            self._conn.execute(
                """INSERT INTO bookmark_cache (tweet_id, completed, updated_at)
                   VALUES (?, 1, datetime('now'))
                   ON CONFLICT(tweet_id) DO UPDATE SET
                     completed = 1,
                     updated_at = datetime('now')""",
                (tweet_id,),
            )
            self._conn.commit()

    # ------------------------------------------------------------------
    # Management
    # ------------------------------------------------------------------

    def clear(self) -> int:
        """Delete all cached entries. Returns count of deleted rows."""
        with self._lock:
            cursor = self._conn.execute("DELETE FROM bookmark_cache")
            self._conn.commit()
        return cursor.rowcount

    def stats(self) -> dict:
        """Return cache statistics."""
        with self._lock:
            total = self._conn.execute("SELECT COUNT(*) FROM bookmark_cache").fetchone()[0]
            classified = self._conn.execute(
                "SELECT COUNT(*) FROM bookmark_cache WHERE classification_json IS NOT NULL"
            ).fetchone()[0]
            planned = self._conn.execute(
                "SELECT COUNT(*) FROM bookmark_cache WHERE plan_json IS NOT NULL"
            ).fetchone()[0]
            scripted = self._conn.execute(
                "SELECT COUNT(*) FROM bookmark_cache WHERE pine_script IS NOT NULL"
            ).fetchone()[0]
            valid = self._conn.execute(
                "SELECT COUNT(*) FROM bookmark_cache WHERE validation_passed = 1"
            ).fetchone()[0]
            completed = self._conn.execute(
                "SELECT COUNT(*) FROM bookmark_cache WHERE completed = 1"
            ).fetchone()[0]
        return {
            "total": total,
            "classified": classified,
            "planned": planned,
            "scripted": scripted,
            "valid": valid,
            "completed": completed,
        }
