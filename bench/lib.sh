# Shared helpers for the benchmark scripts. Sourced, not run.
#
# The reference is OpenJK built from the pinned source (bench/build-openjk.sh), not the
# retail jamp.exe/jasp.exe: on current GPU drivers the retail executables crash on
# every map load (access violation at jamp.exe+0x91666), because the driver's
# GL_EXTENSIONS string (4 KB here) overflows a fixed buffer. OpenJK is the same engine
# and renderer with that fixed. See docs/DECISIONS.md.
#
# Paths are on the Windows side. Override with SWJKJA_GAMEDATA (the retail GameData
# folder, read only) and SWJKJA_WORK (where OpenJK is built, run and writes).
GAMEDATA_WIN=${SWJKJA_GAMEDATA:-'D:\SteamLibrary\steamapps\common\Jedi Academy\GameData'}
WORK_WIN=${SWJKJA_WORK:-"$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')\\.swjkja"}
OPENJK_WIN="$WORK_WIN\\openjk\\JediAcademy"
HOME_WIN="$WORK_WIN\\openjk-home"
# Everything a run writes (config, logs, demos, screenshots) lands in this mod folder
# under HOME_WIN; the retail install is only ever read.
BENCH_GAME=swjkja_bench

# Fixed settings for every run: assets from the retail install, windowed 1280x720,
# no vsync, no frame cap, sound muted, console log on.
OPENJK_SETTINGS=(+set fs_cdpath "$GAMEDATA_WIN" +set fs_basepath "$OPENJK_WIN"
	+set fs_homepath "$HOME_WIN" +set fs_game "$BENCH_GAME" +set logfile 2
	+set r_fullscreen 0 +set r_mode -1 +set r_customwidth 1280 +set r_customheight 720
	+set r_swapInterval 0 +set com_maxfps 0 +set s_volume 0 +set s_musicvolume 0)

ps() { (cd /mnt/c && powershell.exe -NoProfile -NonInteractive -Command "$1" | tr -d '\r'); }

# Copy a file from this repo into the bench mod folder (relative destination).
put() { # src dest
	ps "\$d=Join-Path '$HOME_WIN\\$BENCH_GAME' '$2'; New-Item -ItemType Directory -Force (Split-Path \$d) | Out-Null; Copy-Item -LiteralPath '$(wslpath -w "$(realpath "$1")")' -Destination \$d -Force"
}

# Copy a file from the bench mod folder into this repo.
fetch() { # relative-src dest
	ps "Copy-Item -LiteralPath '$HOME_WIN\\$BENCH_GAME\\$1' -Destination '$(wslpath -w "$(realpath -m "$2")")' -Force"
}

# Print a file from the bench mod folder.
get() { # relative path
	ps "Get-Content -Raw -LiteralPath '$HOME_WIN\\$BENCH_GAME\\$1'"
}

# Benchmarks open a game window, which takes over the screen. Refuse to start while
# the owner is using the PC: keyboard or mouse input within SWJKJA_IDLE_SECONDS
# (default 120). Set SWJKJA_ALLOW_WINDOW=1 once they have said a run may start now.
require_idle() {
	[ "${SWJKJA_ALLOW_WINDOW:-}" = 1 ] && return 0
	local need=${SWJKJA_IDLE_SECONDS:-120} idle
	idle=$(ps "Add-Type -Namespace W -Name U -MemberDefinition '[StructLayout(LayoutKind.Sequential)] public struct L { public uint cb; public uint t; } [DllImport(\"user32.dll\")] public static extern bool GetLastInputInfo(ref L l);'
\$l=New-Object W.U+L; \$l.cb=8; [void][W.U]::GetLastInputInfo([ref]\$l)
[int](([Environment]::TickCount - [int]\$l.t) / 1000)")
	if [ "$idle" -lt "$need" ]; then
		echo "The PC was used ${idle} s ago; not opening a game window. Ask first, then rerun" >&2
		echo "with SWJKJA_ALLOW_WINDOW=1 (or wait until it has been idle for ${need} s)." >&2
		return 3
	fi
}

# Run an OpenJK executable with arguments, wait for it (killing it after timeout_s),
# and print its wall time in ms. Arguments are passed as an array, so paths with
# spaces are safe.
run_timed() { # timeout_s exe args...
	local timeout_s=$1 exe=$2; shift 2
	require_idle || return
	local list="" a
	for a in "$@"; do list+="'${a//\'/\'\'}',"; done
	ps "\$ErrorActionPreference='Stop'
\$sw=[Diagnostics.Stopwatch]::StartNew()
\$p=Start-Process -FilePath '$OPENJK_WIN\\$exe' -WorkingDirectory '$OPENJK_WIN' -ArgumentList @(${list%,} | ForEach-Object { if (\$_ -match '\\s') { '\"' + \$_ + '\"' } else { \$_ } }) -PassThru
if (-not \$p.WaitForExit(${timeout_s}000)) { Stop-Process -Id \$p.Id -Force; throw '$exe did not exit within ${timeout_s} s; killed' }
\$sw.ElapsedMilliseconds"
}

# Like run_timed, but ends the game as soon as a line matching the regex appears in
# the bench mod folder's qconsole.log (OpenJK returns to the menu after a timedemo
# instead of quitting). The log is removed first so an old result cannot match.
run_until() { # timeout_s regex exe args...
	local timeout_s=$1 regex=$2 exe=$3; shift 3
	require_idle || return
	local list="" a
	for a in "$@"; do list+="'${a//\'/\'\'}',"; done
	ps "\$ErrorActionPreference='Stop'
\$log='$HOME_WIN\\$BENCH_GAME\\qconsole.log'
Remove-Item -Force -ErrorAction SilentlyContinue \$log
\$sw=[Diagnostics.Stopwatch]::StartNew()
\$p=Start-Process -FilePath '$OPENJK_WIN\\$exe' -WorkingDirectory '$OPENJK_WIN' -ArgumentList @(${list%,} | ForEach-Object { if (\$_ -match '\\s') { '\"' + \$_ + '\"' } else { \$_ } }) -PassThru
while (\$true) {
	if (\$p.HasExited) { throw '$exe exited before the result appeared' }
	if ((Test-Path \$log) -and (Select-String -Quiet -Path \$log -Pattern '${regex//\'/\'\'}')) { break }
	if (\$sw.Elapsed.TotalSeconds -gt $timeout_s) { Stop-Process -Id \$p.Id -Force; throw '$exe: no result within ${timeout_s} s; killed' }
	Start-Sleep -Milliseconds 250
}
Stop-Process -Id \$p.Id -Force
\$sw.ElapsedMilliseconds"
}
