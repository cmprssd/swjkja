#!/usr/bin/env bash
# Measures the reference (OpenJK, built by bench/build-openjk.sh) on the benchmark
# demo: average FPS from "timedemo" and map load time from CL_InitCGame. The first
# run warms the disk cache and is reported separately; the next RUNS are the result.
# Opens a game window for about 30 seconds per run. Prints a Markdown table row.
set -euo pipefail
cd "$(dirname "$0")"
. ./lib.sh
runs=${RUNS:-3}
demo=${DEMO:-ffa3}
put "demos/$demo.dm_26" "demos/$demo.dm_26"

one() { # -> "fps load_s wall_s"
	local ms log fps load
	ms=$(run_until 300 "frames, .* fps" openjk.x86_64.exe "${OPENJK_SETTINGS[@]}" \
		+set timedemo 1 +demo "$demo")
	log=$(get qconsole.log)
	fps=$(sed -nE 's/.* ([0-9]+) frames, ([0-9.]+) seconds: ([0-9.]+) fps.*/\3/p' <<<"$log" | tail -1)
	load=$(sed -nE 's/.*CL_InitCGame: +([0-9.]+) seconds.*/\1/p' <<<"$log" | tail -1)
	if [ -z "$fps" ] || [ -z "$load" ]; then
		echo "baseline: no timedemo result in qconsole.log" >&2
		tail -20 <<<"$log" >&2
		return 1
	fi
	echo "$fps $load $(awk "BEGIN{print $ms/1000}")"
}

median() { sort -n | awk '{a[NR]=$1} END{print (NR%2 ? a[(NR+1)/2] : (a[NR/2]+a[NR/2+1])/2)}'; }

read -r cold_fps cold_load _ < <(one)
echo "cold run: $cold_fps fps, load $cold_load s" >&2
results=()
for i in $(seq "$runs"); do
	r=$(one)
	echo "run $i: $r (fps load_s wall_s)" >&2
	results+=("$r")
done
fps=$(printf '%s\n' "${results[@]}" | awk '{print $1}' | median)
load=$(printf '%s\n' "${results[@]}" | awk '{print $2}' | median)
fps_all=$(printf '%s\n' "${results[@]}" | awk '{print $1}' | paste -sd' ')
echo "| $demo | $fps | $fps_all | $load | $cold_load |"
