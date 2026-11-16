#!/usr/bin/env bash
# Night-queue job: rebuilds the reference OpenJK with the patch (s_swjkjaClock) and
# captures academy2_students_walking twice with the sound clock on the server's time,
# held, to show whether jedi4's walk starts on the same frame in both runs.
# Results: $NIGHTQ_OUT/run{1,2}/ (png and academy2-npcs.txt).
set -euo pipefail
cd "$(dirname "$0")"
. ./lib.sh
out=${NIGHTQ_OUT:?run under nightq}
./build-openjk.sh
views=$(mktemp)
grep -v '^#' reference/walk-views.txt | awk '$1 == "academy2"' >"$views"
shots=$(wslpath "$WORK_WIN\\reference-shots")
for run in 1 2; do
	SCRIPTS=1 ENTITIES=1 WALK=1 SOUNDCLOCK=1 ./capture-reference.sh "$views"
	mkdir -p "$out/run$run"
	cp "$shots"/academy2_students_walking.png "$shots"/academy2-npcs.txt "$out/run$run/"
done
rm -f "$views"
