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

Superseded on 2026-10-18 (below): JPEGs now decode as libjpeg 9a does.

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
- **The original's random spread**: the original jitters each item (up to 2 units
  and 14 degrees for weapons, 0.5 units and 4 degrees for ammo, a medpack 4 to 8
  units aside) with the game module's random numbers. First left out (items in the
  middle of their range); since 2026-10-21 replayed (see "Rack goods spread with the
  replayed random numbers").
- **Ammo racks' goods come after every map entity**: the original spawns them
  100 ms into the level, after the map's entities.
- **Beacons and planted bombs with a "targetname" are not drawn**, waiting to be
  used; every retail one (10, on t2_wedge) has one. One without would draw its first
  frame (a beacon animates in the original).

## 2026-10-10: Ghoul2 models, first for the racks' weapons (decided by Claude, pending review)

- **Ghoul2 meshes and skeletons load in `crates/model`** (`glm`, `gla`), checked like
  MD3s, and every retail `.glm` loads with its skeleton (a retail-data test). A mesh
  whose skeleton has another number of bones is refused; the original converts old
  72-bone humanoids, and no retail mesh is one.
- **A mesh is posed at frame 0 of its skeleton, from the renderer's root matrix**:
  `RootMatrix` hands every model `tr_ghoul2.cpp`'s `identityMatrix`, which is not the
  identity but a quarter turn about z (the mesh's -y forward). Without it the rack
  weapons lay flat; with it they stand upright like the reference.
- **The rack weapons are items drawn as the renderer draws Ghoul2 models**: culled by
  a sphere of the item's radius (20) around its origin, their level of detail and
  fog picked from the same sphere (`R_GCullModel`, `G2_ComputeLOD`,
  `R_GComputeFogNum`), lit at their origin. Surfaces are drawn as the hierarchy walk
  reaches them; tags and surfaces off by default are not.
- **Weapons stand as the rack places them**: a gun rack's three 9 units apart, an
  ammo rack's 4 to 10 units to a side, turned around and pitched 75 degrees up
  (cgame's `CG_Item` for a VERTICAL item), each spread at random as the other rack
  items (first in the middle of its range). An ammo rack with a weapon and no ammo checked places a blaster at
  the rack's own height, as the original does (its height variable stays 0).

## 2026-10-10: Single player's NPCs, as they spawn (decided by Claude, pending review)

- **Only the NPCs a map spawns by itself**: spawners without a "targetname" (245 on
  the retail single-player maps at the easiest skill). The others wait to be used,
  by triggers or scripts this engine does not run yet.
- **Each NPC's type follows its spawn function** (`SP_NPC_*`): its class and flags,
  and the spawner's "NPC_type" where the function keeps it (some replace it, as
  `NPC_Tavion_New` does). **A random choice takes the branch written first**: the
  "if" side of a coin flip (StormTrooper, not StormTrooper2), the first of several
  cases (a random Jedi is jedi_hf1). The original's choice depends on the game's
  random numbers, as the rack items' spread does.
- **Its look comes from the first `.npc` block of its type**, the files joined in
  the game's listing order (two names are defined twice: rebel2, stofficeralt):
  the Ghoul2 model, skin, surface switches, scale and box. The player's character
  ("Player") is the `g_char_*` defaults, jedi_tf with head_a1, torso_a1, lower_a1.
- **Left out**: vehicles (their model comes from `vehicles.dat`), the MD3 NPCs (two
  mice), `NPC_Galak` (spawns nothing) and typeless `NPC_spawner`s (random NPCs; no
  retail map has one that spawns at load).

## 2026-10-10: Single player's NPCs, drawn standing (decided by Claude, pending review)

- **NPCs are posed at the first frame of `BOTH_STAND1`**, the animation `NPC_Begin`
  starts them in, read from their skeleton's `animation.cfg`, not at frame 0: frame
  0 of the humanoid skeleton belongs to a facial animation (`FACE_ALERT`). A
  skeleton without that animation stays at frame 0, as the original leaves it
  unanimated.
- **Drawn as cgame draws players** (`CG_Player`): the skin on (its "*off" surfaces
  hidden), the `.npc` block's surface switches, yaw only, axes scaled and the model
  lifted 24 units per unit of height scale; culled and leveled by `s.radius` times
  the largest scale, fogged by the radius alone, lit at the origin. The skin's
  shaders register after the mesh's (`NPC_PrecacheByClassName`).
- **Known differences, left for later steps**: no weapons or sabers in their hands,
  and so not the weapon-ready torso the original's first frames give them; no
  floor drop (the original runs a movement frame that drops an NPC to the floor;
  spawners placed above it float, 8 units on t2_dpred); no `customRGBA` tint (its
  retail uses are random colours); no blob shadows; MD3 NPCs (mice) and vehicles
  not drawn.


## 2026-10-11: Single player's NPCs hold their weapons (decided by Claude, pending review)

- **What an NPC holds is what it holds as it spawns**: its `.npc` block's "weapon",
  or, for none or a name the game does not know (`WP_BLASTER_RIFLE`), the weapon its
  team and type give (`NPC_WeaponsForTeam`). Weapons are bolted to the hand as
  `NPC_SetMiscDefaultData` bolts them, by team: none for good Jedi, saber-wielding
  enemies, flying droids or neutrals; a reborn officer's blaster pistol in both
  hands. A saber wielder holds its saber's hilt from the `.sab` files (a second in
  the left hand unless either is two-handed). Weapon models are the `weapons.dat`
  models renamed as the game renames them (`blaster.md3` to `blaster_w.glm`).
- **Bolted models hang from the carrier's tag**: the tag surface's first three
  vertices, posed, give the bolt's axes and origin (`G2_ProcessSurfaceBolt2`); the
  bolted model is posed at frame 0 of its skeleton from there. It is part of the
  carrier's entity: culled, fogged and lit with it, its level of detail picked from
  its own number of levels.
- **NPCs stand as their first movement frames leave them with no enemy**, their
  weapon idle (`NPC_Begin` leaves it `WEAPON_IDLE`, and the AI keeps it so without
  an enemy): legs apart (`BOTH_STAND3`) with a gun between the saber and the
  detonation pack, `BOTH_STAND4` for an imperial with none, `BOTH_STAND1` otherwise;
  the torso in its weapon's idle animation (`TORSO_WEAPONIDLE3` for blasters and
  rifles, `TORSO_WEAPONIDLE2` for pistols, ...). A saber's blade is off, so the
  torso follows the legs. The legs' animation plays on bone `model_root`, the
  torso's on `lower_lumbar` and `Motion`, every other bone playing its parent's. Each
  plays the first frame of its animation; the original's frame depends on how long
  the animation has played.
- **The NPC reference views are captured with NPCs frozen from the first frame**
  (`bench/capture-reference.sh` sets `d_npcfreeze 1` first). Captured as before,
  after about 600 frames of AI, NPCs had turned (vjun2's officer by 90 degrees),
  which nothing at spawn decides. Frozen, the original still runs their movement
  frames (the stance, the drop to the floor) and turns them to their spawn yaw only.
- **Left out**: an NPC that matches the player's weapon (type "Player"; it holds
  nothing here), the scepter's second model, bone bolts (every bolt used is a tag),
  the look angles cgame turns the torso and head by (zero for a standing NPC facing
  ahead), and a saber's blade.

## 2026-10-11: Single player's NPCs and "gravity" breakables dropped to the floor (decided by Claude, pending review)

- **Box traces against the map's brushes and the solid entities**
  (`crates/world/src/collision.rs`), as `CM_TraceThroughBrush` runs them: a box
  stops where it would first enter a brush whose contents meet the mask, held 0.125
  units (`SURFACE_CLIP_EPSILON`) off the face it hits. Every brush is tested against
  the sweep's bounds instead of walking the leaves the sweep crosses, which tests the
  same brushes.
- **What is in the way**: the world model; the `func_` brush entities that spawn,
  at their origin, each brush with its own contents (`SV_SetBrushModel`), except a
  `func_usable` or `func_wall` started off; and the SOLID breakables (spawnflag 1),
  each from where it spawns for what spawns after it, NPCs spawning after all of
  them. Left out: patches, NPCs in each other's way, brush entities' rotation and
  doors that start open. With the world alone, NPCs on kor2's and t3_stamp's crates
  and `func_breakable` floors fell through them.
- **An NPC stands where its first movement frames leave it**: lifted out of a
  floor its box's lower half sinks into (`G_CheckInSolid`, at spawn), then fallen
  straight down onto the floor below with what blocks NPCs (`MASK_NPCSOLID`). With
  no sideways speed, the original's fall (`Pmove` with gravity until
  `PM_GroundTrace` finds ground) ends where that trace does. A stuck NPC, or one
  over nothing, stays at its spawner. The spawner's DROPTOFLOOR trace is not run
  apart: it uses the spawner's box, and the fall that follows ends on the same
  floor.
- **A breakable with "gravity" is traced down from a unit above its origin**
  (`misc_model_breakable_gravity_init`) and stays where it lands. The four on
  taspir2 (scripted crates, thrown later) land 24 and 40 units below where they are
  placed: the note in the breakables entry above, that three hang 2000 units above
  any floor, was wrong (a `bespin/n_floor01` brush lies under all three).

## 2026-10-11: Single player's NPCs at the frame the original reaches (decided by Claude, pending review)

- **The reference captures pin the original's ghoul2 clock too**: with
  `cg_swjkjaTime` set, the client poses ghoul2 models at that time
  (`G2API_SetTime`), as the cgame already pinned the shader clock. Asked first (it
  meant rebuilding OpenJK and opening game windows): before, a looping or held
  animation's frame depended on how long the capture ran, which no rule can
  reproduce. The patched cgame also prints each NPC's legs and torso animation as
  ghoul2 holds it (start time, frames, speed), which `capture-reference.sh` keeps as
  `<map>-npcs.txt`; console notify lines are off (`con_notifytime 0`) so the log
  stays out of the screenshots. The three NPC views were captured again.
- **NPCs standing still play their stance's idle animation after five seconds**
  (`G_CheckClientIdle`): on the first server frame at which the level's time is more
  than 5000 ms past the NPC's `idleTime`, both parts play `BOTH_STAND1IDLE1`,
  `BOTH_STAND3IDLE1` or `BOTH_STAND5IDLE1` for those stances (`BOTH_STAND2` picks
  one of two idles at random: the first), held for its length less a frame; the
  stance comes back, and the next idle 5 s after its full length (plus 0 of a
  random 0 to 2 s). The log found every NPC in the three views in its idle, not its
  stance.
- **Times**: the server's first frame is at 1550 ms and frames are 50 ms apart
  (`SV_SpawnServer`, `sv_fps` 20). `PM_SetAnimFinal` starts an animation at the
  client's time when it has one and the level's otherwise; the client draws by a
  clock one frame behind the level's (measured: idles the level starts at 5050 start
  at 4989-5003 on the client's), so an idle starts 50 ms before its server frame,
  and the stance at 1550 (no client time yet). The views' time is the client's.
- **`idleTime` stays 0 for an NPC still from the start; one that lands on a slope
  moves on its first frame (1550)**: measured with a temporary log of every NPC's
  state over the first second (not kept). NPCs that fell 8 units onto flat floor
  were already still; the three that landed on sloping ground or a ramp had their
  `idleTime` set at 1550, consistent with landing clipping the fall's speed into
  the slope's plane. `settle` reports it: a fall of more than a quarter unit
  (`PM_GroundTrace`'s reach) onto a plane that is not flat. Longer falls (more
  than one frame in the air) are not modelled.
- **Frames**: ghoul2's `G2_TimingModel`: frame = first + elapsed / 50 ms x speed (50 /
  the animation's ms per frame), wrapping over all its frames when it loops (the
  last lerping into the first), held on its last when it does not; each bone's
  transform is lerped component by component toward the next frame
  (`CBoneCache::EvalLow`). Blending from the previous animation (100 ms) is left
  out; animations played backwards stay at their start (no stance or idle does).
- **Models are posed once, when the map loads, at a given time**: `render_world` at
  the map's first view's time (every NPC view is at 12345 ms), the viewer at 0.
  Animating them as time passes is the next step.

## 2026-10-11: Single player's NPCs posed every frame, skinned on the GPU (decided by Claude, pending review)

- **The renderer poses each NPC it draws at the frame's time**: per frame, each
  visible NPC's bones (its skeleton at the frames its animations reach, then each
  bolted model's from its bolt) go into a per-frame table, placed by the NPC's
  origin and axes; the vertex shader skins its surfaces by them as
  `RB_SurfaceGhoul` does (weighted bones for the position, the first bone for the
  normal). A posed surface's vertices stay in the model's space in the static
  vertex buffer, their bones and first three weights in the lightmap coordinates
  model surfaces do not use, and draw apart from the other surfaces of their
  material, just after them. `render_world` now poses each view's NPCs at that
  view's time, not the map's first view's.
- **Why the GPU, not the original's CPU skinning**: skinning on the CPU (rebuilt
  per frame, copied to a per-frame vertex buffer) cost about 0.25 ms of CPU per
  NPC and, on the Intel HD 630 this is measured on, as much again on the GPU:
  1 ms of busy-waiting per frame alone slowed the GPU by 1-2 ms (the two share
  power). kor2's view of four NPCs went from 8.5 to 11 ms a frame. With the
  GPU's skinning it stays at main's (8.4-8.9 ms); the CPU computes about 70 bones
  per NPC.
- **Left as they were loaded**: an NPC's surfaces whose shader has vertex
  deforms (drawn through the CPU deform path) keep the pose of the time given at
  load. Bounds and culling still use the load's pose; the sphere NPCs cull by
  does not depend on it.

## 2026-10-11: Single player's NPCs blend between animations (decided by Claude, pending review)

- **A part whose animation changes blends from where the one before had reached,
  over 100 ms** (`SETANIM_BLEND_DEFAULT`, which the idle's `NPC_SetAnim` and the
  stance's `PM_SetAnim` pass): `G2_Set_Bone_Anim_Index` keeps the old animation's
  frame at the new one's start (`blendFrame`, and `blendLerpFrame` the frame after,
  wrapped or held as the old animation would), and `G2_TransformBone` lerps each bone
  from that toward the new animation's frame by the time since over 100 ms. Bones
  with no animation of their own blend as their parent does (`CBoneCache::EvalLow`).
- **The original's weights are kept, though reversed**: it weighs `blendFrame` by
  its own fraction and `blendLerpFrame` by the rest, so a part replaced a quarter
  past frame 101 blends from a quarter of 101's pose and three quarters of 102's,
  not the other way round. Copying it matches the reference; the difference is
  under a frame.
- **Left out**: the blend into the first frame's stance from the `BOTH_STAND1`
  `NPC_Begin` starts (over by 1650 ms); a part blends neither into an animation
  played backwards (`PM_SetAnimFinal` zeroes the blend) nor into one its skeleton
  lacks (nothing changes then).

## 2026-10-11: No single player NPC surface has vertex deforms; NPC culling does not depend on the pose (measured by Claude)

- **Measured, not changed**: the NPCs every retail map spawns draw 19,242
  posed surfaces over all their levels of detail (body, weapons, sabers' hilts);
  7,765 have a shader definition and none of those has `deformVertexes`. In the
  retail shader text, deforms are on world textures, flares, fogs, effects and the
  powerup shells cgame draws over models (not drawn here), never on a model's own
  shader. So the CPU deform path, which would keep the pose given at load, draws no
  NPC surface, and posing them there every frame is left undone until one needs it.
- **Culling is already right**: NPCs are culled, leveled and fogged by a sphere of
  their radius around their origin (`R_GCullModel`), as the original does, so their
  pose changes nothing. Their levels' bounds (the load pose's) are not used to
  cull them.

## 2026-10-11: Brush entities drawn where they spawn (decided by Claude, pending review)

- **The renderer draws the map's brush entities** (`bsp.models[1..]`) as the game
  spawns them (`swjkja_world::brush_entity`): func_door (at pos2 when it starts
  open), func_plat (at the bottom), func_static (turned by its angles, the only
  class that keeps them), func_rotating (at zero), func_breakable, func_glass, and
  func_wall and func_usable unless started off. Rail movers (`EF_NODRAW`) are not
  drawn. Each is sent by the server's PVS of its linked box (or broadcast), culled
  by its model's box (`R_CullLocalBox`), then each surface as the world's. Locked
  doors and other `EF_SHADER_ANIM` entities hold their animMap at frame 0.
- **Placed once at load**: vertices, normals and planes are moved to where the
  entity stands; moving them is game logic (roadmap 4). Rotated entities cull by
  the placed surfaces' looser box.
- **Reference views**: stamp_crates 5.00 -> 0.25 mean diff (13.8% -> 0.0% off).
  stamp_barrels 0.13 -> 0.32: t3_stamp's `big_door` is opened by the intro's
  scripts. ffa5_lamps 0.17 -> 0.21: of two identical doors, the left now matches
  the reference, the right was open in the capture (game state after spawn).
  ffa3_spawn3 differs where the world-only reference leaves out a platform.
- **Cost**: bench_world within main's spread (three runs of 600 frames on the
  changed views); load up to 0.2 s more on t3_stamp.

## 2026-10-11: Brush entities draw their secondary models (decided by Claude, pending review)

- **A brush entity with a "model2" key also draws that model** at its origin and
  angles, after its brushes (`InitMover`, `InitBBrush`; cgame's `CG_Mover`), sent
  by the PVS of the brush's linked box and culled, lit and fogged as the other
  entity models (lit at its origin). Single player turns it further by
  "modelAngles" when they are not zero and makes a `.glm` a Ghoul2 model of radius
  120 at frame 0 of its skeleton; multiplayer has no "modelAngles", draws no
  `.glm`, and scales a func_static's or func_rotating's model by "model2scale"
  percent. No retail map uses a `.glm` or "model2scale".
- **Retail**: 74 drawn in single player (103 entities, less 29 usables that start
  off), three in multiplayer (siege_desert's dishes, spawned in free-for-all).
  t3_stamp's four cranes with no origin key draw at the world's origin, as the
  original's do. None is broadcast (the forcevisible ones have an origin).
- **Left out**: a func_usable's AUTOANIMATE and ANIM_ONCE frames (no retail model2
  usable has them); the brush entities' own movement (game logic).
- **Reference views** (bench/reference/model-views.txt, new): danger_rock 9.00 ->
  5.52 mean diff (11.2% -> 3.7% off; the rest is two distant peaks, as on main),
  rancor_gates 15.87 -> 0.87 (23.0% -> 0.9%), bounty_bomb 1.77 -> 1.17 (3.8% ->
  0.5%). The 30 views of the earlier list are unchanged.
- **Cost**: bench_world on these views and t3_stamp's within main's spread
  (three runs of 600 frames each).

## 2026-10-14: Rotating brush entities turn at the view's time (decided by Claude, pending review)

- **A func_rotating that turns is drawn at its angles at the view's time**:
  "speed" degrees a second (100 when 0) about yaw, roll (spawnflag 4) or pitch
  (spawnflag 8), from zero at time 0 (`SP_func_rotating`: `TR_LINEAR` with
  `trTime` 0; `BG_EvaluateTrajectory` in floats). Single player turns only those
  that start on (spawnflag 1); multiplayer turns them all, by "spinangles" when
  given. Turning, it links a cube of its radius (the server relinks it at its
  angles every frame).
- **Placed by the GPU**: its surfaces stay in its model's space, drawn by their own
  draws after their material's, turned by one bone of the frame's bones
  (`R_RotateForEntity`); each frame it is culled by its model's box at its angles
  and its surfaces in its model's space (the view's origin and frustum moved
  there, as `R_CullSurface` with `or.viewOrigin` and `R_CullLocalBox`).
- **Retail**: nine, all single player and started on: t3_hevil's four big fans
  (yaw, 300) and four small ones (pitch, 850), t2_rancor's drum (pitch, 350). None
  has a "model2", so a turning entity's model2 still stands at zero (left out).
- **Reference capture**: `openjk-view.patch` also evaluates a turning mover's angles
  at `cg_swjkjaTime` (movers turned at the server's time before, so captures could
  not be compared). Five new views in `bench/reference/model-views.txt`: mean diff
  hevil_fan_low 2.31 -> 0.39 (5.9% -> 0.3% off), hevil_fan_west 4.30 -> 0.99
  (10.9% -> 0.8%; the rest is rain the original draws as effects), hevil_wall_fan
  1.39 -> 0.27 and at 5000 ms 1.33 -> 0.28 (4.0% -> 0.0%), rancor_drum 0.65 ->
  0.44 (0.4% -> 0.0%). The 34 earlier views (all but hoth2's and vjun1's, which
  lose the device) render pixel for pixel as on main.
- **Cost**: bench_world on the five views within main's spread (three runs of 600
  frames each); t3_hevil draws 465 materials instead of 383 (each fan's surfaces
  are drawn per entity).

## 2026-10-15: What a standing NPC does next: its spawnscript first (chosen by the owner)

- **First slice: the state spawnscripts set**, not AI movement. 104 of the 245 NPCs
  the retail maps spawn at load run a spawnscript (compiled ICARUS, `.ibi`); most only
  set state: animations and how long they hold, the weapon, visibility. AI movement
  (looking around, wandering, patrols, navgoals) comes later.
- **The ICARUS reader reads every retail script** (all 1,011 `.ibi` files, pinned by a
  retail test), as milestone 1 read every asset; only what spawned NPCs reach runs.
- **Look targets (`SET_LOOK_TARGET`, heads turned toward Luke) are a follow-up PR.**
- **New reference captures were approved** (game windows on the owner's PC).

## 2026-10-15: Spawnscripts run, a server frame at a time (decided by Claude, pending review)

- **One timeline for every NPC** (`swjkja_world::script::timeline`): the game stepped
  frame by frame in the original's order, the NPC's movement frame (`Pmove`'s timers,
  the stance returning to parts no longer held, `G_CheckClientIdle`) then its script
  (`CTaskManager::Update`: commands until one waits), skipping frames where nothing
  can change, up to an hour of game time (then the last animations hold). It replaces
  the closed form for standing NPCs (`standing_at`), which it reproduces exactly
  (checked at every 7 ms up to 200 s for three stances, idle lengths, a missing idle,
  and NPCs still or falling on the first frame).
- **What runs**: `set` of animations and hold times (a script's animation task
  completes when its timer runs out, as `PM_SetLegsAnimTimer` completes it), timed
  waits, waits on task groups (`task`, `do`), loops, `if`/`else`. A script's first
  pass runs in `NPC_Begin`, on the first server frame (1550 ms: spawners think 350
  ms after the map spawns, `NPC_Begin` 100 ms later), and its animations start then
  on the client's clock, as the capture log shows (1550); later ones a frame behind
  the server, as the idle's do.
- **Random numbers take their range's low end**, as other random choices here take
  their first branch. The original's differ per NPC and per run (the C library's
  `rand()`, seeded from the clock); the captures pin them to the same low end (see
  2026-10-16, "Script random numbers pinned in the captures").
- **Weapon and visibility from the first pass only**: `SET_WEAPON WP_NONE` drops every
  bolted model and gives the unarmed stance (70 NPCs), `SET_INVISIBLE true` leaves the
  NPC out (9). A retail test checks no spawned NPC's script sets either later.
- **Left out**: commands that move or turn the NPC (`SET_NAVGOAL`, `SET_DYAW`, ...)
  never complete, as with the AI frozen; `affect` (another entity), `run` (no spawned
  NPC's script runs another), signals, `get` (a condition with one is false), and
  other weapons than none. Dead NPCs (`SET_HEALTH 0`, t1_fatal's corpses) are left
  for the death code.
- **Reference captures run scripts with the AI frozen**: `d_npcfreeze 1` skips the
  script's later updates too (`NPC_Think` returns before them), so the capture patch
  adds `d_npcfreeze 2` (AI frozen, scripts run after the movement frame, as an
  unfrozen think runs them) and `g_swjkjaStop` (the game runs no frame past a level
  time), and `SCRIPTS=1 bench/capture-reference.sh` stops the game at the view's
  time. yavin1's capture never reaches the forced view (its intro, as kor1's), so its
  talking students are not compared.

## 2026-10-15: What NPCs look at: the settled look, head bones only (chosen by the owner)

- **The settled look, not the turn toward it.** cgame turns an NPC's head toward its
  look target a part of the way each rendered frame (15% scaled by the frame's
  interpolation), measuring from its eyes as last drawn, so the look settles where
  the angles from the turned eyes equal the look (about half the way to the target).
  That settled look is drawn from the moment the script sets the target, the same
  at any frame rate; the original takes about a second to get there.
- **Look targets only.** The thoracic, cervical and cranium bones turn
  (`CG_G2ClientNeckAngles`); the spine's motion-bone correction (`cg_motionBoneComp`
  2, which turns the lumbar bones of NPCs whose legs and torso play different
  animations) is left for later, reusing the same bone overrides.

## 2026-10-16: Look targets resolve to NPCs; the target's eyes as it stands (decided by Claude, pending review)

- **A look target is the first NPC of that name** (`NPC_targetname`, ignoring case).
  Every retail look target (53 lookers on academy1, 2, 4 and 6, all at
  `npc_luke1`) names exactly one spawned NPC and no map entity, which a retail test
  checks; other entities and the player as targets are left out until a map needs
  them.
- **The target's eyes are where they are as it stands, its own look left out**, so
  two NPCs looking at each other need no joint solve (no retail target looks at
  anyone).
- **Only humanoid skeletons turn** (bones "thoracic", "cervical", "cranium"); every
  retail looker is one. The other classes' bone shares (droids, the rancor) are
  left out.
- **Looks set by other scripts (`affect`) are left out**, as their animations are:
  two of academy2's students and Luke get theirs that way.

## 2026-10-16: Per-map cinematic skeletons, checked on four academy views (chosen by the owner)

- **Every single-player map's cinematic skeleton loads** (`_humanoid_<map>.gla` and
  its animation file, 32 retail maps), not only academy2's. Spawned NPCs play
  cinematic animations at load on academy1 (the teachers), academy2, academy4 and
  academy6 (the students; academy4's and 6's run academy2's scripts on their own
  skeletons, so the same `BOTH_CIN_*` is another motion there).
- **New reference captures**: academy4_class and academy6_class (the same
  classroom view as academy2's); academy1_class already shows the teachers.

## 2026-10-16: How the cinematic skeleton plays (decided by Claude, pending review)

- **The whole body switches skeletons**, as `G2API_SetAnimIndex` does: an
  animation from the cinematic file plays on the cinematic skeleton, any other on
  the humanoid's, and a switch stops every bone's animation (the other part then
  plays its parent bone's frames) and blends nothing. A change on both parts at
  once sets the torso first (`PM_SetAnimFinal`); separate changes in one frame are
  taken in that order too (the timeline does not keep the script's order).
- **The renderer takes the map's path, not its rules** (`WorldRenderer::new`,
  `StaticModels::load`): the rules come from it (`Rules::for_map`), as does the
  cinematic skeleton's name.
- **academy1's look test uses the original's point for Luke's eyes**, as
  academy2's: his spawnscript's `BOTH_CIN_4` now plays (before, missing, he stood
  in `BOTH_STAND1`), while another script (`affect`, not run here) has him stand in
  `BOTH_STAND4` from 1950 ms in the original.

## 2026-10-16: Level scripts and their cutscenes run, the player beginning at 2000 ms (chosen by the owner)

- **The intros run as cutscenes** (not only their first pass): the map's script
  runners with the NPCs, `affect` blocks over time, signals, task waits, and voice
  lines lasting their sound files (WAV or MP3); cameras and subtitles left out, and
  walking (navgoals) not done, as with the AI frozen.
- **The player enters the world at 2000 ms**, the first frame a fast load gives
  (academy1's first capture). In the original it is whenever the client has loaded,
  so the captures pin it: the patched server holds its time there until the client
  enters (`sv_swjkjaBegin`), and captures with `SCRIPTS=1` let the cutscene run
  instead of skipping it (skipping froze the actors at whenever the skip came).
- **Captures stand the player off NPCs** (a views file's tenth column): the old
  placement killed academy2's, 4's and 6's Luke.

## 2026-10-16: How level scripts run (decided by Claude, pending review)

- **Script runners update before NPCs in each frame.** The original updates
  entities by number, which is not the map's entity order (academy2's intro runner
  is entity 3, between Rosh and the other students); runners first matches the
  logged trace for every actor the academies' intros touch but Rosh, whose intro
  block only waits for a signal.
- **What the player begins with**: the targets of the first player start
  (`SelectSpawnPoint` picks one at random) and of the `trigger_once` and
  `trigger_multiple` boxes its box stands in at the spawn point (academy4's intro);
  triggers that need a button, a facing or an NPC, or start inactive, are left out,
  as are runners that run on their activator (the player is not run here).
- **A voice line plays out on the first frame from its start plus its length**
  (the client's mixer, in real time, decides in the original); a line whose file
  is missing (four retail ones, missing from the game too) plays out at once.
- **Moves are left out**: `SET_ORIGIN`, `SET_ANGLES` and the like complete without
  moving the NPC (the academies' views reach none before their time).


## 2026-10-16: The game's random numbers replayed while a map spawns (chosen by the owner)

- **The question**: the academies' random Jedi (`NPC_Jedi` spawn flag 4) and their
  tints ("customRGBA jedi_hf" and the like) differed from every capture. Take the
  low end, seed a capture, or log the original's picks?
- **Found**: single player never seeds the game module's generator (`Rand_Init` has
  no caller; `holdrand` starts at `0x89abcdef`), so the picks are the same every
  run: a capture patch addition (`g_swjkjaRand`) logged every draw with its caller,
  and the four academies' random Jedi come out kdm2, hm1, hf2, (tf1 drawn again: the
  player's character's), hf2, zf1, rm2, zf1, as replaying the generator gives.
- **Chosen: replay the generator** (`swjkja_world::rand::Rand`) through the spawn:
  the spawn functions' random types and coin flips (Jedi, prisoners, stormtroopers,
  ugnaughts, grans, weequays, Bespin cops, shadowtroopers, random saber cultists)
  and each player start's saber precache, in entity order, then each NPC's
  `NPC_ParseParms` as it spawns (its "customRGBA", its sabers' and its own random
  saber colours, the player's character's saber). A retail test checks the
  academies' random Jedi and tints against the logged draws; the views' tinted
  NPCs now match the captures.
- **NPCs draw with their colour** (`shaderRGBA` = `customRGBA`, white by default),
  as cgame does; other models keep it zeroed.

## 2026-10-16: What the replay leaves out (decided by Claude, pending review)

- **Other entities' spawn draws** (effects, breakables and the like) are left out:
  none of the academies' entities draw before their NPCs, but a map whose do would
  shift every NPC pick after them. The racks' draws are replayed (2026-10-21).
- **Draws after the spawn are not replayed**: once the client runs, cgame draws from
  the same generator every frame it draws, so what the game draws then depends on
  frame timing. Scripts' random numbers do not come from it (see 2026-10-16,
  "Script random numbers pinned in the captures").
- **A spawner's misnumbered "saberColor" key** (a blade number outside 2 to 8), which
  the original reads past without its value, draws nothing here (no retail block
  has one).

## 2026-10-16: NPCs' blob shadows (chosen by the owner)

- **The question**: the captures run with `cg_shadows` at its default, 1 (blob
  shadows). Draw only those, or other modes too; and lay them as the original does,
  or as a simple quad?
- **Chosen: blob shadows only, laid as the original lays them**: `_PlayerShadow`'s
  box trace down through the world (`MASK_PLAYERSOLID`, the world model only, as
  `cgi_CM_BoxTrace` with model 0), `CG_ImpactMark`'s square turned with the legs, and
  `R_MarkFragments` chopping it onto the world's surfaces (`swjkja_world::mark`).
  Stencil and projected shadows (`cg_shadows` 2 and 3) are left out.

## 2026-10-16: How shadows are laid (decided by Claude, pending review)

- **A cinematic NPC's shadow comes from its eyes**, as the original's does: it reads
  the bolt numbered `rootBone`, and `rootBone` is a bone list number (0, the first
  bone added). Checked against the captures (closer than from the origin on all
  three cinematic views).
- **The surfaces a mark can land on** are every world surface whose bounds meet the
  projection's box, not those of the BSP leaves it touches, and without the 64
  surface cap. The chop leaves nothing of a surface outside the projection either
  way.
- **The trace leaves out patches** (the collision here has none), as NPCs' drops do.
- **Polygons pick their fog as seen from outside any fog** (`RE_AddPolyToScene`'s
  partial-fog rule needs the view's fog), as static models do.
- **The `markShadow` material is created after the world's**, before the models'
  (cgame registers it after loading the world), which orders it among other
  sort-4 (decal) materials.
- **Left out**: the rancor's and the AT-ST's larger shadows (64 units, and the
  AT-ST's feet), cloaked NPCs and sand creatures (none), the player's own shadow.


## 2026-10-16: The spine turned against the "Motion" bone, checked on a new pistol-stance view (chosen by the owner)

- **The question**: `cg_motionBoneComp` 2 changes only NPCs whose legs and torso play
  different animations, which none of the academy views has, and the trooper views'
  rifle stance turns by under 0.02 degrees. Check it on a new capture at a time an
  NPC stands in a pair that turns, or on the existing views only?
- **Chosen: a new capture.** vjun2's imperial at 3000 ms (`vjun2_imp_stance`,
  `SCRIPTS=1` to stop the game before his idle), whose pistol stance (`BOTH_STAND3`
  legs, `TORSO_WEAPONIDLE2` torso) turns about 30 degrees: 1.86 -> 0.82.

## 2026-10-16: How the spine turns (decided by Claude, pending review)

- **The original's rule, as at `cg_motionBoneComp` 2**: when the legs' and the
  torso's animations differ (by name), the angles the "Motion" bolt faces (its -y,
  roll from its -x) are turned back, shared 45/35/20% in yaw and roll (40/40/20% in
  pitch) among "lower_lumbar", "upper_lumbar" and "thoracic"; the thoracic's share
  is then averaged with the look's in each angle the spine turns it by, also when
  the NPC looks at nothing (`CG_G2ClientNeckAngles` runs either way, with no look).
- **Classes**: `G_ClassHasBadBones`' (wampa, rocket trooper, saber droid, hazard
  trooper, assassin droid, rancor) are left unturned; other skeletons without the
  three bones are too.
- **Lookers' eyes and look targets' eyes** are where the spine's turn leaves them.
- **Left out**: the flipping and spinning-saber animations that skip the turn
  (`PM_FlippingAnim`, `PM_SpinningSaberAnim`; no NPC plays one here), the legs' yaw
  lagging behind the torso (`CG_PlayerLegsYawFromMovement`; NPCs stand still), and
  pitch from `lerpAngles` (0 for a standing NPC). An NPC on the cinematic skeleton
  reads "Motion" by the same bone number, as the rest of its rig does.

## 2026-10-16: Script random numbers pinned in the captures (chosen by the owner)

- **The question**: three of academy2's students (`studentSpawn5`, run on academy4
  and 6 too) wait a random time between animations, and the captures differed from
  run to run. Pin them in the capture (a patch), or model the original's draws?
- **Found**: ICARUS's random numbers (`CQuake3GameInterface::Random`) call the C
  library's `rand()`, not the game's `Q_irand`, and the game seeds it from the clock
  (`srand( randomSeed )` in `InitGame`, `com_frameTime`): no model of the game could
  predict them.
- **Chosen: pin them in the capture.** With `g_swjkjaScriptRand` set (a patch
  addition, on the command line with `SCRIPTS=1`, so spawnscripts that draw as the
  map loads get it too), `Random` returns its range's low end, as the new renderer
  takes it. Two captures of academy2, 4 and 6 now log the same animations; academy2
  1.61 -> 1.27, academy4 2.23 -> 1.40, academy6 2.45 -> 1.98.

## 2026-10-17: Scripts' SET_MORELIGHT lights NPCs as the original does (decided by Claude, pending review)

- **Found**: the academies' students were drawn darker than the original's because
  their spawnscripts set `SET_MORELIGHT`, which this renderer ignored. Modelled as
  the original has it: the flag's changes are kept on the NPC's timeline (on the
  client's clock, as its look target's), and an NPC drawn while it is set gets 96
  of minimum ambient light instead of 32 (`RF_MORELIGHT`), clamped as before.

## 2026-10-17: Doors NPCs stand in open as the map begins (decided by Claude, pending review)

- **What opens**: a single-player `func_door` that spawns a trigger an NPC can open
  (no "targetname" or "health"; not force-activated, toggled, locked, key-opened,
  player-used or inactive: spawnflags 2, 8, 16, 32, 64, 128), when a drawn NPC's
  box, where it stands, touches the trigger: the team's linked boxes, 120 units
  further each way along their thinnest axis. A team opens together, its first
  door in spawn order deciding. Toggled doors are left closed: touched every frame,
  the original's would open and close again.
- **When**: touched at `FIRST_FRAME` (1550 ms, the NPCs' first think; their first
  `ClientThink` may come a frame later), opening "delay" seconds plus 50 ms later,
  over its distance at "speed" (400 when 0) in whole ms, slowing to a stop
  (`TR_NONLINEAR_STOP`) or at one speed ("linear"), then standing open while the
  NPC stays. NPCs do not move here, so a door once touched stays touched.
- **Drawn** like a turning entity, its bone placed each frame; linked over its
  whole way.
- **Left out**: what the door does as it opens (its targets, `BSET_USE` scripts,
  area portals, sounds); its "model2", which stays where it spawned (no retail door
  NPCs open has one); collision, which keeps it closed; touches by the player,
  by invisible NPCs (not drawn here) and by anything else; multiplayer's doors.

## 2026-10-17: `run` and saber items modelled; kor1 kept out of the script views (decided by Claude, pending review)

- **`run` runs the named script in place**, in the same update, then the rest of
  the script that ran it (`CSequencer::ParseRun` routes it as a nested sequence;
  `CheckRun` switches to it). The scripts a map's runners reach through `run` are
  read with the map; their task groups join the running entity's as the `run`
  runs (the original routes them when the outer script loads: only the order of
  two groups of the same name could differ). yavin1's opening runs its intro.
- **Saber items draw before the NPCs spawn**: each `weapon_saber` not waiting to be
  used reads its saber's block on the 1300 ms frame (`FinishSpawningItem`; "player"
  is the player's saber, `g_saber`), after the spawn functions' draws and before
  the NPCs' blocks. Other entities' draws stay left out until a map needs them.
- **kor1's statue views are not reference views**: with the AI frozen its intro's
  camera never ends (its encounter waits on a navgoal), and while a camera runs the
  original sends only the entities visible from it. A kor1 view of the encounter's
  own room would need the runners that run on the player (spawnflags 1).


## 2026-10-17: Surface sprites as the game draws them, and a capture that sees them so (recapture chosen by the owner)

- **Drawn**: `surfaceSprites` stages of the vertical and flattened kinds (grass;
  42 of the 70 retail lines), each frame over the visible world surfaces, placed
  as `RB_DrawVerticalSurfaceSprites` places them (its random chart, seeds from each
  triangle's corners, `Q_rsqrt` steps), faded with distance from the view, swaying
  at the view's time, lit by their vertices' blue. **Left out** (decided by Claude,
  pending review): the oriented and effect kinds, wind (no weather system or wind
  cvars give any on the retail maps viewed), wind points, a fog pass over sprites
  in a fog volume, and sprites on brush entities' surfaces, models and deformed
  surfaces. (The range scale for other fields of view came later, below.)
- **The capture**: with the time pinned, the original's sprite update runs only
  when the time changes, so a capture whose grass first shows at the held time
  keeps the view vectors of that first frame: a zero lean, which the game never
  has while its clock runs. The reference patch now runs that update every frame
  while `cg_swjkjaTime` is set (a debugging capture logged the forward and origin
  it then has: the view's), and dpred_troopers was captured again; the new
  renderer draws the game's lean.
- **Quads split along the top right to bottom left diagonal** (decided by Claude,
  pending review): the lean makes a sprite's quad not flat, and the original draws
  `GL_QUADS`, whose split is the driver's. The reference GPU's matches this
  diagonal (dpred_troopers 0.33 against 1.02 for the other one); another GPU may
  split the original's the other way.

## 2026-10-17: Surface sprites' range scale, with a scripted camera's 90 degrees first (decided by Claude, pending review)

- **The range scale as the original's**: the first field of view that draws
  sprites, between 50 and 135 degrees, is the standard, and any other lengthens
  the sprites' fade ranges by `tan(standard / 2) / tan(fov / 2)`. This is what
  dpred_troopers' far grass needed (0.33 -> 0.15).
- **A camera the level scripts enabled counts as drawing first, at 90 degrees**:
  cameras are not modelled, and the reference's standard came from t2_dpred's
  intro camera (`CAMERA_DEFAULT_FOV`) before its view was pinned. The alternative,
  a standard written into each view's line, would hold only for the captures.
  A camera that zooms before sprites show, or never sees any, would set another
  standard; no reference view has one.

## 2026-10-18: STARTINSOLID spawners keep their height; the client's clock starts after the intro frame (decided by Claude, pending review)

- **An NPC whose spawner has STARTINSOLID (128) is not lifted out of the floor**
  (`NPC_Begin` runs `G_CheckInSolid` with fix only without it). It still falls onto
  the floor below. The capture logs show the difference: 672.0 against 672.125 for
  the academies' students.
- **Animations set before the client draws start at the level's time**, through the
  frame after the player begins (2050). Before, only the first frame (1550) did. In
  the captures the client draws its first frame after that server frame, and the logs
  give 2050 for every animation set then. A game whose client draws sooner would
  start them a frame earlier, as it does later ones. This renderer follows the
  captures.

## 2026-10-18: JPEGs decode as OpenJK's libjpeg 9a, ported to Rust (chosen by the owner)

The 2026-10-07 entry's condition came true: ctf1_fog_above's remaining 0.40 was the
JPEG decoder (9a scales 4:2:0 chroma up in its inverse DCT, zune-jpeg filters it),
and the reference captures turn texture compression off. With 9a's pixels all 47
views came closer (median 0.14, from 0.21). Of the two ways to match 9a exactly,
the owner chose porting its decoder to Rust (`crates/image/src/jpeg.rs`) over
building its C source into the engine, which would have needed a C compiler for
Linux builds. The port covers what the retail files and common JPEGs use; the rest
(arithmetic coding, 12-bit, 4:2:2-style sampling) fails to decode with an error.

## 2026-10-18: The target blends in 10 bits, everywhere (chosen by the owner)

The reference captures come from a 10:10:10:2 framebuffer (the Intel OpenGL
driver's, though OpenJK asks for 8 bits), and rendering into one brings the 47
views from a median of 0.14 to 0.04, most pixels exact. It costs about 0.3 ms of
GPU a frame on the HD 630 (ffa3 path, offscreen). Asked whether to use it
everywhere or only for captures and tests, the owner chose everywhere: one format,
so what the views verify is what the viewer and benchmarks run. On a PC whose
OpenGL gives the original 8 bits, its pictures differ from these by a level here
and there.

## 2026-10-19: Captures pin the snapshot's eye and movers' places to the view (decided by Claude, pending review)

t3_stamp's intro camera never ends in a capture with the AI frozen, so the server
sent only what the camera saw, and SP's cgame placed a moving mover at its real
clock, not the pinned time. The capture patch now builds the snapshot from the
pinned view (`cg_swjkjaView`) and places a mover at `cg_swjkjaTime`, as it already
turned one: what a capture shows is then the view at its time, as this renderer
draws it. The other way, modelling the camera's snapshot and the client's clock,
would tie every scripted view to its cutscene. Captures taken before this were not
taken again; one that showed a camera's entities may differ if taken again.

## 2026-10-19: A sliding entity is linked where it is (decided by Claude, pending review)

A door opening was linked over its whole way, so the server sent it to a view that
saw any of that way. The server relinks a mover where it is each frame
(`SV_LinkEntity`), and scripted movers travel thousands of units (t3_stamp's
cranes, 6,144), so a sliding brush entity and its "model2" are now linked where
they are at the frame's time, once they have moved. yavin1's open door renders as
before.


## 2026-10-19: Captures pin every mover's angles to the view's time (decided by Claude, pending review)

The capture patch evaluated a mover's angles at the pinned time only when they turn
at one speed (`TR_LINEAR`: the func_rotatings). A script's turn eases out
(`TR_NONLINEAR_STOP`), and cgame evaluated it at its real clock, which in the first
hevil_ship capture was past the turn's end: the ship stood fully turned at 6000 ms,
in the right place. Every mover's angles now take the pinned time, as their positions
already did. The func_rotatings' captures are unchanged (the same time for them);
no earlier view shows another turning mover.

## 2026-10-19: A moved model keeps its scale and its "modelAngles" are left out of its turns (decided by Claude, pending review)

Once `move` makes a breakable a mover (`ET_MOVER`), the original draws it with
`CG_Mover`, which drops its "modelscale"; and a brush entity's "model2" turns by its
entity's angles plus "modelAngles". Here a moved model keeps its scale, and a turned
"model2" turns by its entity's angles alone. No reference view shows either
(t3_hevil's ship is unscaled); retail maps were not surveyed for them.

## 2026-10-19: What triggers and buttons start is pinned as the names they use (decided by Claude, pending review)

The elevators of t1_fatal and hoth2 rise when the player walks into a
`trigger_multiple` (or presses use in one), which uses a script runner. The player
does not move here and, in the captures, stands in noclip, which touches no trigger.
What a trigger or button does is use a name (`G_UseTargets`), so that is what is
pinned: a level takes uses, each a name and a time, made in the first level frame at
or after the time, before that frame's entities run (`Level::uses`). The capture
patch makes the same uses (`g_swjkjaUse`, as the `use` cheat command makes them),
and a views file gives them in a map's first view's eleventh column. The other way,
modelling the player's box touching triggers along a path, waits for the player's
movement (milestone 4); its touches would come to these same uses.

## 2026-10-19: A used door closes after its wait; a use while it moves is left out (decided by Claude, pending review)

Doors used by name (by a script's `use` or a pinned use) open, stand open "wait"
seconds (2 when 0) and close again, as `Use_BinaryMover`, `Reached_BinaryMover` and
`ReturnToPos1` have them, a team on its first door's times. A use while the door
stands open puts its closing off; a toggled door waits for the next use. Left out:
a use while the door moves (the original turns it back part of the way), a locked
door's unlocking, and the targets a door uses as it moves (11 of the 101 retail
doors a use opens have them). No reference view needs them yet.

## 2026-10-19: A sky portal's scene is drawn as the original draws it; its entities wait (decided by Claude, pending review)

hevil_ship's sky was t3_hevil's `misc_skyportal`: the original draws the scene from
the portal behind the view, every frame. This renderer now does the same, with a
second copy of the per-frame data, at the original's cost (2.7 ms of GPU on
hevil_ship, whose portal room fills the screen with blended cloud patches). Not
drawing the portal's scene would have kept the view's own sky box, which is not
what any retail map with a portal shows. Left for later: the entities the original
flags as in the portal (only yavin1's room holds one, a model no reference view
shows), which need the game's trace from the portal.

## 2026-10-19: Frames keep the last frame's colour without a global fog (decided by Claude, pending review)

The original clears the colour only on a map with a global fog (to its colour) or
with `r_fastsky`; otherwise a frame draws over the last one, which shows where a
blend lies over nothing drawn (t1_fatal's power pipe, inside a brush, multiplies
its goo by it). This renderer now does the same: `WorldRenderer::clear_color` is
`None` there, the target's colour persists from frame to frame, and a window image
keeps its last frame. `render_world` draws each view 10 times, as the capture holds
a view still, so its pictures are the settled ones. The other way, clearing to black
and drawing such pipes some other way, would match no view held still.

## 2026-10-21: Rack goods spread with the replayed random numbers (decided by Claude, pending review)

- **Found**: a capture with `g_swjkjaRand` logged t2_dpred's draws to 2000 ms. Up
  to the ammo racks only two callers draw: the stormtroopers' coin flip and the
  gun racks (`GunRackAddItem`'s spread and turn); then the ammo racks at 1100 ms.
  The capture's build evaluates a call's arguments right to left (`Q_flrand(0, 1)`
  before `Q_flrand(-1, 1)` in the ammo rack's calls), so a spread draws aside before
  forward.
- **Chosen: replay them**: `entity_models` walks the spawn with its own generator,
  drawing the gun racks' goods in entity order and the other entities' draws
  (`npc::spawn_draws`, the NPC spawners and player starts), then the ammo racks'
  goods. `npc_models`' walk draws the racks' numbers too (discarding the goods), so
  the NPCs' blocks read on from the same state. The angles are normalized as the
  original's (`AngleNormalize180`, 65536ths of a turn).
- **A retail test** pins dpred_guns' middle rack and t2_dpred's first ammo rack to
  the logged numbers; the other maps' racks are checked only by their views
  (stamp_barrels, vjun2_officer).

## 2026-10-24: Rain is compared with world effects off on both sides (chosen by the owner)

- **Found**: the original places rain particles with the renderer's own random
  numbers (`CParticleCloud`, `Q_flrand` in the renderer module) and moves them by
  each frame's real time (`backEnd.refdef.frametime`), from the map's first frame
  on; its wind changes with the C library's `rand()`. The capture loads the map,
  waits and moves the camera between views, so where a reference shot's streaks
  fall cannot be reproduced. Rain drawn at other places than the reference's
  would add its own streaks to the ones missing: hevil_fan_west's 0.63 would grow.
- **Chosen: world effects off in comparisons.** The reference patch gets a switch
  that leaves world effects undrawn, and `render_world` leaves them out too, so the
  kept views compare without rain (pixel for pixel, as before). Rain itself is
  checked by its numbers (particles, size, colour, speed, where it counts as
  outside) against the original's code, and by eye. The rain views need one
  recapture with the switch (asking first).
- **The other way**, rain drawn in the kept views and judged by statistics (the
  frame's added brightness, its streak pixels), would keep no view comparable pixel
  for pixel.
- **First slice (world)**: `weather::commands` gives the `CS_WORLD_FX` strings the
  single-player spawners add (`fx_rain`, `fx_snow`, `fx_wind`, `fx_wind_zone`,
  `misc_weather_zone`), `Command::parse` reads them as the renderer, and `Outside`
  is the renderer's outside cache (`COutside`), scanned from the world brushes'
  contents (`PointContents`, through the BSP tree as `CM_PointContents`). Left
  out: `fx_puff`, multiplayer's spawners, and heavy rain's lightning
  (`fx_rain_think`, which also draws from the game's random numbers and so could
  shift later picks on such a map).
- **Second slice (render)**: `weather::Weather` runs the rain clouds and winds
  (`CParticleCloud`, `CWindZone`) once a frame from the time since the last one
  (1 to 1000 ms; the first frame only caches, as the original's), and
  `WorldRenderer` draws every cloud's triangles after the scene in one draw: added
  to the frame, depth tested but not written, unfogged, not in the sky portal's
  scene. The renderer module's numbers come from a `Rand` of its own and the
  winds' lulls from MSVC's `rand()` unseeded: numbers of the right kind, not the
  capture's. `WorldRenderer::draw_weather` (on by default) is off in
  `render_world`. Left out: snow, space dust, fog clouds, puffs, in-game
  cinematics (no world effects during one), and the rain image's nearest filter.

## 2026-10-26: t3_hevil's streaks are an effect, not rain; the scan matches the shipped caches

- **Found**: t3_hevil has no world effects. The single-player maps with any are
  hoth2 (snow, wind, fog), t1_rail (heavy rain), t2_wedge (wind, fog), vjun1 (acid
  rain) and yavin2 (light rain). hevil_fan_west's and hevil_fan_low's streaks sit
  over the `fx_runner`s of `bespin/dust` at the fans (-1664 512 520 and
  2688 512 -120): an effect (effects are not drawn yet), which
  `r_swjkjaNoWeather` does not take out (BASELINE.md's "rain over the fan" was
  wrong). Of the kept views, only vjun1_hall shows world-effect rain, so it is
  the only one to recapture without it.
- **Found**: every weather map ships `maps/<map>.weather`, its header (version 1,
  the map's MD4 checksum) matching, so the original reads that cache and never
  scans. The scan gives the same bytes once its dot products are unrounded (the
  files come from an x87 build); in floats, 70 of t1_rail's cells differ. The
  retail test compares all five byte for byte, so the files need not be read.
- **vjun1_hall** sees rain only within ~150 units of the eye: beyond, the hall is
  inside (the shipped cache agrees), and the floor hides what falls below it. How
  many streaks fall that near is luck, so the rain test turns round from the same
  eye, out over the fogged air.

## 2026-10-26: Voice lines play out a frame later than their length (decided by Claude, pending review)

- **Found**: a capture of academy4's intro with its script trace (`g_ICARUSDebug 4`)
  has five of its first six voice lines end a frame later than "the first frame
  from its start plus its length" (2026-10-16's rule) gave: Luke's first line,
  started at 3100 and 9979 ms long, ends at 13150, not 13100, and the intro's
  later lines start 50 to 300 ms later than here (Kyle's first at 44,800, here
  44,500). The client plays a line as it draws the frame that started it, a frame
  behind the server, and the game sees it stopped (`G_CheckTasksCompleted`, the
  mixer's `s_entityWavVol` back to 0) on its first frame after that.
- **Chosen: a line plays out at the first frame from its start, plus 50 ms, plus
  its length** (`VOICE_LAG`). The mixer runs in real time, so a run may still end
  a line a frame later still (Luke's fourth, 12.4 s long, ended two frames
  later in both traces; a second trace started his third at 23,350, the first at
  23,300), and no fixed lag fits all six; views late in an intro are better taken
  where nothing has just changed.

## 2026-10-26: NPC AI next, turning first (chosen by the owner)

- **Next task: NPC AI**, NPCs acting beyond their scripts' animations: behavior
  states, turning and moving toward goals.
- **First slice: turning** (`SET_DYAW`, `SET_VIEWTARGET`), over walking to navgoals
  (`SET_NAVGOAL`, 226 retail scripts, which needs the NPC's `Pmove` and the
  navigation). 41 retail scripts set a yaw, 22 a view target.
- **Found: a frozen AI still turns.** `NPC_Think` with `d_npcfreeze` set runs
  `NPC_UpdateAngles` and the movement frame before it returns, so the captures
  (`d_npcfreeze 2`) turn NPCs as an unfrozen game does and complete their turns:
  turning needs no new capture mode.

## 2026-10-26: How turns are drawn and aimed (decided by Claude, pending review)

- **The legs are drawn at the view's yaw**, from snapshot to snapshot
  (`LerpAngle`). cgame's legs trail the view (by up to 60 degrees, catching up 10
  degrees a rendered frame at most, the spine turned to make up the difference),
  at a speed that depends on its frame rate; once the capture has stopped the
  game they have caught up, as the settled look (2026-10-15) has.
- **Pitch is turned but not drawn**: a view target's pitch (`SET_DPITCH`, within
  44 degrees, the default head range; `.npc` blocks' ranges are not read) delays
  the task as in the original, while the head and spine take no part of it.
- **A view target is the first NPC of that name**, its eyes and the NPC's own
  their origin raised by their view height (`maxs[2] - 4`), as the target stands:
  the original measures to the target's `renderInfo.eyePoint`, which cgame sets
  from its head as drawn. Other entities are not found here (the turn then
  completes without turning, as one the original does not find).
- **Yaw speed is the `.npc` block's "yawSpeed" (90 by default)**, three quarters of
  it for stormtroopers, swamp troopers, imperial workers and "rodian2", as at the
  easiest skill the captures play (`g_spskill 0`).
- **Captures of late views wait longer**: `SCRIPTS=1` waited 60 frames a second of
  the view's time, and a 47.5 s view on academy4 was taken 13 s early (the frames
  do not keep to `com_maxfps`); it now waits 150 a second (the game stops itself at
  the view's time).

## 2026-10-27: Walks captured with the walker's AI alone running; straight paths first (chosen by the owner)

- **Captures of walks run the AI of an NPC walking to a navgoal only**
  (`d_npcfreeze 3`, a patch addition; `WALK=1 bench/capture-reference.sh`), over
  unfreezing every NPC (`d_npcfreeze 0`), whose idle AI would look around and draw
  random numbers. An NPC thinks unfrozen while its `TID_MOVE_NAV` task is pending;
  its commands stay 1000 ms behind the level's, as a frozen NPC's do, so each moves
  it 50 ms; each of its frames is logged ("swjkja walk": origin, velocity, speed,
  view and desired yaw, move direction, whether its behavior thought).
- **First slice: straight paths** (`STEER::GoTo` to a goal in reach, the NPC's
  `Pmove`), over the nav grid with it (`NAV::GoTo`, the map's `.nav` graph), which
  comes later; the walk and run animations are the next slice (academy3's Luke
  walks in a held `BOTH_CIN_1`).

## 2026-10-27: How NPCs walk to their navgoals (decided by Claude, pending review)

- **Found: a frozen NPC's view turns every frame, an unfrozen one's only as its
  behavior thinks** (every 100 ms, `nextBStateThink`): between thinks `NPC_Think`
  replays the last command, whose angles overwrite those `NPC_UpdateAngles` just
  set. academy3's traced Luke turns 24 degrees every other frame.
- **Found: steering sets no velocity.** `STEER::DeActivate` turns the velocity it
  wants into a move direction, a speed (`ps.speed`, a whole number, three
  quarters while the NPC backs up: Luke starts at 41 of his 55) and the yaw to
  turn to; `Pmove` then brakes (friction 6, at least 100 units a second's worth)
  and accelerates (12) along that direction, so a walk at 55 gains 3 a frame from
  33. Within 48 units (four times the navgoal's radius of 12) it slows.
- **A goal within 400 units is steered to once a trace of the path clears**, from
  the first think: the original's first trace returns "not yet" and that think
  takes the nav grid, and the nav graph's own check (`NAV::OnNeighboringPoints`)
  is not modelled; academy3's traced walk matches frame for frame either way. A
  path that is not safe (beyond 400 units, or blocked) would take the nav grid:
  the NPC is not steered and its task does not complete, as before.
- **Its goal is reached in its box**: `STEER::Reached` takes the goal within its
  radius or inside the NPC's box a unit larger (`absmin`, `absmax`); Luke stops
  16.7 units from lukeNav1, which is inside his box. The task completes on that
  think, and friction stops him the same frame (12 units a second, braked by 30).
- **Level ground, no one in the way**: the NPC keeps its height, and its box stops
  at walls (sliding along them); steps, slopes and other NPCs (`AvoidCollisions`:
  the traces here hit no NPCs) are left out.
- **It is drawn from snapshot to snapshot**, as its turns are; what looks at it and
  its shadow follow it.

## 2026-10-27: How walking NPCs move their legs (decided by Claude, pending review)

- **The command picks the animation**: each think's steering also gives the NPC a
  command (`STEER::DeActivate`: `forwardmove` and `rightmove`, the move direction
  against its current yaw times 127, floored; `BUTTON_WALKING` under its walking
  speed plus 5), replayed until the next think and cleared by a think that does not
  steer. A command that moves it plays `BOTH_WALK1` or `BOTH_RUN1` on legs not held
  (`PM_Footsteps`), `BOTH_WALKBACK1` or `BOTH_RUNBACK1` while its last forward or
  backward move went back (`PMF_BACKWARDS_RUN`); the torso plays what the legs do
  (`PM_TorsoAnimation`), but for an armed NPC backing up, which keeps its stance's
  torso (its weapon's own choice is not modelled). The ATST, rancor, wampa, jawa
  and Galak mech's own walks, and lit sabers' (`BOTH_WALK2`...), are left out.
- **Found: late in an intro the original's clock drifts from ours.** academy6's
  hangar walks (kyle2, jaden2) start 350 and 700 ms earlier in the original than
  here: a voice line plays out when the client's mixer has painted its last sample
  (`G_CheckTasksCompleted`), and the mixer runs in real time, ahead of what is
  heard, so a capture's line can end well before its length (2026-10-26's lag
  fits academy4's first lines, not these). A walking view is taken early instead.
- **Found: the first think takes the nav grid, and it shows.** t1_sour's intro
  walks Kyle and the player's stand-in to introNav1 from 2050 ms; the original's
  first think steers them along the nav graph's path (headings 288 and 317, not
  the goal's 238 and 252), then straight from the next. They walk on 9 and 23
  units off the straight path, and the frame after ours. Until the nav grid is
  modelled, t1_sour_intro_walking scores worse walking (2.02) than sliding in the
  stance (1.82): the strides match the original's, the positions do not.

## 2026-10-28: How NPCs find their way by the nav graph (decided by Claude, pending review)

- **Found: the trace that clears a path does not clear it for that think.**
  `STEER::SafeToGoTo` returns false even when its trace (every 1.5 s) is clear: it
  only marks the path safe for the 2 s after. So the first think, and each one
  whose 2 s have run out when the next trace is due, takes the nav graph
  (`NAV::GoTo`); the thinks between go straight. Before the trace, a goal within
  500 units is safe when the NPC's nearest point and the goal's are neighbors and
  each stands within its point's safe radius (`NAV::OnNeighboringPoints`,
  `NAV::InSafeRadius`).
- **`NAV::GoTo`**: the goal's nearest point, the NPC's nearest toward it, the
  shortest path (`NAV::FindPath`), the path's next point sought within its slowing
  radius (`STEER::Path`), then collisions avoided. On an edge it takes the edge's
  first point, where the original draws one of the two. The path is found anew on
  each think that takes the graph; `NAV::UpdatePath` and the path cache are left
  out, as no traced walk uses them (the thinks between go straight and clear it).
  Without a path the NPC stops (`STEER::Stop`) and turns to its last path's yaw,
  0 if it never had one (OpenJK's own rancor code notes this).
- **Collisions are avoided as the original does** (`STEER::AvoidCollisions`, after
  straight steering too): the velocity the steering so far would give, traced a
  second ahead through the world and every other NPC's box where it stands now
  (those earlier in the frame have moved), then 40 degrees aside at 0.3 s (ahead
  again while the NPC has no side yet, which is how a first think seeks three
  points). An NPC slower its way is followed at its speed, one standing on the next
  point is waited behind, else the NPC steers past what it hit by the side the
  plane leans its way. That side holds for 2000 ms: the original draws 500 to 2000
  (`Q_irand`), a draw not replayed here, and t1_sour's stand-in needs at least
  1300. Doors are not told apart from the world (the original's avoidance passes
  through `func_door`), and being blocked for 2 s (`STEER::Blocked`) is left out.
- **What NPCs meet**: every other NPC's box but those spawned not solid
  (`SFB_NOTSOLID`, 64: contents 0), as academy3's Kyle, who stands within Luke's
  box and is walked through.
- **The graph's edges are tested against the world's brushes alone**: doors,
  walls and breakables on edges, area portals and danger are left out
  (`crates/world/src/nav.rs`).
- **A level without a graph** (the tests' made-up levels) skips the graph's test,
  and the think whose trace clears goes straight, as before. The renderer always
  builds one for single player.
- **t1_sour's intro walkers match every traced frame** (2100 to 3550, to four
  decimals), and so does academy3's Luke on the real map (78 frames), each a frame
  early. In t1_sour: the original updates the intro's runner after the two
  walkers (by entity number), so they get its navgoal a frame after ours, where
  runners update first (2026-10-16). Entity numbers are not modelled yet.

## 2026-10-28: Runners and NPCs update by entity number (decided by Claude, pending review)

- **Replaces "script runners update before NPCs" (2026-10-16).** Each frame updates
  the runners and NPCs by entity number, as `G_RunFrame` does; movers still update
  after them. Both a runner and an NPC run their scripts in their own turn, so an
  NPC numbered below the runner that sets its navgoal first thinks a frame later.
- **Entity numbers are worked out from the map** (`crates/world/src/entity_number.rs`):
  `G_Spawn`'s lowest free number from 1, and which entities free themselves and
  when (as they spawn: skill, static models, waypoints, combat points, `ref_tag`s
  without a target, `info_null`s with flag 1; at 100 ms: the other `ref_tag`s,
  while touch doors spawn their triggers; at 300 ms: the other `info_null`s; at
  350 ms: each NPC spawner spawns its NPC and its goal, then frees itself). This
  gives every NPC the number the original's traces log on the five academies,
  kor1, kor2 and t1_sour (`npcs_and_runners_are_numbered_as_in_the_original`).
  t2_dpred's and t1_danger's later numbers come out lower: racks' goods, plats'
  triggers, emplaced guns and the like are left out.
- **A runner used during a frame starts in its own turn**, that frame if its turn
  is still to come, else the next (`scriptrunner_run` starts the script and the
  runner's update runs it). What the player begins with still starts the frame
  after 2000.
- **t1_sour's intro walkers now match every traced frame at its own time** (2100 to
  3550): its runner is entity 336, after the walkers (144, 146). academy3's Luke
  stays a frame early: the runner there is entity 2 and Luke 3, so the order is
  the same as before, and his frame comes from the voice line before (2026-10-26).
- **Found: the walk captures drew walking NPCs one snapshot late.** With
  `SCRIPTS=1` the game stopped at the view's time plus 50 (`g_swjkjaStop`, so the
  animations the next frame sets, stamped a frame earlier on the client's clock,
  show), and the patch pins only movers' places to `cg_swjkjaTime`. NPCs are
  placed from cgame's last snapshot, so they were drawn at the view + 50. On main
  the walk a frame early hid this: against those captures t1_sour_intro_walking
  went from 1.10 to 1.67.
- **Walk captures now stop at the view's time** (`WALK=1`; recapture chosen by the
  owner). The two walk views were taken again; their traces match the old ones
  frame for frame (the old, longer trace files were kept). t1_sour_intro_walking
  scores 1.13: the walkers stand where the reference has them, and the stride's
  phase differs. academy3_luke_walking scores 1.11, where it was 0.24: Luke is
  still simulated a frame early (the voice line), and the new capture shows it.
  Drawn 50 ms earlier it scores 0.08.

## 2026-10-29: Walks play as fast as their NPC moves (decided by Claude, pending review)

- **A walk or run plays at the speed its NPC moves** (`g_noFootSlide`, on by
  default): `PM_SetAnimFinal` scales the animation's own speed by the last
  movement frame's speed (`resultspeed`) over the animation's stride (50 a second
  for walks, 150 for runs, 75 for a crouched walk, the hazard trooper's 50 and the
  rancor's 173), from 0.01 to 1.5. Droids (R2, R5, protocol, gonk, mouse, probe,
  seeker), the howler and the wampa play at their own speed. The dual and staff
  styles' strides (100 for walks, 250 for staff runs) are left out: the saber
  style an NPC ends up with is not modelled.
- **A part's clock is Ghoul2's** (`script::Clock`): when its bone began
  (`startTime`) and its speed. A part already playing the animation is set again
  only when the speed changes, and it carries on from the frame it has reached
  (`setFrame`), which moves its start back, truncated to the millisecond. A
  re-set does not blend, and ends a blend in progress. A part set without
  restarting while the other part plays the same animation at another frame takes
  up the other's frame (`g_synchSplitAnims`). This is one function
  (`Body::play`) for footsteps and scripts alike, so a script's walk on an NPC
  standing still plays at 0.01.
- **`Holds` gives the animation itself** (`swjkja_model::animation::Animation`,
  a new dependency of the world crate) instead of its hold and length: the
  frame reached needs its first frame, frame count and rate.
- **Found: the client's clock stays at 2050 through the frame at 2150.** With 2100
  for the walkers' second frame, both walks start again at 2063 when their speed
  reaches the cap. The trace has Kyle at 2050 with speed 1.5. Replaying the trace's
  own positions through Ghoul2's timing gives its 2754 for the player's stand-in
  only when the second frame is stamped at the first's time: the client is still
  drawing its first frame (2026-10-18). So animations set at level 2100 and 2150
  both start at 2050.
- t1_sour's intro walkers' legs and torsos end where the trace has them, start
  and speed (`t1_sours_intro_walkers_stride_as_in_the_original`).
- **Found: what is left of t1_sour_intro_walking is the stand-in's legs' yaw**
  (0.54, from 1.13). She walks at the frame the trace gives, but she steers left of
  Kyle from 3400 and moves at an angle to where she faces: cgame turns the legs
  toward the move (`CG_G2PlayerAngles`), which is not modelled.

## 2026-10-29: Legs turn toward the move (decided by Claude, pending review)

- **The legs face where the NPC moves, within 60 degrees of where it faces**
  (`CG_PlayerLegsYawFromMovement`, for every class but the gonk, interrogator and
  sentry): toward its velocity's yaw, the other way when it backs up
  (`PMF_BACKWARDS_RUN`), not at all when it stands still or moves more than 150
  degrees from its view. cgame moves them part of the way each rendered frame (by
  the frame's interpolation, 5 degrees at most), at a speed that depends on its
  frame rate; they are drawn where they get to once the capture has stopped the
  game, as the trailing legs (2026-10-26) and the settled look (2026-10-15) are.
  Between frames they turn from one frame's to the next over its 50 ms
  (`Timeline::legs_turns`), as the view does.
- **The velocity is the next frame's**: as the client draws the frame at T, the
  server has run the frame at T + 50 (the trace logs the stand-in at 3550's origin
  for a view at 3500), and cgame reads the NPC's `ps.velocity` from it. The
  timeline records the velocity and the backing-up flag after each movement frame
  that changes them.
- **The spine turns the torso back to the view** (`CG_G2ClientSpineAngles`): by
  the yaw between view and legs, shared out over the lumbar and thoracic bones as
  the motion bone's turn is, whether or not the legs and torso play the same
  animation; the motion bone's turn is subtracted from it when they differ.
- t1_sour's stand-in's legs at 3500 face the trace's velocity at 3550 (27.1
  degrees left of her view; `t1_sours_intro_walkers_stride_as_in_the_original`).
  t1_sour_intro_walking goes from 0.54 to 0.31; her feet are still spread a
  little wider than the reference's. Whether the original's legs had fully
  settled is not logged (the capture would need `renderInfo.legsYaw` traced, a
  GUI run).

## 2026-10-30: Voice lines last as the original counts their MP3s (decided by Claude, pending review)

- **Found: the original counts one frame more in 2,142 of the 3,531 voice MP3s.**
  `C_MP3_GetUnpackedSize` decodes every frame, a leading Xing or Info frame too
  (1152 samples of silence), which the scripts slice (#58) left out: those lines play
  26 ms longer. It also stops when the bytes left before a rear ID3v1 tag are
  fewer than the first frame's unpadded length, or a frame would run past them.
  `sound::length_ms` now counts as it does, frame for frame on all 12,901 MP3s
  in the game (checked against a port of the C code).
- **Replaces the 50 ms of 2026-10-26 with 33** (`VOICE_LAG`). With the lengths
  right, a line ends on the frame the traces have it when its lag is between 32
  and 35 ms: academy4's first, second, third (as the first trace has it) and fifth
  lines, and academy3's first, Luke's before his walk (13,750 in the original, at
  13,700 here before). 33 ms is a server frame less a client frame at the
  captures' 60 fps, but it is fitted, not derived: the mixer runs in real time, and
  academy4's fourth line (12.5 s) still needs more than 39 ms, its third in the
  second trace more than 42. No fixed lag fits every traced line; this one fits
  the same academy4 lines as before and academy3's too.
- academy3's Luke now walks at the trace's own frames (78 of them, 14,850 to
  18,700; `academy3s_luke_walks_as_in_the_original`), no longer a frame early.

## 2026-10-30: How NPCs keep their paths and take stairs (decided by Claude, pending review)

- **Traced scene: yavin2's intro** (Kyle and Rosh to kyleNav1 and roshNav1, 415
  units off, down a flight of stairs into the garden; `WALK=1`, 200 frames), over
  academy4's Luke, whose walk from the stage turned out to start with a teleport
  (`SET_ORIGIN putlukehere`, not modelled) and never takes the stairs, and over
  academy2's four 800-unit walks (no stairs).
- **A path is kept from one think that takes the graph to the next**
  (`NAV::UpdatePath`): the points reached (within their reached radius, heights
  closer than 60 left out) are dropped; a next point overdue (its ETA, from the
  path's length at a quarter of the walking speed) finds the path again; steering
  straight drops it (`NAV::ClearPath`). A search to the same end is not run again
  for 3000 ms (`mLastAStarTime`: the original draws 3000 to 6000, not replayed; the
  low end). Found: Kyle's first think seeks the point where his path passes beside
  him, 2.4 units off; the next heads for the stairs. Found anew each think, as
  before, he sought that point forever.
- **`Pmove` as an NPC takes it**: `PM_GroundTrace` (a quarter unit down; steeper
  than 0.7 is not walked on; landing with no command stops the fall), then
  `PM_WalkMove` (along the ground's plane at the same speed: down a ramp the move
  direction's height counts) or `PM_AirMove` (the command, from the view's yaw,
  steers at 4; gravity 800), each through `PM_StepSlideMove` (`PM_SlideMove`'s four
  bumps and clip planes, a step up of 18, `g_stepSlideFix`). It never steps down:
  walking off a step, an NPC falls, as the trace shows (40 units a second faster
  each frame). Left out: other NPCs in the way of the move (`pm->trace` hits their
  boxes; avoidance keeps them apart here), `PM_CrashLand` and the falling and
  landing animations, `pm_time`'s landing lock (falls faster than 200), jumps,
  water, ladders.
- **Found: the original's trace skips a brush outside the sweep's bounds with no
  epsilon** (`CM_TraceThroughBrush`), so a box ending within 0.125 of a floor, short
  of it, misses it: Kyle, 0.19 above the garden's floor, falls one more frame
  instead of walking. Our trace had a unit's margin; it now has none, and sweeps the
  box about its centre as `CM_Trace` does (a sloped plane's distances round as the
  original's: a 0.0003 difference on the stairs' ramp had tipped a think's speed
  from 90 to 89).
  Two NPCs that settle onto slopes (kor2's st1, a vjun2 imp) rest 0.0002 lower.
- **Found: waypoints standing on the floor are lifted clear of it**
  (`G_CheckInSolid`'s fix, from `SP_waypoint` and `SP_waypoint_small`): yavin2's at
  88 rest at 88.125, and a path's point beside an NPC is then reached by a clear
  trace. A point still in solid at a crouch's height is not spawned.
- **Angles as the original rounds them**: the yaw steering turns to is kept in
  65536ths of a turn (`AngleNormalize360`, `STEER::DeActivate`), and so is an NPC's
  view from its first movement frame (academy3's Luke begins at 49.9988, as the
  trace logs). His traced view yaws now match to the trace's four decimals.
  Normalizing multiplies by the inverse length (`VectorNormalize`).
- **Every traced frame of both walkers matches** (Kyle 100, Rosh 96, to four
  decimals), a frame later than the original: Kyle's navgoal is set a frame later
  here, when the voice line before it ends (that line's end is not traced).

## 2026-10-30: Scripts put NPCs at tags (decided by Claude, pending review)

- **`SET_ORIGIN` puts an NPC there at once** (`Q3_Set`'s `G_SetOrigin`, not
  `Q3_SetOrigin`'s teleport: no extra unit up, no knockback time, its velocity kept),
  its nearest nav point forgotten; it is then on nothing until its next movement
  frame finds the ground (`PM_GroundTrace`), so a tag above the floor drops it there.
  academy4's "putlukehere" is a unit up: Luke stands at 673 the frame the script
  runs, at 672.125 the next. Such a frame is no longer skipped as one where nothing
  changes. This replaces part of 2026-10-16's "Moves are left out".
- **`SET_ANGLES` sets the view at once** (`SetClientViewAngle`, kept in 65536ths of
  a turn as the next command keeps it) and turns to its yaw (`Q3_SetDYaw`); its task
  completes at once.
- **A tag in a `set` gives its origin or angles** (ICARUS' `GetTag`, the kind as a
  float member: 54 `TYPE_ORIGIN`, 53 `TYPE_ANGLES`); a tag's angles are its own, or
  for a `ref_tag` with a "target" those toward that entity (`ref_link`). A vector
  value is read too. Tags with an owner are still not kept.
- **academy4's Luke now walks every traced frame** (44, to four decimals) from where
  the intro puts him, on the clock of the scene's second capture (`a4-walked`), with
  no offset; the first capture (`a4-walk`) logs the same positions 100 ms later: the
  original's voice lines play in real time, so its clock late in an intro drifts
  from capture to capture (2026-10-27). Our goal is set where the second capture
  sets it.

## 2026-10-30: Invisible NPCs run their scripts, and the player is the female Jaden (decided by Claude, pending review)

- **Traced scene: academy2's intro** (`WALK=1`, three captures: to 66,000 ms, to
  80,000 ms, and once more to 80,000 with the original's signals logged), whose six
  students walk to their navgoals, jedi3 to jedi6 800 units to the north stairs.
  Before this, each walked the trace's path a frame or more late, and jedi4 and jedi6
  were never moved to their tags: they stalled short of the stairs.
- **An NPC whose spawnscript makes it invisible is kept, hidden** (`SET_INVISIBLE`,
  `EF_NODRAW`: neither it nor its shadow drawn), and its script runs. It had been
  left out, so nothing received the signals its script waits on: academy2's
  `npc_luke2` waits on "newluke" and "luke2 line 6", then moves jedi4 and jedi6 to
  movejedi4here and movejedi6here (77,450 in the original) and later walks Luke to
  lukeNav1. It is shown when a script says so (`SET_INVISIBLE false`); hiding it
  clears its contents, so other NPCs' moves no longer meet it, and so does
  `SET_SOLID false` (`Q3_SetSolid`: `CONTENTS_CORPSE` or none; `true` gives
  `CONTENTS_BODY` back; the solidifier that waits for a clear spot is left out).
- **Found with a new log of the original's signals** ("swjkja signal", sent and
  received with their level time, `d_npcfreeze 3`; the patch logs them in
  `CIcarus::Signal` and `ClearSignal`): ours and the original's intros send the same
  signals in the same order, but two of ours came 450 ms late, both after Jaden's
  lines. **The player's lines are the female Jaden's**: a script's sound is
  lowercased and, the `sex` cvar being "f" (its default, and the captures'
  config's), "jaden_male/" read from "jaden_fmle/", else a file starting "mr_" from
  "ms_" (`PlayIcarusSound`, `G_AddSexToPlayerString`). Those lines are shorter.
  A choice of the male Jaden would be a setting; none is modelled.
- **All six walk every traced frame that moves one** (529, to four decimals) with no
  offset on the 66,000 ms capture's clock (`a2-walk`). The two 80,000 ms captures
  have the same frames 100 and 150 ms later: the original's voice lines play in real
  time (2026-10-27), and its drift grows to 200 and 250 ms by the teleport.
  Checked in a scratch run with Kyle's third line 100 ms longer, so the teleport
  lands on the same walk frame as in the second capture: jedi4 then walks every
  traced frame after it too. jedi6 does not: its tag drops it 25 units from jedi4,
  their boxes overlapping, and the original's next `Pmove` starts inside jedi4's box
  (other NPCs in `Pmove`'s traces are still left out, 2026-10-30).
- Views: academy2_students_walking (66,000 ms) and academy2_students_walked
  (80,000 ms) look north up the classroom; `npc_luke2` now stands where the
  reference has him.
