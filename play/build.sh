#!/bin/sh
# Build the playback DLL (play/bin/bass.dll, 32-bit Windows) from play/hook.c, on any OS, without admin rights.
# Uses Zig 0.17.1's C compiler, downloaded once into play/.zig/ (~50 MB).
set -e
cd "$(dirname "$0")"
mkdir -p bin
V=0.17.1
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) p=x86_64-linux ;; Linux-aarch64) p=aarch64-linux ;;
  Darwin-arm64) p=aarch64-macos ;; Darwin-x86_64) p=x86_64-macos ;;
  MINGW*|MSYS*|CYGWIN*) p=x86_64-windows ;;
  *) echo "unsupported platform $(uname -s)-$(uname -m): build on Windows, Linux or macOS"; exit 1 ;;
esac
d=.zig/zig-$p-$V
if [ ! -x "$d/zig" ] && [ ! -x "$d/zig.exe" ]; then
  mkdir -p .zig; echo "downloading Zig $V ($p)"
  if [ "$p" = x86_64-windows ]; then
    curl -fsSL "https://ziglang.org/download/$V/zig-$p-$V.zip" -o .zig/zig.zip && (cd .zig && unzip -q zig.zip && rm zig.zip)
  else
    curl -fsSL "https://ziglang.org/download/$V/zig-$p-$V.tar.xz" | tar -xJ -C .zig
  fi
fi
ZIG=$d/zig
# -mcpu=i686: x87 floating point only, no SSE (SSE code expects a 16-byte aligned stack, which the game's
# callers don't keep)
"$ZIG" cc -target x86-windows-gnu -mcpu=i686 -O2 -shared -o bin/bass.dll hook.c bass.def
rm -f bin/*.pdb bin/*.lib
# every BASS function the DLL doesn't implement is forwarded to the game's original (bass_orig.dll). Some
# toolchains mangle the forwarder's module name (`_bass_orig.`), and the game then fails to start with a
# "function not found" dialog: refuse such a build.
if LC_ALL=C grep -a -q "_bass_orig\." bin/bass.dll || ! LC_ALL=C grep -a -q "bass_orig\.BASS_Free" bin/bass.dll; then
  rm -f bin/bass.dll; echo "bad DLL exports from $("$ZIG" version 2>/dev/null): use zig $V"; exit 1
fi
echo "built play/bin/bass.dll (zig $("$ZIG" version))"
