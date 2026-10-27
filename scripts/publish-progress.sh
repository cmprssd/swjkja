#!/usr/bin/env bash
# Builds the progress page (scripts/progress_page.py) and publishes it to GitHub Pages:
# the gh-pages branch holds the page alone, as one commit replaced on every publish,
# so main stays free of the game's imagery and the repository doesn't grow with each
# refresh. Run it after each merge, once bench/history.sh has rendered any new
# milestone. Offscreen: no window opens.
# Usage: scripts/publish-progress.sh <render folder>...   (current renders of every
# reference view, e.g. %USERPROFILE%\.swjkja\ab-branch as a WSL path; later ones win)
set -euo pipefail
cd "$(dirname "$0")/.."
. bench/lib.sh
[ $# -ge 1 ] || { echo "usage: $0 <render folder>..." >&2; exit 2; }
work=$(wslpath "$WORK_WIN")
out="$work/progress-page"  # kept between runs: images already converted are reused
uv run -q --with numpy --with pillow python scripts/progress_page.py "$out" --work "$work" --ours "$@"
site=$(mktemp -d)
trap 'rm -rf "$site"' EXIT
cp -r "$out"/. "$site"
touch "$site/.nojekyll"
remote=$(git remote get-url origin)
source=$(git rev-parse --short HEAD)
(
	cd "$site"
	git init -q -b gh-pages
	git add -A
	git commit -q -m "The progress page, built from main at $source"
	git push -q -f "$remote" gh-pages
)
echo "published: https://cmprssd.github.io/swjkja/ (built from $source)"
