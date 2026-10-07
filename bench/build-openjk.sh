#!/usr/bin/env bash
# Builds the benchmark reference: OpenJK at the revision pinned in docs/ROADMAP.md,
# Release x64 with the MSVC Build Tools, installed to %USERPROFILE%\.swjkja\openjk.
set -euo pipefail
cd "$(dirname "$0")"
. ./lib.sh
rev=260c59c2907187af555a676fe0cc798893bf7757
src=${SWJKJA_OPENJK_SRC:-$HOME/.cache/swjkja-ref/OpenJK}
if [ ! -d "$src/.git" ]; then
	git clone -q https://github.com/JACoders/OpenJK.git "$src"
fi
git -C "$src" fetch -q --depth 1 origin "$rev"
git -C "$src" -c advice.detachedHead=false checkout -q "$rev"

work=$(wslpath "$WORK_WIN")
mkdir -p "$work/openjk-src"
rsync -a --delete --exclude .git "$src/" "$work/openjk-src/"
# Test instrumentation only: cg_swjkjaView pins the camera to an exact view and
# cg_swjkjaTime the game time (shaders, light styles) for reference screenshots.
# Both are off by default; nothing else changes.
patch_file="$(pwd)/reference/openjk-view.patch"
(cd "$work/openjk-src" && git apply -p1 "$patch_file")

vswhere='/mnt/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe'
cmake=$(wslpath "$("$vswhere" -latest -products '*' -find '**/CMake/bin/cmake.exe' | tr -d '\r' | head -1)")
cd /mnt/c
"$cmake" -S "$WORK_WIN\\openjk-src" -B "$WORK_WIN\\openjk-build" -A x64 \
	-DCMAKE_INSTALL_PREFIX="$WORK_WIN\\openjk"
"$cmake" --build "$WORK_WIN\\openjk-build" --config Release --parallel
"$cmake" --install "$WORK_WIN\\openjk-build" --config Release
# The installed engine is all the benchmarks need; the copied source and the build
# tree (about 150 MB) would otherwise stay behind.
rm -rf "$work/openjk-src" "$work/openjk-build"
