#!/usr/bin/env bash
# Phase H gate: every required §19/§22.16 ID carried by a passing named
# test, or by an evidence note for what only a person or device can show
# (final review SPEC-B1). Usage: scripts/phase-h-gate.sh [--map-only] [--stress]
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 -I scripts/vault-id-gate.py --phase h "$@"
