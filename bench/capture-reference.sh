#!/usr/bin/env bash
# Captures reference screenshots with the reference OpenJK (built with the
# cg_swjkjaView/cg_swjkjaTime patch by bench/build-openjk.sh): every view in
# reference/views.txt at its pinned game time,
# world only (r_drawentities 0, no HUD), full-quality textures, 1280x720.
# Holocron mode (g_gametype 1) because free-for-all opens the force-power menu on
# joining, over the view. Opens a game window per map; refuses while the PC is in
# use (see lib.sh).
# PNGs go to %USERPROFILE%\.swjkja\reference-shots (game imagery: never committed).
set -euo pipefail
cd "$(dirname "$0")"
. ./lib.sh
out_win="$WORK_WIN\\reference-shots"
out=$(wslpath "$out_win")
mkdir -p "$out"

maps=$(grep -v '^#' reference/views.txt | awk 'NF {print $1}' | sort -u)
for map in $maps; do
	cfg=$(mktemp)
	{
		# Cheat cvars need the devmap; r_drawentities 0 leaves only the world.
		echo "wait 300"
		echo "set cg_draw2D 0; set r_drawentities 0; set cg_drawGun 0"
		echo "wait 300"
		grep -v '^#' reference/views.txt | awk -v m="$map" '$1 == m {
			printf "set cg_swjkjaView \"%s %s %s %s %s %s\"\nset cg_swjkjaTime %s\nwait 200\nscreenshot_png %s\nwait 100\n", $3, $4, $5, $6, $7, $8, $9, $2
		}'
		echo "quit"
	} >"$cfg"
	put "$cfg" capture.cfg
	rm -f "$cfg"
	echo "capturing $map"
	run_timed 300 openjk.x86_64.exe "${OPENJK_SETTINGS[@]}" +set com_maxfps 60 \
		+set r_picmip 0 +set r_ext_compress_textures 0 +set r_DynamicGlow 0 \
		+set cg_fov 80 +set r_textureMode GL_LINEAR_MIPMAP_NEAREST \
		+set g_gametype 1 +set sv_pure 0 +devmap "$map" +exec capture >/dev/null
	cp -f "$(wslpath "$HOME_WIN")/$BENCH_GAME/screenshots/"*.png "$out/"
done
ls -l "$out"
