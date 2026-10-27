#!/usr/bin/env bash
# The progress page's videos, rendered offscreen by render_world as main builds it
# (no window opens), encoded with ffmpeg (imageio-ffmpeg's, through uv):
# - ffa3-flythrough.mp4: the ffa3 demo's camera path (reference/ffa3.path, the one
#   the speed comparison times), every frame, at its recorded 20 frames a second;
# - academy3-luke-walk.mp4: academy3's intro walking Luke to lukeNav1, from the
#   walk view (reference/walk-views.txt) every 50 ms. NPCs stand as of a map's first
#   view, so each frame is its own run.
# Each video gets a .txt caption beside it. Frames are deleted once encoded.
# Output: %USERPROFILE%\.swjkja\progress-media (game imagery: never committed).
set -euo pipefail
cd "$(dirname "$0")"
. ./lib.sh
work=$(wslpath "$WORK_WIN")
media="$work/progress-media"
frames="$media/frames"
mkdir -p "$media"
../scripts/cargo-msvc.sh build -q --release --locked -p swjkja-render --example render_world
exe="$work/target-msvc/release/examples/render_world.exe"
ffmpeg=$(uv run -q --with imageio-ffmpeg python -c "import imageio_ffmpeg as f; print(f.get_ffmpeg_exe())")
encode() { # frames-glob out.mp4
	"$ffmpeg" -loglevel error -y -framerate 20 -pattern_type glob -i "$1" \
		-vf scale=1280:720 -c:v libx264 -preset slow -crf 24 -pix_fmt yuv420p -movflags +faststart "$2"
}

rm -rf "$frames"; mkdir -p "$frames"
awk '!/^#/ && NF {printf "mp/ffa3 f%04d %s %s %s %s %s %s %s\n", NR, $3, $4, $5, $6, $7, $8, $2}' \
	reference/ffa3.path >"$frames/views.txt"
"$exe" "$GAMEDATA_WIN" "$(wslpath -w "$frames/views.txt")" "$(wslpath -w "$frames")" >/dev/null
encode "$frames/f*.png" "$media/ffa3-flythrough.mp4"
n=$(ls "$frames"/f*.png | wc -l)
echo "ffa3, the demo's camera path ($n frames, world only), rendered by swjkja." >"$media/ffa3-flythrough.txt"

rm -rf "$frames"; mkdir -p "$frames"
read -r map name x y z p yw r _ rest < <(grep -v '^#' reference/walk-views.txt | awk 'NF' | head -1)
for ms in $(seq 14500 50 19250); do
	echo "$map $(printf 'w%05d' "$ms") $x $y $z $p $yw $r $ms $rest" >"$frames/one.txt"
	"$exe" "$GAMEDATA_WIN" "$(wslpath -w "$frames/one.txt")" "$(wslpath -w "$frames")" >/dev/null
done
encode "$frames/w*.png" "$media/academy3-luke-walk.mp4"
echo "academy3's intro: Luke walks to lukeNav1 as its script asks (14.5-19.25 s, every 50 ms), rendered by swjkja." \
	>"$media/academy3-luke-walk.txt"
rm -rf "$frames"
ls -la "$media"
