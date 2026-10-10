#!/usr/bin/env bash
# Records the camera path and frame times of the reference (OpenJK, built with the
# cg_swjkjaPath patch by bench/build-openjk.sh) on the benchmark demo, world only:
# timedemo with r_drawentities 0 and no HUD, at the quality the reference screenshots
# use (bench/capture-reference.sh). The first run warms the disk cache; the next RUNS
# are the result. Every run must follow the same path (only the timestamps differ),
# which this checks.
# Opens a game window for about 30 seconds per run; refuses while the PC is in use
# (see lib.sh).
#
# Writes paths/<demo>-<n>.txt under the bench mod folder and copies them to OUT
# (default: the scratch folder bench/out, not committed). The path itself (every
# column but the timestamp) is committed as reference/<demo>.path; rerun with
# UPDATE=1 to replace it. Then:
#   bench_path --stats <files>                     the reference's frame times
#   bench_path <GameData> mp/<map> reference/<demo>.path   the new renderer's
set -euo pipefail
cd "$(dirname "$0")"
. ./lib.sh
runs=${RUNS:-3}
demo=${DEMO:-ffa3}
out=${OUT:-out}
mkdir -p "$out"
put "demos/$demo.dm_26" "demos/$demo.dm_26"

one() { # name
	run_until 300 "swjkja path: [0-9]+ frames written" openjk.x86_64.exe "${OPENJK_SETTINGS[@]}" \
		+set r_picmip 0 +set r_ext_compress_textures 0 +set r_DynamicGlow 0 \
		+set cg_fov 80 +set r_textureMode GL_LINEAR_MIPMAP_NEAREST \
		+set cg_draw2D 0 +set r_drawentities 0 +set cg_drawGun 0 \
		+set cg_swjkjaPath "paths/$1.txt" +set timedemo 1 +demo "$demo" >/dev/null
	fetch "paths/$1.txt" "$out/$1.txt"
	get qconsole.log | grep -E "frames, .* fps" | tail -1 >&2
}

one "$demo-cold"
for i in $(seq "$runs"); do one "$demo-$i"; done

# Same views and times in every run: drop the timestamp column and compare.
path() { grep -v '^#' "$1" | cut -d' ' -f2-; }
for i in $(seq "$runs") cold; do
	if ! cmp -s <(path "$out/$demo-1.txt") <(path "$out/$demo-$i.txt"); then
		echo "path-reference: run $i followed a different path than run 1" >&2
		exit 1
	fi
done
if [ "${UPDATE:-}" = 1 ]; then
	{
		echo "# Camera path of bench/demos/$demo.dm_26 in the reference timedemo (bench/path-reference.sh)."
		echo "# usec time_ms x y z pitch yaw roll fov_x; usec is 0: these are views, not timings."
		grep -v '^#' "$out/$demo-1.txt" | awk '{$1 = 0; print}'
	} >"reference/$demo.path"
fi
ls "$out/$demo"-*.txt
