#!/usr/bin/env bash
# Live preview of the dictation pill visuals (equalizer, waterfall, layout) on the
# real mic. Recompiles and relaunches the preview window within a couple of
# seconds of any save to the files below; a failed compile keeps the last
# good window up and prints the errors. Ctrl-C stops it.
# Usage: npm run pill:preview [-- --demo]
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/src-tauri/native-pkg/Sources/SourceDictation"
FILES=(
  "$ROOT/src-tauri/native-pkg/Preview/pill_preview.swift"
  "$SRC/overlay_visualizers.swift"
  "$SRC/overlay_layout.swift"
  "$SRC/overlay_copy.swift"
  "$SRC/overlay_gear.swift"
  "$SRC/band_meter.swift"
)
OUT="${TMPDIR:-/tmp}/source-pill-preview"
mkdir -p "$OUT"
pid=""

stop_preview() {
  if [ -n "$pid" ]; then
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
  fi
  pid=""
}
trap 'stop_preview; exit 0' INT TERM HUP

build_and_run() {
  local started=$SECONDS
  if swiftc -Onone -parse-as-library -o "$OUT/pill-preview.new" "${FILES[@]}" 2>&1; then
    mv "$OUT/pill-preview.new" "$OUT/pill-preview"
    stop_preview
    "$OUT/pill-preview" "$@" &
    pid=$!
    echo "[$(date +%H:%M:%S)] reloaded ($((SECONDS - started))s)"
  else
    echo "[$(date +%H:%M:%S)] build failed; keeping the last good preview"
  fi
}

fingerprint() { cat "${FILES[@]}" 2>/dev/null | md5 -q; }

last=$(fingerprint)
build_and_run "$@"
echo "Watching the pill visual files. Save one to reload. Ctrl-C to stop."
while true; do
  sleep 0.4
  current=$(fingerprint)
  if [ "$current" != "$last" ]; then
    last=$current
    build_and_run "$@"
  fi
done
