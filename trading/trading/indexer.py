"""
Indexer — scans output/finance/**/*.meta.json and populates finance_signals table.

Run this after the pipeline processes new bookmarks to keep signals.db in sync.
Idempotent: uses INSERT OR REPLACE keyed on tweet_id.
"""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from trading.config import FINANCE_OUTPUT_DIR, SIGNALS_DB_PATH
from trading.db.schema import get_connection, setup


def _load_pine(meta_path: Path) -> str | None:
    """Return .pine content adjacent to the .meta.json, or None."""
    pine_path = meta_path.with_suffix("").with_suffix(".pine")
    if pine_path.exists():
        return pine_path.read_text(encoding="utf-8")
    return None


def _row_from_meta(
    meta: dict[str, Any],
    meta_path: Path,
    pine: str | None,
    root: Path | None = None,
) -> dict[str, Any]:
    """Map .meta.json fields to finance_signals columns."""
    key_levels = meta.get("key_levels") or {}
    try:
        rel = str(meta_path.relative_to(root or FINANCE_OUTPUT_DIR.parent.parent))
    except ValueError:
        rel = str(meta_path)
    return {
        "tweet_id":          meta.get("tweet_id", ""),
        "tweet_url":         meta.get("tweet_url", ""),
        "author":            meta.get("author", ""),
        "date":              meta.get("date", ""),
        "script_type":       meta.get("script_type", ""),
        "category":          meta.get("category", ""),
        "subcategory":       meta.get("subcategory", ""),
        "ticker":            meta.get("ticker", ""),
        "direction":         meta.get("direction"),
        "timeframe":         meta.get("timeframe", ""),
        "indicators_json":   json.dumps(meta.get("indicators") or []),
        "key_levels_json":   json.dumps(key_levels),
        "rationale":         meta.get("rationale", ""),
        "pine_script":       pine,
        "meta_json":         json.dumps(meta),
        "validation_passed": 1 if meta.get("validation_passed") else 0,
        "file_path":         rel,
    }


def upsert_one(meta_path: str | Path, db_path: Path = SIGNALS_DB_PATH) -> bool:
    """
    Index a single .meta.json file immediately after the pipeline writes it.
    Called by the pipeline's on_meta_saved hook. Returns True if upserted.
    """
    meta_path = Path(meta_path)
    if not meta_path.exists():
        return False

    # Only index finance bookmarks (those inside output/finance/)
    try:
        meta_path.relative_to(FINANCE_OUTPUT_DIR)
    except ValueError:
        return False  # not a finance file — skip silently

    setup(db_path)
    conn = get_connection(db_path)
    try:
        meta = json.loads(meta_path.read_text(encoding="utf-8"))
        tweet_id = meta.get("tweet_id", "")
        if not tweet_id:
            return False
        pine = _load_pine(meta_path)
        row = _row_from_meta(meta, meta_path, pine)
        conn.execute(
            """INSERT OR REPLACE INTO finance_signals
                (tweet_id, tweet_url, author, date, script_type, category,
                 subcategory, ticker, direction, timeframe, indicators_json,
                 key_levels_json, rationale, pine_script, meta_json,
                 validation_passed, file_path)
               VALUES
                (:tweet_id, :tweet_url, :author, :date, :script_type,
                 :category, :subcategory, :ticker, :direction, :timeframe,
                 :indicators_json, :key_levels_json, :rationale,
                 :pine_script, :meta_json, :validation_passed, :file_path)""",
            row,
        )
        conn.commit()
        return True
    except Exception:
        return False
    finally:
        conn.close()


def run(
    finance_dir: Path = FINANCE_OUTPUT_DIR,
    db_path: Path = SIGNALS_DB_PATH,
    verbose: bool = True,
) -> dict[str, int]:
    """
    Scan finance_dir for .meta.json files and upsert into finance_signals.
    Returns {"inserted": N, "updated": N, "skipped": N, "errors": N}.
    """
    setup(db_path)
    conn = get_connection(db_path)

    meta_files = sorted(finance_dir.rglob("*.meta.json"))
    counts = {"inserted": 0, "updated": 0, "skipped": 0, "errors": 0}

    for meta_path in meta_files:
        try:
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
            tweet_id = meta.get("tweet_id", "")
            if not tweet_id:
                counts["skipped"] += 1
                continue

            pine = _load_pine(meta_path)
            row = _row_from_meta(meta, meta_path, pine, root=finance_dir.parent)

            # Check if already indexed
            existing = conn.execute(
                "SELECT id, meta_json FROM finance_signals WHERE tweet_id = ?",
                (tweet_id,),
            ).fetchone()

            if existing:
                if existing["meta_json"] == row["meta_json"]:
                    counts["skipped"] += 1
                    continue
                conn.execute(
                    """UPDATE finance_signals SET
                        tweet_url=:tweet_url, author=:author, date=:date,
                        script_type=:script_type, category=:category,
                        subcategory=:subcategory, ticker=:ticker,
                        direction=:direction, timeframe=:timeframe,
                        indicators_json=:indicators_json,
                        key_levels_json=:key_levels_json, rationale=:rationale,
                        pine_script=:pine_script, meta_json=:meta_json,
                        validation_passed=:validation_passed, file_path=:file_path,
                        updated_at=datetime('now')
                    WHERE tweet_id=:tweet_id""",
                    row,
                )
                counts["updated"] += 1
            else:
                conn.execute(
                    """INSERT INTO finance_signals
                        (tweet_id, tweet_url, author, date, script_type, category,
                         subcategory, ticker, direction, timeframe, indicators_json,
                         key_levels_json, rationale, pine_script, meta_json,
                         validation_passed, file_path)
                       VALUES
                        (:tweet_id, :tweet_url, :author, :date, :script_type,
                         :category, :subcategory, :ticker, :direction, :timeframe,
                         :indicators_json, :key_levels_json, :rationale,
                         :pine_script, :meta_json, :validation_passed, :file_path)""",
                    row,
                )
                counts["inserted"] += 1

        except Exception as e:
            counts["errors"] += 1
            if verbose:
                print(f"  ERROR {meta_path.name}: {e}")

    conn.commit()
    conn.close()

    if verbose:
        total = len(meta_files)
        print(
            f"Indexed {total} files: "
            f"{counts['inserted']} inserted, {counts['updated']} updated, "
            f"{counts['skipped']} unchanged, {counts['errors']} errors"
        )

    return counts
