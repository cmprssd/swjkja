#!/usr/bin/env bash
# Records the benchmark demo (bench/reference/record_ffa3.cfg) with the reference
# OpenJK multiplayer client and copies it to bench/demos/. Only needed when the demo
# itself should change. Opens a game window for about 75 seconds.
set -euo pipefail
cd "$(dirname "$0")"
. ./lib.sh
put reference/record_ffa3.cfg record_ffa3.cfg
ms=$(run_timed 180 openjk.x86_64.exe "${OPENJK_SETTINGS[@]}" +set com_maxfps 60 \
	+set g_gametype 0 +set sv_maxclients 10 +set g_synchronousClients 1 +set fraglimit 0 +set timelimit 0 +set sv_pure 0 \
	+devmap mp/ffa3 +exec record_ffa3.cfg)
echo "openjk ran for ${ms} ms"
fetch demos/ffa3.dm_26 demos/ffa3.dm_26
ls -l demos/ffa3.dm_26
