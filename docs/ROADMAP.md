# Roadmap

Each milestone ships as one or more PRs with tests, and must beat the original game's
numbers from milestone 0 before it counts as done.

Reference source: OpenJK at `260c59c2907187af555a676fe0cc798893bf7757` (2026-09-29).
`code/` is single-player, `codemp/` multiplayer; about 1M lines of C/C++ between them.

| # | Milestone | Done when |
|---|-----------|-----------|
| 0 | **Baseline and setup** (multiplayer FPS and load time done, see BASELINE.md; frame-time spikes and single-player pending): Rust workspace and local CI; measure the original game's FPS, frame-time spikes and load times on fixed scenes; screenshot-comparison tool | The original's numbers are recorded in `docs/BASELINE.md` and reproducible with one script |
| 1 | **Assets** (**done**: `crates/fs`, `crates/bsp`, `crates/image`, `crates/shader`, `crates/console`): `.pk3` archives, virtual filesystem with the original's search order, `.bsp` maps, textures (TGA, JPEG, PNG), `.shader` scripts, cvars and configs | Every file in the retail `.pk3`s parses; tests pin the results |
| 2 | **Map viewer**: Vulkan renderer for world geometry, lightmaps, shader stages, fog, sky | Fly-through matches the original's screenshots; faster than the original on the same scenes |
| 3 | **Models**: Ghoul2 (`.glm`/`.gla`), MD3, skins, sabers and effects | Characters animate and match the original |
| 4 | **Single-player**: collision, movement, entities, ICARUS scripts, NPC AI, sound, UI, saves | The campaign is playable start to finish |
| 5 | **Multiplayer**: new protocol, server, client, bots | Matches play on Windows and Linux |
| 6 | **Mods**: asset mods load; Rust mod interface for code mods | Popular asset mods load unchanged |

## Measured hardware

Owner's PC: Intel Core i5-7400T, Intel HD Graphics 630 (integrated, Vulkan 1.3).
