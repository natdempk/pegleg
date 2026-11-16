#!/bin/sh
# Play a TAS script in the real game and check the run against the simulator.
#   tas/play.sh SCRIPT SEED OUT [--video] [--speed N]
# SCRIPT is a solver output (S_L.txt), SEED its RNG seed (S_L.seed), OUT the run directory to create.
# Runs on Windows (WSL or Git Bash) through play/play.ps1 with your Steam copy; Steam must be running.
# Plans from the solver expect fast forward + human-timed clicks, which are always on here.
set -e
ROOT=$(cd "$(dirname "$0")/.." && pwd)
[ $# -ge 3 ] || { echo "usage: tas/play.sh SCRIPT SEED OUT [--video] [--speed N]"; exit 2; }
SCRIPT=$1; SEED=$2; OUT=$3; shift 3
VIDEO=0; SPEED=8
while [ $# -gt 0 ]; do case $1 in --video) VIDEO=1 ;; --speed) SPEED=$2; shift ;; *) echo "unknown option $1"; exit 2 ;; esac; shift; done
mkdir -p "$OUT"
# powershell.exe: on PATH (Git Bash, most WSL setups) or at its standard place (WSL without the Windows PATH)
PS=$(command -v powershell.exe 2>/dev/null || true)
for p in /mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe /c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe; do
  if [ -z "$PS" ] && [ -x "$p" ]; then PS=$p; fi
done
[ -n "$PS" ] || { echo "powershell.exe not found: real-game playback runs on Windows (WSL or Git Bash)"; exit 2; }

winpath() { if command -v wslpath >/dev/null 2>&1; then wslpath -w "$1"; else cygpath -w "$1"; fi; }
unixpath() { if command -v wslpath >/dev/null 2>&1; then wslpath -u "$1"; else cygpath -u "$1"; fi; }

# the game writes to a Windows temp dir (not a WSL path), copied back afterwards
name=pegleg-$(date +%s)-$$
wtmp=$("$PS" -NoProfile -Command '[IO.Path]::GetTempPath()' | tr -d '\r')$name
extra=""; [ "$VIDEO" = 1 ] && extra="-Video"
cp "$SCRIPT" "$(unixpath "$wtmp").txt"
"$PS" -NoProfile -ExecutionPolicy Bypass -File "$(winpath "$ROOT/play/play.ps1")" \
  -Script "$wtmp.txt" -Seed "$SEED" -Out "$wtmp" -Speed "$SPEED" -Fast $extra 2>&1 | tr -d '\r'
src=$(unixpath "$wtmp")
[ -d "$src" ] || { echo "playback failed (see the PowerShell output above)"; rm -f "$src.txt"; exit 1; }
cp -r "$src"/. "$OUT"/ && rm -rf "$src" "$src.txt"
for f in "$OUT"/*.txt; do tr -d '\r' < "$f" > "$f.tmp" && mv "$f.tmp" "$f"; done
GC_REAL_FIRE=1 "$ROOT/sim/target/release/game_check" "$OUT" | head -3
