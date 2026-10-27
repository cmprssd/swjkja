#!/usr/bin/env bash
# Renders the history views (reference/history-views.txt) with swjkja as each
# milestone of reference/milestones.txt built it, for the progress page
# (scripts/progress_page.py): each commit is checked out in its own worktree and built
# in its own target folder, so no build reuses another's crates. Offscreen: no window
# opens. Milestones already rendered are skipped, so a new one costs one build; the
# build folder and worktree are removed when it is done.
# PNGs go to %USERPROFILE%\.swjkja\history\<PR>-<commit> (game imagery: never committed).
set -uo pipefail
cd "$(dirname "$0")"
. ./lib.sh
repo=$(git rev-parse --show-toplevel)
views=$PWD/reference/history-views.txt
wt=${HOME}/.cache/claude-worktrees/swjkja-history
work=$(wslpath "$WORK_WIN")
export CARGO_TARGET_DIR="$WORK_WIN\\target-history"
export RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc
export WSLENV="${WSLENV:-}:CARGO_TARGET_DIR:RUSTUP_TOOLCHAIN"
win_home=$(dirname "$work")
cargo="$win_home/.cargo/bin/cargo.exe"
examples="$work/target-history/release/examples"
[ -d "$wt" ] || git -C "$repo" worktree add --detach "$wt" HEAD
one="$work/history-one.txt"

grep -v '^#' reference/milestones.txt | awk 'NF {print $1}' | while read -r c; do
	pr=$(git -C "$repo" log -1 --format=%s "$c" | grep -o '#[0-9]*' | tail -1 | tr -d '#')
	out="$work/history/$(printf %03d "${pr:-0}")-$c"
	if ls "$out"/*.png >/dev/null 2>&1; then continue; fi
	mkdir -p "$out"
	git -C "$wt" checkout -q --detach "$c"
	echo "== $c (#$pr)"
	(cd "$wt" && "$cargo" build -q --release --locked -p swjkja-render --example render_world </dev/null) || continue
	# One view per run: an old build stops at the first map it cannot draw. The fourth
	# argument is the time (seconds) of builds before the views file had a time column.
	grep -v '^#' "$views" | awk 'NF' | while read -r line; do
		echo "$line" >"$one"
		"$examples/render_world.exe" "$GAMEDATA_WIN" "$(wslpath -w "$one")" "$(wslpath -w "$out")" 12.345 \
			</dev/null >/dev/null 2>&1 || echo "   $(echo "$line" | awk '{print $2}'): not drawn by this build"
	done
done
# The builds are only needed once (C: is short of space); the renders stay.
rm -rf "$one" "$work/target-history"
git -C "$repo" worktree remove --force "$wt"
