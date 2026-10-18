#!/usr/bin/env python3
"""ID-by-ID gate for the vault's §19 rows (final review SPEC-B1 / VER-B3).

Every required ID must be carried by at least one named test that passes
(`scripts/vault-gate-map.py` finds them), or — for what only a person or a
device can show — by an evidence note `docs/security/evidence/<ID>.md`.
A missing test, a failure, an ignored test or a missing note fails the
gate; nothing passes by count.

Usage:
  python3 scripts/vault-id-gate.py --phase f2|g|h|i|j [--map-only]
  --map-only   coverage only: no test runs (cheap; shows what is missing)

Each test binary runs once, serially, under a timeout. The Phase I timing
test runs only with --stress (optimized, separate target dir).
"""

import argparse
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EVIDENCE = ROOT / "docs/security/evidence"


def rng(prefix, a, b, skip=()):
    return [f"{prefix}-{i:02d}" for i in range(a, b + 1) if i not in skip]


# Required IDs per phase (spec §19 rows; §22.16; v0.6 §23). DA-10/DA-11
# are out of v1 (owner Q4: logins only); RE-12 is withdrawn.
PHASES = {
    "f2": rng("PA", 1, 8) + rng("PS", 1, 14) + rng("AU", 1, 7) + rng("MT", 1, 2) + rng("SG", 1, 3)
    + rng("CX", 1, 5) + rng("PW", 1, 12) + ["FFI-01", "XV-PEER"] + rng("RE", 1, 11) + rng("IO", 1, 7) + ["PV-01"],
    "g": rng("DA", 1, 12, skip=(10, 11)),
    "h": rng("BR", 1, 19) + rng("NM", 1, 6),
    "i": rng("IM", 1, 6),
}
PHASES["j"] = sorted(set(
    sum(PHASES.values(), [])
    + rng("CR", 1, 13) + rng("RG", 1, 18) + rng("CP", 1, 8) + rng("RC", 1, 8) + ["RC-01M"]
    + rng("FR", 1, 3) + rng("RF", 1, 8) + rng("HC", 1, 5) + rng("RU", 1, 5) + rng("KD", 1, 3)
    + rng("RL", 1, 4) + rng("EV", 1, 5) + rng("BK", 1, 28) + rng("SC", 1, 4) + rng("SY", 1, 13)
    + rng("TR", 1, 9) + rng("ST", 1, 5) + rng("PR", 1, 6) + rng("UI", 1, 5) + rng("CS", 1, 8)
))
# Shown by a person or a device, never by `cargo test`: an evidence note.
MANUAL = set(rng("IO", 1, 7) + ["PV-01", "NM-05", "NM-06", "UI-02", "UI-04", "EV-03"] + rng("CS", 1, 5))


def load_map():
    out = subprocess.run([sys.executable, "-I", str(ROOT / "scripts/vault-gate-map.py")], capture_output=True, text=True, check=True).stdout
    m = defaultdict(list)
    for line in out.splitlines():
        i, runner, test = line.split("\t")
        m[i].append((runner, test))
    return m


def run_binary(runner, stress):
    """Run one test binary; return {test name: ok|FAILED|ignored}."""
    env = dict(os.environ)
    if runner.startswith("node:"):
        cmd, cwd = ["node", "--test", runner[5:]], ROOT
    else:
        _, pkg, target = runner.split(":", 2)
        cmd, cwd = ["cargo", "test", "-p", pkg, *target.split(), "--", "--test-threads=1"], ROOT / "src-tauri"
        if stress and "dashlane_import" in target:
            cmd.append("--include-ignored")
            env["CARGO_PROFILE_DEV_OPT_LEVEL"] = "3"
            env["CARGO_TARGET_DIR"] = str(ROOT / "src-tauri/target/gate-opt")
    try:
        p = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=3600)
        text = p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return {"__timeout__": "FAILED"}
    results = {}
    for name, status in re.findall(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", text, re.M):
        results[name.split("::")[-1]] = status
    for mark, name in re.findall(r"^([✔✖]) (.+?)(?: \(\d+(?:\.\d+)?ms\))?$", text, re.M):
        results[name] = "ok" if mark == "✔" else "FAILED"
    return results


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--phase", required=True, choices=sorted(PHASES))
    ap.add_argument("--map-only", action="store_true")
    ap.add_argument("--stress", action="store_true")
    a = ap.parse_args()
    required, m = PHASES[a.phase], load_map()
    runs = {}
    if not a.map_only:
        for runner in sorted({r for i in required for r, _ in m.get(i, [])}):
            print(f"running {runner} …", flush=True)
            runs[runner] = run_binary(runner, a.stress)
    failed = 0
    for i in required:
        note = EVIDENCE / f"{i}.md"
        if i in MANUAL:
            status = "MANUAL ok" if note.exists() else "MANUAL: evidence missing"
        elif not m.get(i):
            status = "MISSING"
        elif a.map_only:
            status = f"mapped ({len(m[i])})"
        else:
            got = [runs.get(r, {}).get(t, "not run") for r, t in m[i]]
            status = "ok" if got and all(g == "ok" for g in got) else "FAILED: " + ", ".join(f"{t}={g}" for (r, t), g in zip(m[i], got) if g != "ok")
        bad = status.startswith(("MISSING", "FAILED", "MANUAL: evidence"))
        failed += bad
        print(f"{'FAIL' if bad else 'pass'}  {i:8} {status}")
    print(f"\nphase {a.phase}: {len(required) - failed}/{len(required)} IDs evidenced" + (" (map only)" if a.map_only else ""))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
