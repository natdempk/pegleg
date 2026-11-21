#!/usr/bin/env bash
# One-command setup: find your Steam copy of Peggle Deluxe, extract its level files, build the simulator and
# solver, run the bit-exactness test, and build the playback DLL.
#
#   ./bootstrap.sh                 # finds the game in your Steam libraries
#   PEGGLE_DIR=/path ./bootstrap.sh   # or point at the "Peggle Deluxe" folder yourself
#   STEAM_USER=name ./bootstrap.sh    # no local install: download the game with steamcmd (your Steam login)
#
# Needs: bash, python3, Rust (https://rustup.rs). Real-game playback needs Windows (this also works from
# WSL or Git Bash on that Windows machine). Nothing from the game is copied into the repository: the
# extracted files go to game/ (git-ignored).
set -euo pipefail
cd "$(dirname "$0")"
say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v python3 >/dev/null || die "python3 not found"
command -v cargo >/dev/null || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }
command -v cargo >/dev/null || die "Rust not found: install it from https://rustup.rs, then rerun"

# --- 1. find the game -------------------------------------------------------------------------------
to_local() {  # a Windows path (C:\x or C:/x) → this shell's path (WSL /mnt/c/x, Git Bash /c/x)
  local p=${1//\\\\/\\}; p=${p//\\//}
  if [[ $p =~ ^([A-Za-z]):(.*)$ ]]; then
    local d; d=$(echo "${BASH_REMATCH[1]}" | tr A-Z a-z)
    if [ -d /mnt/c ]; then echo "/mnt/$d${BASH_REMATCH[2]}"; else echo "/$d${BASH_REMATCH[2]}"; fi
  else echo "$p"; fi
}
steam_roots() {
  printf '%s\n' "/mnt/c/Program Files (x86)/Steam" "/mnt/c/Program Files/Steam" "/c/Program Files (x86)/Steam" \
    "/c/Program Files/Steam" "$HOME/.steam/steam" "$HOME/.local/share/Steam" "$HOME/.var/app/com.valvesoftware.Steam/data/Steam" \
    "$HOME/Library/Application Support/Steam"
}
find_game() {
  [ -n "${PEGGLE_DIR:-}" ] && { [ -f "$PEGGLE_DIR/main.pak" ] && echo "$PEGGLE_DIR" && return; die "no main.pak in PEGGLE_DIR=$PEGGLE_DIR"; }
  local root lib
  while read -r root; do
    [ -d "$root/steamapps" ] || continue
    { echo "$root"; [ -f "$root/steamapps/libraryfolders.vdf" ] && grep -o '"path"[[:space:]]*"[^"]*"' "$root/steamapps/libraryfolders.vdf" | sed 's/.*"\([^"]*\)"$/\1/'; } |
      while read -r lib; do lib=$(to_local "$lib"); [ -f "$lib/steamapps/common/Peggle Deluxe/main.pak" ] && echo "$lib/steamapps/common/Peggle Deluxe"; done
  done < <(steam_roots) | head -1
}
GAME=$(find_game || true)
if [ -z "$GAME" ] && [ -f game/steam/main.pak ]; then GAME=$PWD/game/steam; fi
if [ -z "$GAME" ]; then
  if [ -n "${STEAM_USER:-}" ]; then
    command -v steamcmd >/dev/null || die "steamcmd not found (https://developer.valvesoftware.com/wiki/SteamCMD)"
    say "downloading Peggle Deluxe (Steam app 3480, Windows depot) with steamcmd as $STEAM_USER"
    mkdir -p game/steam
    steamcmd +@sSteamCmdForcePlatformType windows +force_install_dir "$PWD/game/steam" +login "$STEAM_USER" +app_update 3480 validate +quit
    [ -f game/steam/main.pak ] || die "steamcmd did not install the game"
    GAME=$PWD/game/steam
  else
    die "Peggle Deluxe not found. Install it in Steam, or set PEGGLE_DIR=/path/to/Peggle Deluxe, or STEAM_USER=<login> to download it with steamcmd"
  fi
fi
say "game: $GAME"
mkdir -p game; printf '%s\n' "$GAME" > game/steam_dir

# --- 2. extract the level files (and sounds/fonts the video tools use) ----------------------------
if [ ! -f game/levels/_base.dat ]; then
  say "extracting main.pak → game/"
  python3 -I tas/unpak.py "$GAME/main.pak" game
fi
# Linux file systems are case-sensitive; the simulator looks levels up in lower case
for f in game/levels/*[A-Z]*.dat; do [ -e "$f" ] || continue; l=$(echo "$f" | tr A-Z a-z); [ -e "$l" ] || ln -s "$(basename "$f")" "$l"; done

# --- 3. build + test --------------------------------------------------------------------------------
say "building the simulator and solver (release)"
(cd sim && cargo build --release -q)
(cd solver && cargo build --release -q)
say "bit-exactness test: the simulator against 297 recorded real-game runs"
(cd sim && cargo test --release -q --test golden 2>&1 | grep -E "test result|FAIL") || die "golden test failed"

# --- 4. playback DLL --------------------------------------------------------------------------------
if [ ! -f play/bin/bass.dll ] || [ play/hook.c -nt play/bin/bass.dll ]; then
  say "building play/bin/bass.dll"
  sh play/build.sh || echo "note: the playback DLL did not build (see play/README.md). Simulation and solving work without it."
fi

say "ready. Try: solver/target/release/peggle-solver 1-1 --out plans/   (see README.md)"
