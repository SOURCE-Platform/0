#!/usr/bin/env python3
"""Compact SOURCE's repeated running-app snapshots while the app is closed.

Usage: python3 scripts/compact-window-snapshots.py DB_PATH [--apply]
The default is a read-only estimate. --apply checkpoints SQLite, saves a gzip
backup, merges adjacent equal observations, and vacuums into a replacement DB.
"""

import argparse
import gzip
import json
import os
import shutil
import sqlite3
import subprocess
import sys
from datetime import datetime
from pathlib import Path

BATCH_SIZE = 1000
MAX_SAMPLE_GAP_MS = 15_000


def state_key(row):
    _, _, session, _, front_name, front_bundle, payload, _ = row
    try:
        apps = json.loads(payload)
        app_key = tuple(sorted(
            (str(app.get("bundleId")), str(app.get("processId")),
             str(app.get("appName")), bool(app.get("isFrontmost")))
            for app in apps
        ))
    except (ValueError, TypeError, AttributeError):
        app_key = payload
    return session, front_name, front_bundle, app_key


def check_app_closed():
    result = subprocess.run(["pgrep", "-x", "SOURCE"], capture_output=True, check=False)
    if result.returncode == 0:
        raise RuntimeError("Quit SOURCE before compacting its live database")


def save_backup(database):
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    backup = database.with_name(f"{database.name}.pre-compact-{stamp}.gz")
    try:
        with database.open("rb") as source, gzip.open(backup, "wb", compresslevel=6) as target:
            shutil.copyfileobj(source, target, length=1024 * 1024)
    except Exception:
        backup.unlink(missing_ok=True)
        raise
    return backup


def scan_and_compact(conn, apply):
    cursor = 0
    previous_key = None
    previous_id = None
    observed_until = 0
    total = duplicates = redundant_bytes = 0
    while True:
        rows = conn.execute(
            "SELECT rowid, id, session_id, timestamp, frontmost_app_name, "
            "frontmost_bundle_id, visible_windows_json, COALESCE(last_seen_at, timestamp) "
            "FROM window_snapshots WHERE rowid > ? ORDER BY rowid LIMIT ?",
            (cursor, BATCH_SIZE),
        ).fetchall()
        if not rows:
            break
        deletes = []
        updates = {}
        for row in rows:
            rowid, snapshot_id, _, timestamp, _, _, payload, last_seen = row
            cursor = rowid
            total += 1
            key = state_key(row)
            if key == previous_key and 0 <= timestamp - observed_until <= MAX_SAMPLE_GAP_MS:
                duplicates += 1
                redundant_bytes += len(payload.encode("utf-8"))
                observed_until = max(observed_until, last_seen)
                updates[previous_id] = observed_until
                deletes.append((rowid,))
            else:
                previous_key = key
                previous_id = snapshot_id
                observed_until = last_seen
        if apply and deletes:
            conn.execute("BEGIN IMMEDIATE")
            conn.executemany(
                "UPDATE window_snapshots SET last_seen_at = ? WHERE id = ?",
                [(end, snapshot_id) for snapshot_id, end in updates.items()],
            )
            conn.executemany("DELETE FROM window_snapshots WHERE rowid = ?", deletes)
            conn.commit()
            conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        if total % 50_000 == 0:
            print(f"Scanned {total:,} rows; {duplicates:,} redundant", flush=True)
    return total, duplicates, redundant_bytes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    database = args.database.expanduser().resolve()
    if not database.is_file():
        parser.error(f"Database does not exist: {database}")
    if args.apply:
        check_app_closed()
    conn = sqlite3.connect(f"file:{database}?mode={'rw' if args.apply else 'ro'}", uri=True)
    conn.execute("PRAGMA busy_timeout = 2000")
    backup = None
    compact = database.with_name(database.name + ".compact.tmp")
    try:
        columns = {row[1] for row in conn.execute("PRAGMA table_info(window_snapshots)")}
        if args.apply:
            checkpoint = conn.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
            if checkpoint[0] != 0:
                raise RuntimeError(f"Could not checkpoint database: {checkpoint}")
            backup = save_backup(database)
            print(f"Backup: {backup}", flush=True)
            if "last_seen_at" not in columns:
                conn.execute("ALTER TABLE window_snapshots ADD COLUMN last_seen_at INTEGER")
                conn.commit()
        elif "last_seen_at" not in columns:
            # The installed app may still use the old schema. A temporary view
            # keeps the estimate read-only and conservative.
            conn.close()
            conn = sqlite3.connect(":memory:")
            conn.execute("ATTACH DATABASE ? AS source", (str(database),))
            conn.execute("CREATE TEMP VIEW window_snapshots AS SELECT rowid, id, session_id, "
                         "timestamp, frontmost_app_name, frontmost_bundle_id, "
                         "visible_windows_json, NULL AS last_seen_at FROM source.window_snapshots")
        total, duplicates, redundant_bytes = scan_and_compact(conn, args.apply)
        print(f"Rows: {total:,}; redundant: {duplicates:,}; repeated JSON: "
              f"{redundant_bytes / (1024 ** 3):.2f} GiB")
        if not args.apply:
            return
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        result = conn.execute("PRAGMA quick_check").fetchone()[0]
        if result != "ok":
            raise RuntimeError(f"Database check failed: {result}")
        compact.unlink(missing_ok=True)
        free = shutil.disk_usage(database.parent).free
        if free < database.stat().st_size // 3 + 512 * 1024 * 1024:
            print("Rows compacted; insufficient free space for VACUUM INTO. Backup retained.")
            return
        conn.execute("VACUUM INTO ?", (str(compact),))
        conn.close()
        conn = None
        with sqlite3.connect(f"file:{compact}?mode=ro", uri=True) as verified:
            if verified.execute("PRAGMA quick_check").fetchone()[0] != "ok":
                raise RuntimeError("Compacted database failed integrity check")
        with sqlite3.connect(compact) as replacement:
            mode = replacement.execute("PRAGMA journal_mode=WAL").fetchone()[0]
            if mode.lower() != "wal":
                raise RuntimeError(f"Could not restore WAL mode: {mode}")
        old_size = database.stat().st_size
        check_app_closed()
        for suffix in ("-wal", "-shm"):
            database.with_name(database.name + suffix).unlink(missing_ok=True)
        os.replace(compact, database)
        print(f"Database: {old_size / (1024 ** 3):.2f} -> "
              f"{database.stat().st_size / (1024 ** 3):.2f} GiB; backup: {backup}")
    finally:
        if conn is not None:
            conn.close()
        if args.apply:
            compact.unlink(missing_ok=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, sqlite3.Error) as error:
        print(f"Compaction stopped: {error}", file=sys.stderr)
        sys.exit(1)
