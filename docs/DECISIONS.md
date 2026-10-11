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
- **Weapons sit in the middle of their random range, like the other rack items**: a
  gun rack's three 9 units apart, an ammo rack's 7 units to the right (the medpack
  then on the left), turned around and pitched 75 degrees up (cgame's `CG_Item` for a
  VERTICAL item). An ammo rack with a weapon and no ammo checked places a blaster at
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
