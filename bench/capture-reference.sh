#!/usr/bin/env bash
# Captures reference screenshots with the reference OpenJK (built with the
# cg_swjkjaView/cg_swjkjaTime patch by bench/build-openjk.sh): every view in
# reference/views.txt (or the views file given) at its pinned game time,
# world only (r_drawentities 0, no HUD), full-quality textures, 1280x720.
# With ENTITIES=1, entities are drawn too (the static models of
# reference/model-views.txt; those views keep items and movers out of sight).
# With SCRIPTS=1, single player's NPCs run their scripts (only their AI is frozen)
# and the game stops at the map's first view's time, so scripted NPCs stand as of
# that time: a map's views then share one time.
# Multiplayer maps (mp/...) run in holocron mode (g_gametype 1) because free-for-all
# opens the force-power menu on joining, over the view. Other maps run in the
# single-player executable at the easiest skill (g_spskill 0, the cvar's default, as
# the new renderer spawns them) in first person, so the player stays out of sight.
# Opens a game window per map; refuses while the PC is in use (see lib.sh).
# PNGs go to %USERPROFILE%\.swjkja\reference-shots (game imagery: never committed).
set -euo pipefail
cd "$(dirname "$0")"
. ./lib.sh
out_win="$WORK_WIN\\reference-shots"
out=$(wslpath "$out_win")
mkdir -p "$out"

views=${1:-reference/views.txt}
entities=${ENTITIES:-0}
scripts=${SCRIPTS:-0}
maps=$(grep -v '^#' "$views" | awk 'NF {print $1}' | sort -u)
for map in $maps; do
	cfg=$(mktemp)
	sp=0
	case $map in mp/*) ;; *) sp=1 ;; esac
	{
		# NPCs frozen from the first frame stay as they spawned, facing their
		# spawner's way: their AI does not run (it turns them and moves them),
		# their movement frames do (their weapon-ready stance, the drop to the
		# floor). One capture without it found t3_stamp's crates already broken.
		# With SCRIPTS=1 the AI alone is frozen (d_npcfreeze 2, a patch addition)
		# and the game runs no frame the client would draw after the view's time
		# (it draws one frame behind).
		if [ "$sp" = 1 ] && [ "$scripts" = 1 ]; then
			grep -v '^#' "$views" | awk -v m="$map" '$1 == m {print "set d_npcfreeze 2; set g_swjkjaStop " $9 + 50; exit}'
		elif [ "$sp" = 1 ]; then
			echo "set d_npcfreeze 1"
		fi
		# Cheat cvars need the devmap; r_drawentities 0 leaves only the world.
		echo "wait 300"
		# A single-player map may open with a cutscene: skip it as Esc would. The
		# renderer draws only the areas the player's snapshot marks visible and
		# single player never clears the screen, so the player is moved to each
		# view (noclip, setviewpos) or other views' stale pixels stay on screen. The
		# player stands 128 units behind the eye: holding a saber, single player
		# draws the player's body in third person, around the eye.
		if [ "$sp" = 1 ]; then echo "exitview"; echo "wait 300"; echo "noclip"; fi
		# No console notify lines (the patched cgame logs NPC timing to the console).
		echo "set cg_draw2D 0; set r_drawentities $entities; set cg_drawGun 0; set con_notifytime 0"
		echo "wait 300"
		grep -v '^#' "$views" | awk -v m="$map" -v sp="$sp" '$1 == m {
			if (sp) printf "setviewpos %.0f %.0f %s %s\nwait 100\n", $3 - 128 * cos($7 * 3.14159265 / 180), $4 - 128 * sin($7 * 3.14159265 / 180), $5, $7
			printf "set cg_swjkjaView \"%s %s %s %s %s %s\"\nset cg_swjkjaTime %s\nwait 200\nscreenshot_png %s\nwait 100\n", $3, $4, $5, $6, $7, $8, $9, $2
		}'
		echo "quit"
	} >"$cfg"
	put "$cfg" capture.cfg
	rm -f "$cfg"
	echo "capturing $map"
	case $map in
	mp/*) game=(openjk.x86_64.exe +set g_gametype 1 +set sv_pure 0) ;;
	*) game=(openjk_sp.x86_64.exe +set g_spskill 0 +set cg_thirdPerson 0) ;;
	esac
	run_timed 300 "${game[0]}" "${OPENJK_SETTINGS[@]}" +set com_maxfps 60 \
		+set r_picmip 0 +set r_ext_compress_textures 0 +set r_DynamicGlow 0 \
		+set cg_fov 80 +set r_textureMode GL_LINEAR_MIPMAP_NEAREST \
		"${game[@]:1}" +devmap "$map" +exec capture >/dev/null
	cp -f "$(wslpath "$HOME_WIN")/$BENCH_GAME/screenshots/"*.png "$out/"
	# The patched cgame logs each NPC's animation timing once per view time.
	if [ "$sp" = 1 ]; then
		get qconsole.log | grep -a "swjkja npc" >"$out/$map-npcs.txt" || true
	fi
done
ls -l "$out"
