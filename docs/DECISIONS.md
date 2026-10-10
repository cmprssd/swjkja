# Decisions

Product and technical decisions, newest last. Each entry: the question, the choice,
and why.

## 2026-10-07: Project direction (chosen by the owner)

- **Goal of "faster".** All of: higher and steadier FPS, faster loading, running well
  on weak hardware, and a modern memory-safe codebase. Every milestone is measured
  against the original game (see the baseline milestone in ROADMAP.md).
- **Starting point: a new engine in Rust,** not incremental work on OpenJK. Accepted
  trade-off: far more work before anything is playable, for a clean modern design.
- **Scope: single-player and multiplayer.**
- **Renderer: Vulkan, looking the same as the original.** Vulkan does not by itself
  change the look; the original's shader scripts, lightmaps, overbright lighting, fog
  and filtering are reproduced and checked by automated screenshot comparison against
  the original game. Visual extras are settings, off by default.
- **Game logic: all Rust from day one.** No original C code is linked in, not even
  temporarily, so the game is playable end to end only once gameplay is ported.
- **Multiplayer protocol: our own.** No compatibility with original or OpenJK servers
  and clients; the netcode is free to change for speed.
- **Platforms: Windows and Linux.**
- **Mods: asset mods (.pk3 maps, models, skins, sounds) load unchanged. Code mods are
  rewritten in Rust** against a Rust mod interface rather than run through the
  original 32-bit DLL interface: a mod must never cost performance, and rewriting a
  slow mod is preferred over supporting it as is.
- **License: GPLv2, porting from the released source.** Using Raven's and OpenJK's
  code as the reference makes this a derivative work. Game assets are never copied
  into the repository.
- **Repository: cmprssd/swjkja. Toolchain: Rust on Windows (MSVC, real GPU) and on
  Linux (WSL).**

## 2026-10-07: Reference source (decided by Claude, pending review)

The port follows OpenJK (github.com/JACoders/OpenJK) rather than the raw 2013 Raven
release: it is the same code with years of crash and portability fixes, and its
`code/` (single-player) and `codemp/` (multiplayer) trees map directly to the two
halves of the game. The reference revision is recorded in ROADMAP.md.

## 2026-10-07: Benchmark reference is OpenJK, not the retail executables (decided by Claude, pending review)

The retail `jamp.exe` crashes on every map load on the owner's PC (access violation
at `jamp.exe+0x91666`, three times out of three, with or without bots): the Intel
driver's GL_EXTENSIONS string is about 4 KB and overflows a fixed buffer in the 2003
engine, which OpenJK fixed with `TruncateGLExtensionsString`. The baseline is
therefore OpenJK built from the pinned source: the same engine and renderer with that
fix. It also never writes to the Steam install: it reads the retail assets through
`fs_cdpath` and keeps config, logs and demos under `%USERPROFILE%\.swjkja`.

Launching the retail executables directly also needs `SteamAppId=6020` in the
environment, or Steam's copy protection stops them with "Application load error
5:0000065434".

## 2026-10-07: JPEG decoding is close, not bit-exact (decided by Claude, pending review)

zune-jpeg (pure Rust) decodes the retail JPEGs within 5 of 255 levels of libjpeg
(mean 0.077, 13% of pixels touched); the other pure-Rust decoder, `jpeg-decoder`,
is not exact either. Bit-exact output would need libjpeg-turbo, a C library, in the
engine. Not worth it: the original renders textures re-compressed to S3TC/DXT by
default, which moves pixels far more than this. TGA and PNG decoding are exact.
If screenshot comparisons ever trace a visible difference to JPEG decoding, revisit.

## 2026-10-07: Shader parsing is split from resolving (decided by Claude, pending review)

The original parses a shader and loads its textures in one pass, failing the shader
when a texture is missing. swjkja-shader only parses: it fails exactly where the
original fails for a reason in the text, and returns texture names. The renderer
resolves a parsed shader against the loaded map and applies the remaining failure
rules (missing textures, `$lightmap` on a surface without a lightmap), which keeps
both halves testable without game data.

Not carried over: OpenJK's two hard-coded fixes for retail shaders (`gfx/2d/wedge`
and `gfx/menus/radar/arrow_w`, recognised by a hash of their text). They change how
those HUD elements look compared with the original; whether to keep them is a
question for the renderer milestone.

## 2026-10-07: Console follows OpenJK's command and cvar rules (decided by Claude, pending review)

swjkja-console reproduces the command buffer, tokenizer, cvar flags and built-in
commands of OpenJK, which are the retail rules plus fixes. Left out on purpose:
- OpenJK's range checks for some cvars (`Cvar_CheckRange`), which retail lacks;
  individual systems clamp values where they read them.
- The VM- and server-created cvar flags: there are no VMs (code mods are rewritten
  in Rust), and multiplayer uses our own protocol.
- The 128 KB command-buffer overflow limit.

Single-player uses `helpUsObi` (default 0) to unlock cheat cvars and multiplayer
`sv_cheats` (default 1, read-only, set by the server), as in the original.

## 2026-10-07: Renderer stack (decided by Claude, pending review)

- **`ash` (Vulkan directly), not `wgpu`.** The owner chose Vulkan for speed; `ash`
  gives full control over memory, synchronisation and pipelines, with nothing in
  between. Vulkan 1.3 with dynamic rendering and synchronization2 is required; the
  Intel HD 630, the Steam Deck and Mesa's lavapipe all have it.
- **Shaders in WGSL, compiled to SPIR-V by `naga` in build.rs.** Pure Rust, no
  external tools. (naga 30's GLSL frontend does not build on its own, and WGSL is
  naga's primary language.)
- **`gpu-allocator` for GPU memory.**
- **Tests render offscreen and read pixels back**, on the real GPU on Windows and on
  lavapipe (CPU) under WSL, so renderer tests need no window.

## 2026-10-07: One unreadable value in patch stitching (decided by Claude, pending review)

When `R_StitchPatches` walks an edge backwards, it passes the LOD error at index
`k + 1`; on the first step that is one past the end of the array, so the original
reads whatever memory follows. swjkja-world uses the entry at `k - 1` in that one case
(the value the code evidently meant) and `k + 1` everywhere else, as the original
does. It only affects at what distance one inserted vertex row may be dropped.

## 2026-10-07: Renderer conventions (decided by Claude, pending review)

- **Reversed, infinite depth**: depth is `z_near / distance` (1 at the near plane,
  0 at infinity), compared with greater-or-equal. More precise than the original's
  near/far range over big maps; it changes no visible ordering by itself.
- **The camera follows the original exactly**: `AngleVectors` axes, horizontal FOV
  fixed (`cg_fov`, 80) and the vertical one from the aspect ratio (cgame `CalcFov`;
  OpenJK's optional widescreen adjustment is off by default and not used).
- **One texture array for everything**, in an update-after-bind descriptor set: the
  Intel HD 630's driver allows only 200 images per shader stage in a normal set but
  1,048,576 in an update-after-bind one. Devices without that are refused at start-up
  with a clear message instead of crashing in the driver.
- **naga's y flip is off**: shaders work in Vulkan's clip space directly. A GPU test
  checks that up is at the top and left at the left.

## 2026-10-10: "Faster than the original" is measured on the same camera path (decided by Claude, pending review)

The reference's timedemo FPS includes bots, models, effects and HUD, which the new
engine does not draw yet, and our per-view numbers are fixed views. To compare like
with like, the reference build logs every frame's view, game time and a microsecond
timestamp during the ffa3 timedemo (`cg_swjkjaPath`, kept in memory and written at
shutdown), with entities and HUD off and at the screenshot comparison's quality
settings. The new renderer replays that exact path (`bench_path`). Both frame times
are the interval between the same point in consecutive frames, and both engines'
statistics come from one function.

Remaining differences, stated with every result: the reference still runs the demo
and cgame logic (light at r_drawentities 0) and presents to a window; the new
renderer draws offscreen and waits for each frame before the next (no overlap of CPU
and GPU work). The first favours us slightly, the second costs us.

## 2026-10-10: Two frames in flight (decided by Claude, pending review)

The renderer records a frame while the GPU draws the previous one (`FrameLoop`,
`FRAMES_IN_FLIGHT = 2`), with a copy of every buffer a frame rewrites per slot. Two
is the usual choice: it hides the CPU's recording and the submit/wait behind GPU
work, and adds at most one frame of latency. More slots would only help if CPU
frames varied by more than a GPU frame, which they do not here (recording takes
0.2-0.9 ms, the GPU 2.3 ms). The colour and depth images are shared between frames;
the GPU runs the frames in order and the full barriers between them keep that
correct. A retail-data test renders along the benchmark path and checks each frame
equals the same frame rendered alone (it fails if the frames share one slot).

## 2026-10-10: The map viewer's window (decided by Claude, pending review)

- **`winit` 0.30 for the window and input, `ash-window` for the Vulkan surface.**
  The usual pure-Rust pair on Windows and Linux; 0.31 is still in beta.
- **No vsync by default** (IMMEDIATE present mode), like the original's
  `r_swapInterval 0`; `--vsync` uses FIFO.
- **An 8-bit UNORM swapchain, never sRGB**: the renderer writes colours as the
  original does, without sRGB conversion; an sRGB swapchain would brighten them.
- **Frames are rendered into the offscreen target and copied to the swapchain
  image**, so the renderer, its pipelines and the tests are the same with and
  without a window. The copy is one of the suspects for the cost of presenting.
  (Changed on 2026-10-10, see below: in a window, frames are drawn straight into
  the swapchain image.)
- **Frame rate and frame time in the window title**, updated twice a second: there
  is no text rendering yet.
- **Controls like the original's noclip**: 250 units/s (`g_speed`), Shift for 3x,
  forward follows the view, mouse at 0.11 degrees per count (`sensitivity` 5).
  The viewer starts at the first player start, eyes 36 units up.

## 2026-10-10: Drawing straight into the window's image (decided by Claude, pending review)

- **The target's colour is 8-bit BGRA** (was RGBA), the format Windows swapchains
  offer, so a frame can be drawn straight into a swapchain image with the same
  pipelines. `Target::read_rgba` swaps the channels back, so tests and screenshots
  are unchanged.
- **In a window the viewer draws straight into the swapchain image; in exclusive
  fullscreen it draws into the target and copies** (`Swapchain::direct`, on by
  default). Measured on the Intel HD 630: in a window direct drawing saves the
  0.6 ms copy and the world costs the same; in exclusive fullscreen the world costs
  4.1 ms in the swapchain image against 2.5 ms in the target, more than the 0.8 ms
  copy. The copy stays as the fallback whenever the sizes or formats differ.
- **Replays draw their first view for 100 frames before the timed path starts**, to
  leave out the 68-160 ms stall right after the window opens (in fullscreen, the
  mode switch). The original's timed demo starts seconds after its own mode switch.

## 2026-10-10: The viewer's loading screen (decided by Claude, pending review)

- **The map loads on its own thread**, so the window shows frames and handles
  events (moving, closing, Esc) from the start. The loading thread uploads to the
  GPU while the window presents, so `Gpu::queue` is now a `Mutex` and every submit,
  present and `vkDeviceWaitIdle` (`Gpu::wait_idle`) takes it: Vulkan allows one
  thread at a time on a queue, and the type makes forgetting the lock impossible.
- **A progress bar on black, without text** (there is no text rendering yet): a
  track half the window wide with the part done filled, drawn with
  `vkCmdClearAttachments` (`Frame::fill`), so it needs no pipeline. The step under
  way and the percentage are in the title. The bar advances by step, weighted by
  each step's share of a load from a cold file cache (game data 36%, textures and
  pipelines 62%), when the bar is on screen long enough to matter.
- **The first loading frame is drawn as soon as the window exists**, inside
  `resumed`, rather than at the first redraw, which comes about 100 ms later while
  Windows sets the window up. Loading frames are then drawn at about 60 per second,
  to leave the CPU and GPU to the loading thread.
- **The mouse is grabbed once the map is ready**, so the window can be moved while
  it loads. Frame times, `--timings` and replays start when the map is ready.
- **Closing while loading just exits**: the loading thread is not cancelled, and
  ends with the process.

## 2026-10-10: Milestone 3 starts with the map's MD3 models

- **Asked: which models first?** Every MD3 the maps place (static models, then the
  breakables, racks, siege items and beacons), not only `misc_model_static`. The
  static models come first: 6,684 of the retail maps' roughly 7,300 MD3 placements.
- **Asked: where to check them against the original?** mp/ffa5 (84 static models),
  a multiplayer map, so the existing capture tooling works.

## 2026-10-10: How static models are drawn (decided by Claude, pending review)

- **Baked into world space at load**: the models never move, so each placement's
  surfaces become triangle soups that join the world's materials, draw order and
  per-frame index lists. Their normals keep the axes' scale, which is what the
  original's lighting sees when it turns the light into the model's scaled axes.
- **Lit through an entity table**: the frame's descriptor set has a fourth buffer,
  one light (ambient, directed, direction) and colour per entity, filled each frame
  for the visible models; a model's stages read their entity number from a spare
  lightmap coordinate. Entity 0 is the world (no grid light, white).
- **Free-for-all spawn rules** (`notfree`, `gametype "ffa"`), the viewer having no
  game type yet.
- **A model that does not load is skipped with a warning**, where the original stops
  the map load with an error. No retail map has one.
- **Model views are captured with entities drawn** (`ENTITIES=1`), the views placed so
  no item or mover is in sight.

## 2026-10-10: Single-player maps follow single-player rules

- **Asked: which models after the static ones?** The single-player maps' breakables
  and racks (about 1,100 placements), not the multiplayer siege items (about 50).
  A census of the retail maps found that multiplayer maps place no breakables,
  racks, beacons or planted bombs: all of them are on single-player maps, spawned by
  the single-player game code.
- **Asked: how does the viewer pick spawn rules?** By the map's path: maps under
  `maps/mp/` follow multiplayer rules (free-for-all), every other map single-player
  rules (`Rules::for_map`).
- **First step: the static models of single-player maps, drawn the single-player
  way** (6,210 placements). Single player spawns them in the game module, not cgame,
  and draws them differently: cgame culls by the PVS and a fixed 8192 units only;
  a view or cull point in solid draws nothing; the renderer culls unscaled models by
  their sphere, picks levels with
  `r_lodscale` 10 (multiplayer's is 5), prefers a fog that holds the whole model and
  lights the model at its cull point. Breakables come next, then racks and beacons.

## 2026-10-10: Single-player spawn details (decided by Claude, pending review)

- **Skill 0 (easy)**, `g_spskill`'s default, for the spawnflags that leave entities
  out per skill (256, 512, 1024). The viewer has no skill setting yet; the reference
  captures use the same skill.
- **A model's fog is fixed at load**, as seen from outside any fog. Single player's
  choice can depend on the fog the view is in when a model straddles fogs; a
  retail-data test checks that no retail placement's choice does.
- **Single-player reference views come from OpenJK's single-player executable**,
  patched like the multiplayer one to pin the view and game time
  (`bench/reference/openjk-view.patch`), in first person so the player stays out of
  sight.

## 2026-10-10: Single player's breakables (decided by Claude, pending review)

- **Breakables join the static models**, baked into world space the same way: the
  game spawns them standing still, and until something damages or uses them they
  do not move. They come first in the entity table, as cgame adds packet entities
  before the static models.
- **Culled as the server sends them**: a breakable is drawn when its linked box
  (origin plus mins and maxs, an epsilon larger) touches a cluster the view's
  cluster can see (`SV_AddEntitiesToPacket`), including the original's slip past 16
  clusters, then culled by the renderer like a single-player static model, and lit
  at its origin.
- **Initial state only**: START_OFF breakables are not drawn, and animated ones
  (AUTOANIMATE, 46 placements) draw their first frame, where the original cycles
  their frames by game time.
- **Breakables with "gravity" are left out** (4 on taspir2): the original drops them
  to the floor with a box trace at spawn, which needs collision (brushes, patches,
  terrain, solid entities). Three of them hang 2000 units above any floor, so drawn
  where placed they would float.
- **One model's fog depends on the view** (taspir1's Raven's Claw, a breakable that
  straddles fogs 1 and 2, takes fog 1 from a view inside it): its fog stays the one
  chosen from outside any fog, a known difference.
- **Reference captures in single player stand the player 128 units behind the eye
  and freeze NPCs** (`bench/capture-reference.sh`): a saber holder is drawn in third
  person, around the eye, and one capture with NPCs free found t3_stamp's crates
  already broken.

## 2026-10-10: Single player's racks, beacons and planted bombs (decided by Claude, pending review)

- **Racks draw their own model, whatever their "model" key says**, as the original
  does: `kejim/weaponsrack.md3` for a gun rack, `kejim/weaponsrung.md3` for an ammo
  rack (t3_stamp's racks name `imperial/weaponsrung.md3`, which no retail archive
  holds). A rack is linked with no box of its own (its origin, an epsilon larger).
- **The items on racks are drawn where they are MD3 models**: ammo packs and
  medpacks, turned 90 degrees from the rack and linked by their `items.dat` boxes.
  The weapons (blaster, repeater, rocket launcher) are Ghoul2 models and wait for
  Ghoul2; gun racks hold only weapons, so they show empty for now.
- **No random spread**: the original jitters each item (up to 2 units and 14 degrees
  for weapons, 0.5 units and 4 degrees for ammo, a medpack 4 to 8 units aside) with
  the game module's random numbers, whose sequence depends on everything that ran
  before. Items sit in the middle of their range, and a medpack takes the right
  side unless a weapon took it (the original picks a side at random).
- **Ammo racks' goods come after every map entity**: the original spawns them
  100 ms into the level, after the map's entities.
- **Beacons and planted bombs with a "targetname" are not drawn**, waiting to be
  used; every retail one (10, on t2_wedge) has one. One without would draw its first
  frame (a beacon animates in the original).
