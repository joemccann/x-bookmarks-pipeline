"""Tests for trading.indexer — scan output/finance/ and upsert_one."""
from __future__ import annotations

import json
import tempfile
from pathlib import Path
from unittest.mock import patch

import pytest

from trading.db.schema import setup, get_connection
from trading import indexer


SAMPLE_META_FINANCE = {
    "tweet_id": "1234567890",
    "tweet_url": "https://x.com/user/status/1234567890",
    "author": "testuser",
    "date": "2026-01-15",
    "script_type": "strategy",
    "category": "finance",
    "subcategory": "equities",
    "ticker": "SPY",
    "direction": "long",
    "timeframe": "D",
    "indicators": ["RSI", "ATR"],
    "pattern": None,
    "key_levels": {"entry": 500.0, "stop_loss": 490.0},
    "rationale": "VIX spike mean reversion play",
    "image_urls": [],
    "chart_data": None,
    "is_finance": True,
    "validation_passed": True,
    "validation_errors": [],
    "validation_warnings": [],
}

SAMPLE_PINE = "//@version=6\nstrategy('Test', overlay=true)\n"


@pytest.fixture
def tmp_db(tmp_path):
    db = tmp_path / "signals.db"
    setup(db)
    return db


@pytest.fixture
def finance_dir(tmp_path):
    d = tmp_path / "output" / "finance" / "equities"
    d.mkdir(parents=True)
    return d


def _write_meta(finance_dir: Path, meta: dict, pine: str | None = None) -> Path:
    meta_path = finance_dir / f"{meta['author']}_{meta['ticker']}_{meta['date']}.meta.json"
    meta_path.write_text(json.dumps(meta), encoding="utf-8")
    if pine is not None:
        pine_path = meta_path.with_suffix("").with_suffix(".pine")
        pine_path.write_text(pine, encoding="utf-8")
    return meta_path


# ---------------------------------------------------------------------------
# run() tests
# ---------------------------------------------------------------------------

def test_run_inserts_new_record(finance_dir, tmp_db):
    _write_meta(finance_dir, SAMPLE_META_FINANCE, SAMPLE_PINE)
    result = indexer.run(finance_dir=finance_dir.parent.parent, db_path=tmp_db, verbose=False)
    assert result["inserted"] == 1
    assert result["errors"] == 0


def test_run_skips_unchanged_record(finance_dir, tmp_db):
    _write_meta(finance_dir, SAMPLE_META_FINANCE, SAMPLE_PINE)
    indexer.run(finance_dir=finance_dir.parent.parent, db_path=tmp_db, verbose=False)
    result = indexer.run(finance_dir=finance_dir.parent.parent, db_path=tmp_db, verbose=False)
    assert result["skipped"] == 1
    assert result["inserted"] == 0


def test_run_updates_changed_record(finance_dir, tmp_db):
    _write_meta(finance_dir, SAMPLE_META_FINANCE, SAMPLE_PINE)
    indexer.run(finance_dir=finance_dir.parent.parent, db_path=tmp_db, verbose=False)

    updated = {**SAMPLE_META_FINANCE, "rationale": "Updated rationale"}
    _write_meta(finance_dir, updated, SAMPLE_PINE)
    result = indexer.run(finance_dir=finance_dir.parent.parent, db_path=tmp_db, verbose=False)
    assert result["updated"] == 1


def test_run_stores_pine_script(finance_dir, tmp_db):
    _write_meta(finance_dir, SAMPLE_META_FINANCE, SAMPLE_PINE)
    indexer.run(finance_dir=finance_dir.parent.parent, db_path=tmp_db, verbose=False)
    conn = get_connection(tmp_db)
    row = conn.execute(
        "SELECT pine_script FROM finance_signals WHERE tweet_id=?",
        (SAMPLE_META_FINANCE["tweet_id"],),
    ).fetchone()
    assert row["pine_script"] == SAMPLE_PINE
    conn.close()


def test_run_null_pine_when_missing(finance_dir, tmp_db):
    _write_meta(finance_dir, SAMPLE_META_FINANCE, pine=None)
    indexer.run(finance_dir=finance_dir.parent.parent, db_path=tmp_db, verbose=False)
    conn = get_connection(tmp_db)
    row = conn.execute(
        "SELECT pine_script FROM finance_signals WHERE tweet_id=?",
        (SAMPLE_META_FINANCE["tweet_id"],),
    ).fetchone()
    assert row["pine_script"] is None
    conn.close()


def test_run_skips_meta_without_tweet_id(finance_dir, tmp_db):
    bad = {**SAMPLE_META_FINANCE, "tweet_id": ""}
    _write_meta(finance_dir, bad)
    result = indexer.run(finance_dir=finance_dir.parent.parent, db_path=tmp_db, verbose=False)
    assert result["skipped"] == 1
    assert result["inserted"] == 0


def test_run_handles_zero_files(tmp_path, tmp_db):
    empty_dir = tmp_path / "output" / "finance"
    empty_dir.mkdir(parents=True)
    result = indexer.run(finance_dir=empty_dir, db_path=tmp_db, verbose=False)
    assert result == {"inserted": 0, "updated": 0, "skipped": 0, "errors": 0}


# ---------------------------------------------------------------------------
# upsert_one() tests
# ---------------------------------------------------------------------------

def test_upsert_one_indexes_finance_file(finance_dir, tmp_db):
    meta_path = _write_meta(finance_dir, SAMPLE_META_FINANCE, SAMPLE_PINE)
    with patch("trading.indexer.FINANCE_OUTPUT_DIR", finance_dir.parent.parent):
        result = indexer.upsert_one(meta_path, db_path=tmp_db)
    assert result is True
    conn = get_connection(tmp_db)
    row = conn.execute(
        "SELECT tweet_id FROM finance_signals WHERE tweet_id=?",
        (SAMPLE_META_FINANCE["tweet_id"],),
    ).fetchone()
    assert row is not None
    conn.close()


def test_upsert_one_skips_nonexistent_file(tmp_db):
    result = indexer.upsert_one("/nonexistent/file.meta.json", db_path=tmp_db)
    assert result is False


def test_upsert_one_skips_non_finance_path(tmp_path, tmp_db):
    """Files outside FINANCE_OUTPUT_DIR should be silently skipped."""
    non_finance = tmp_path / "output" / "technology" / "ai"
    non_finance.mkdir(parents=True)
    meta_path = non_finance / "user_2026-01-01_abc12345.meta.json"
    meta_path.write_text(json.dumps({**SAMPLE_META_FINANCE, "category": "technology"}))
    with patch("trading.indexer.FINANCE_OUTPUT_DIR", tmp_path / "output" / "finance"):
        result = indexer.upsert_one(meta_path, db_path=tmp_db)
    assert result is False


def test_upsert_one_is_idempotent(finance_dir, tmp_db):
    meta_path = _write_meta(finance_dir, SAMPLE_META_FINANCE, SAMPLE_PINE)
    with patch("trading.indexer.FINANCE_OUTPUT_DIR", finance_dir.parent.parent):
        indexer.upsert_one(meta_path, db_path=tmp_db)
        result = indexer.upsert_one(meta_path, db_path=tmp_db)
    assert result is True  # replace on same tweet_id
    conn = get_connection(tmp_db)
    count = conn.execute("SELECT COUNT(*) FROM finance_signals").fetchone()[0]
    assert count == 1
    conn.close()
