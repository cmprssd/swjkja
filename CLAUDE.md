# CLAUDE.md

swjkja is a new engine for **STAR WARS Jedi Knight: Jedi Academy** in Rust, ported from OpenJK (GPLv2; reference commit in `docs/ROADMAP.md`). Same game, same look, much faster. It ships no game data: it reads `GameData/base/*.pk3` from the owner's install (`D:\SteamLibrary\steamapps\common\Jedi Academy\GameData`).

## Read only what the task needs

The docs are large, and every line read stays in the context for the rest of the session:

- `docs/ROADMAP.md`: milestones and what's done (short; the milestone cells are long).
- `docs/DECISIONS.md` (1,100+ lines, dated `##` entries, newest last): list headings with `grep -n '^## ' docs/DECISIONS.md`, then read the entries you need by line range. Never read it whole.
- `docs/BASELINE.md` (2,300+ lines of measurements): the same way, by section. Add a section when you measure something.
- `docs/OVERVIEW.md`: how the project and its process work, for orientation.
- The OpenJK source: grep for the function you're porting; don't browse.

## Commands

```sh
cargo build --release                    # Linux
scripts/cargo-msvc.sh build|test|clippy  # Windows MSVC from WSL (its own target dir)
swjkja view <GameData> mp/ffa3           # map viewer (opens a window: see below)
bench/baseline.sh, bench/capture-reference.sh   # measurements and reference views
scripts/disk.sh [prune|clean-all]        # what the project stores, and bounding it
local-ci push -u origin <branch>; local-ci wait # PR checks (format, committed-tree build, Linux + Windows clippy and tests)
```

Retail-data tests read the install above; if the drive is unplugged `.local-ci` skips them and says so.

## Rules

- **Windows on the owner's desktop need a go-ahead.** The viewer, benchmarks and reference captures open game windows; `bench/lib.sh`'s `require_idle` refuses while the PC was used in the last 120 s. Batch GUI runs, say how long they take, and set `SWJKJA_ALLOW_WINDOW=1` only after the owner agrees to a run now.
- **Matching the original is measured**: a slice is done when its reference view matches (the score is in the PR) and, for speed work, when BASELINE.md shows it beats the original on the same scene.
- **A/B builds in worktrees:** `scripts/cargo-msvc.sh` builds the main checkout's target, and a stale shared target can reuse old crates. Give the worktree its own `CARGO_TARGET_DIR` and `cmp` the binaries before timing.
- `.gitignore` is a whitelist, so game data never gets committed: a new kind of file is ignored until you add it there deliberately (the committed-tree check in `.local-ci` catches a missed one).
- Decisions go in `docs/DECISIONS.md` as dated `##` entries; mid-build ones are marked "decided by Claude, pending review".
- Work autonomously, slice after slice, without stopping for status reports; stop only for a question the owner must answer. **But hand off at ~250k tokens of context** (the status line turns red), even mid-roadmap: save the handoff, then the next slice starts in a fresh session.
- Commit subjects are Conventional Commits with a crate scope: `feat(world): NPCs turn as their scripts ask (SET_DYAW, SET_VIEWTARGET) (#106)`.
