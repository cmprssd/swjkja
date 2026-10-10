# Baseline

The numbers every milestone has to beat, measured on the reference engine. Re-measure
with `bench/baseline.sh` (after `bench/build-openjk.sh`); it opens a game window for
about 30 seconds per run, so it refuses to start while the PC is in use (input in
the last 2 minutes) unless `SWJKJA_ALLOW_WINDOW=1` is set.

## Reference

OpenJK at the revision pinned in ROADMAP.md, Release x64 (MSVC), vanilla renderer,
reading the retail Steam assets. Not the retail `jamp.exe`: on this PC it crashes on
every map load (see DECISIONS.md, 2026-10-07).

Settings: windowed 1280x720, `r_swapInterval 0`, `com_maxfps 0`, sound muted,
otherwise defaults.

## Machine

Intel Core i5-7400T (4 cores, 2.4 GHz), Intel HD Graphics 630 (driver
31.0.101.2141), Windows, game on drive D:.

## Results, 2026-10-07

| Demo | FPS (median) | FPS (each run) | Load, warm (s) | Load, cold (s) |
|------|--------------|----------------|----------------|----------------|
| ffa3 | 197.4 | 197.4 200.8 189.8 199.2 194.4 | 19.67 | 22.38 |

- **ffa3** (`bench/demos/ffa3.dm_26`): multiplayer map `mp/ffa3`, eight bots in a
  free-for-all, camera following one bot, 59 seconds, 1183 snapshots.
- **FPS** is `timedemo`: the demo plays as fast as possible, one snapshot per frame.
- **Load** is `CL_InitCGame` as the engine logs it: loading the map, models, sounds
  and textures until the first frame. The cold run is the first after building; warm
  runs reuse the OS file cache.

## Reading the retail assets (swjkja-fs), 2026-10-07

`cargo run --release -p swjkja-fs --example read_all -- <GameData> <threads>` reads
and checks (CRC-32) every one of the 22,775 files: 3,049 MB uncompressed from the
1.2 GB of `.pk3` archives. Indexing all four archives takes 30-170 ms.

| Archives on | 1 thread | 4 threads |
|-------------|----------|-----------|
| Internal NVMe (C:) | 9.0, 9.7 s | 4.6, 5.1 s |
| USB drive (D:, Sabrent) | 9.7, 20.4 s | 12.2, 14.3 s |

- From NVMe, one thread is limited by decompression (about 330 MB/s of output on
  this 2.4 GHz CPU); four threads give about 2x, not 4x.
- From the USB drive, parallel reads at scattered offsets are slower than one
  thread, and the 7.9 GB of RAM cannot keep all 1.2 GB cached between runs.
- **So the loader should read each archive in offset order on one thread and spread
  only the decompression over the cores.** Loading a map needs a few hundred MB, not
  all 3 GB; this is the full-read upper bound.

## Parsing the retail maps (swjkja-bsp), 2026-10-07

`cargo run --release -p swjkja-bsp --example parse_all -- <GameData>` reads and
parses all 57 maps (521 MB) and checks every cross-reference in them: 15-24 ms per
map to parse (3 runs), after 30-45 ms per map to read it from the USB drive. A
first version took 67-85 ms per map because it formatted an error message for every
check, not just failing ones.

## Decoding the retail textures (swjkja-image), 2026-10-07

All 5,875 textures decode: 4,271 JPEG, 1,134 PNG, 470 TGA. Compared with reference
decodes (`bench/images/reference.py`: Pillow 12.3 with libjpeg-turbo and libpng, and
a hand-written TGA decoder that follows the original's rules):

| Format | Files | Identical pixels | Largest difference |
|--------|-------|------------------|--------------------|
| TGA | 470 | 470 | none |
| PNG | 1,134 | 1,134 | none |
| JPEG | 4,271 | 35 | 5 of 255 levels; mean 0.077, 13% of pixels differ (sample of 303 files) |

The JPEG differences come from the decoder's inverse DCT and chroma upsampling;
`jpeg-decoder` differs too (4,238 files). They are below what the original shows
anyway: with its default `r_ext_compress_textures 1`, the original has the driver
re-compress every texture to S3TC/DXT, which changes pixels far more. See
DECISIONS.md.

To rerun the comparison (Windows Python with Pillow and numpy in
`%USERPROFILE%\.swjkja\venv`):

    cargo run --release -p swjkja-image --example decode_all -- <GameData> > ours.tsv
    python bench/images/reference.py <GameData> > ref.tsv

## Loading the retail shader scripts (swjkja-shader), 2026-10-07

`cargo run --release -p swjkja-shader --example shader_report -- <GameData>`: 68
files, 3,365 shader definitions (both counts cross-checked with a separate Python
count). Loading and indexing all scripts takes 17-21 ms; parsing every definition
9-11 ms (3 runs). The 40 the original rejects are all editor and compiler-only
shaders (caulk, clip, hint, nodraw, ...) without drawing stages. An early version took
1.8 s to parse: each parse rescanned the whole 2 MB text for NUL bytes.

## Building map surfaces (swjkja-world), 2026-10-07

`cargo run --release -p swjkja-world --example build_all -- <GameData>` builds the
drawable surfaces of all 57 maps (patch subdivision, stitching, LOD sync) in 1.20 s;
the slowest is `taspir2` at 141 ms (156 stitches). Comparing patches only within
their LOD group (identical results) took this from 1.84 s. `mp/ffa3`: 4,426 faces,
88 triangle soups, 526 patches (2 `nodraw`, skipped), 100 stitches.

## Visual comparison with the reference, 2026-10-07

`bench/capture-reference.sh` renders the 11 views in `bench/reference/views.txt`
(3 multiplayer maps) with the reference OpenJK, world only, full-quality textures
(the owner's choice: `r_picmip 0`, no texture compression), 1280x720; the reference
build has a test-only patch pinning the camera exactly (`cg_swjkjaView`).
The `render_world` example renders the same views with the new renderer and
`bench/reference/compare.py` compares them.

First comparison, with stand-in shading (base texture x lightmap):

| View | Mean difference (0-255) | Pixels off by > 16 |
|------|------|------|
| ffa3_down | 1.09 | 0.3% |
| ctf4_spawn1 | 1.58 | 0.7% |
| ffa3_spawn2 | 2.44 | 4.9% |
| ffa3_up | 2.66 | 1.5% |
| ctf4_up | 5.25 | 15.2% |
| ffa3_spawn3 | 6.40 | 15.0% |
| duel5_spawn1 | 6.72 | 20.6% |
| duel5_spawn2 | 6.95 | 15.3% |
| duel5_down | 8.18 | 25.3% |
| ctf4_spawn2 | 8.51 | 30.1% |
| ffa3_spawn1 | 18.07 | 42.7% |

Geometry and camera line up exactly (edges match in every view). The large
differences are surfaces whose shaders brighten or blend in extra stages (for
example a `GL_DST_COLOR GL_SRC_COLOR` stage doubles brightness), plus the sky, which
the preview does not draw: the shader-stage system is next.

With the stage renderer (materials resolved and drawn stage by stage as the
original does; no sky or fog yet), 2026-10-07:

| View | Mean difference | Pixels off by > 16 |
|------|------|------|
| duel5_down | 0.08 | 0.0% |
| ffa3_down | 0.16 | 0.0% |
| ffa3_up | 0.21 | 0.1% |
| duel5_spawn1 | 0.24 | 0.1% |
| ffa3_spawn2 | 0.27 | 0.0% |
| ffa3_spawn3 | 0.40 | 0.1% |
| ffa3_spawn1 | 0.53 | 0.1% |
| ctf4_spawn1 | 0.53 | 0.0% |
| duel5_spawn2 | 1.29 | 2.1% |
| ctf4_up | 4.17 | 12.0% |
| ctf4_spawn2 | 7.42 | 28.7% |

`ctf4` has global fog (a teal haze growing with distance), which is not drawn yet;
the remaining `duel5_spawn2` difference is mostly sky (5.7% of the view undrawn).

With global fog (GL_EXP2 on eye-space depth, per-stage fog colours, clear to the fog
colour), 2026-10-07: `ctf4_spawn1` 0.16, `ctf4_spawn2` 0.23 (from 7.42), `ctf4_up`
0.32 (from 4.17), all with 0.0% of pixels off by more than 16. Ten of the eleven
views are now within 0.53 on average; `duel5_spawn2` (1.29) is the undrawn sky.

With the sky (outer box and cloud layers), 2026-10-07: no view has more than 0.1%
undrawn; `duel5_spawn2` 1.23 (2.0% of pixels off), where the rest is glow sprites
(`deformVertexes autoSprite`), which come with deforms.

With flares (screen-facing glows with the original's depth test), 2026-10-07:
`duel5_spawn2` 0.24 with 0.0% of pixels off. **All eleven views are within 0.53 on
average, with at most 0.1% of pixels off by more than 16.**

| View | Mean difference | Pixels off by > 16 |
|------|------|------|
| duel5_down | 0.08 | 0.0% |
| ctf4_spawn1 | 0.16 | 0.0% |
| ffa3_down | 0.16 | 0.0% |
| duel5_spawn1 | 0.20 | 0.0% |
| ffa3_up | 0.21 | 0.1% |
| ctf4_spawn2 | 0.23 | 0.0% |
| duel5_spawn2 | 0.24 | 0.0% |
| ffa3_spawn2 | 0.27 | 0.0% |
| ctf4_up | 0.32 | 0.0% |
| ffa3_spawn3 | 0.40 | 0.1% |
| ffa3_spawn1 | 0.53 | 0.1% |

These are still single moments: the reference shots were taken at an unknown game
time, so animated stages (waves, scrolls, animMaps) can only match by chance; the
next reference capture should pin the time.

Fog volumes (`RB_FogPass`, the fog generators and the fog fade of blended stages)
leave these eleven views unchanged: none of them shows a fog volume. Seven
multiplayer maps have one (`fogs <GameData>` lists them with their bounds: ctf1,
duel2, duel3, duel7, duel9, ffa1, ffa2), but no reference shot shows one yet, so
fog volumes are checked by unit tests and by eye only.

Since 2026-10-07 every view in `views.txt` carries a game time (12345 ms). The
reference engine's patch pins it with `cg_swjkjaTime` (shader time and light
styles), and `render_world` renders at the same time, so animated stages and
flickering lights can be compared. 

Recaptured at the pinned time, 2026-10-07, with the eight fog-volume views from the
`fog_views` example: **all 19 views are within 0.53 mean difference, with at most
0.1% of pixels off by more than 16.**

| View | Mean difference | Pixels off by > 16 | Shows |
|------|------|------|------|
| ffa1_fog_in | 0.05 | 0.0% | inside a fog volume |
| duel5_down | 0.08 | 0.0% | |
| duel9_fog_above | 0.08 | 0.0% | above a fog volume |
| duel9_fog_in | 0.08 | 0.0% | inside a fog volume |
| duel3_fog_above | 0.10 | 0.0% | above a fog volume |
| duel2_fog_above | 0.12 | 0.0% | above a black fog volume |
| ffa1_fog_above | 0.13 | 0.0% | above a fog volume |
| ctf4_spawn1 | 0.16 | 0.0% | global fog |
| ffa2_fog_above | 0.16 | 0.0% | above a fog volume |
| ffa3_down | 0.16 | 0.0% | |
| duel5_spawn1 | 0.20 | 0.0% | |
| ffa3_up | 0.21 | 0.1% | |
| ctf4_spawn2 | 0.23 | 0.0% | global fog |
| duel5_spawn2 | 0.24 | 0.0% | flares |
| ffa3_spawn2 | 0.27 | 0.0% | |
| ctf4_up | 0.32 | 0.0% | global fog |
| ffa3_spawn3 | 0.40 | 0.1% | |
| ctf1_fog_above | 0.42 | 0.0% | above a black fog volume |
| ffa3_spawn1 | 0.53 | 0.1% | |

The time pin works: on the pixels that animate in duel5_spawn2 and duel5_down (where
a time-0 render and a 12.345 s render differ by more than 8), the new reference shots
differ from the 12.345 s render by 0.54 and 0.80 on average, and from the time-0
render by 19.2 and 21.7. In the fog views "undrawn" (pure black) pixels are the
black fogs themselves, which the reference shows black too.

## Static models compared with the reference, 2026-10-10

`bench/reference/model-views.txt` holds three mp/ffa5 views of its static models
(`misc_model_static`: lava rings, lamps, struts, air purifiers), captured with
entities drawn (`ENTITIES=1 bench/capture-reference.sh reference/model-views.txt`)
and placed to keep items and movers out of sight. The rings view draws 8 of the
map's 84 models after culling.

| View | Mean difference | Pixels off by > 16 | Shows |
|------|------|------|------|
| ffa5_lamps | 0.17 | 0.1% | lamp posts at the edges |
| ffa5_rings | 0.22 | 0.1% | four lava rings filling the middle |
| ffa5_pipes | 3.87 | 4.4% | rings and an air purifier; the original's view saber |

The models match: on the rings (the box around them in ffa5_rings) ours differs by
0.6 on average, +0.28/+0.05/+0.20 per channel (no bias, so the light grid lighting
agrees), with 2.3% of pixels off by more than 8. ffa5_pipes differs mostly by the
original's first-person lightsaber, which `cg_drawGun 0` does not hide; ffa5_rings
also shows one world flare near the floor that the original does not draw.

The 19 world views (references taken with `r_drawentities 0`) are unchanged by the
models: all within 0.41 mean difference, at most 0.1% of pixels off by more than 16.

## Single-player rack weapons compared with the reference, 2026-10-10

Two t2_dpred views in `bench/reference/model-views.txt` look at a row of seven racks:
dpred_racks the whole row from 170 units (gun racks with blasters and repeaters, an
ammo rack with a blaster), dpred_guns two gun racks from 76 units. The weapons are
Ghoul2 models (`.glm`); they stand upright in the racks only once posed from the
renderer's root matrix, a quarter turn about z (see DECISIONS.md).

| View | Mean difference | Pixels off by > 16 | Flares left out | Without weapons, flares left out |
|------|------|------|------|------|
| dpred_racks | 7.14 | 12.3% | 0.68, 1.6% | 0.64, 1.6% |
| dpred_guns | 6.72 | 12.3% | 1.29, 3.0% | 1.33, 3.5% |

Most of the difference is world flares the original does not draw (as on ffa5_rings
and t2_rancor). With them patched out of this renderer for a measurement (both
columns on the right; the weaponless renders are from `main`), what remains is the
weapons' outlines: the original turns each one at random (up to 14 degrees of yaw
and 4 of pitch and roll) and moves it up to 2 units, which this renderer leaves out,
so a drawn weapon is a little off its reference and counts twice. Up close the
models, heights and shading match (`dpred_guns`).

The other model views are unchanged (dpred_canopy 1.34, ffa5 0.17, 3.87 and 0.22,
stamp_barrels 0.13, stamp_crates 5.00, vjun1_hall 1.48). hoth2_fog and rancor_plate
were not rendered: both maps now stop `render_world` on the Windows GPU with
`ERROR_DEVICE_LOST`, on `main` too.

## Single-player ammo rack compared with the reference, 2026-10-10

stamp_barrels (above) has an ammo rack on the wall with a blaster pack and a medpack.
With racks drawn it matches the original at 0.13 mean difference (0.16 before), 0.0%
of pixels off by more than 16, against the same reference capture. Around the rack
(130x130 pixels) the difference is 0.36, an outline on the medpack: the original
spreads rack items at random (here up to 2 units and 4 degrees), which this
renderer leaves out (see DECISIONS.md).

## Single-player breakables compared with the reference, 2026-10-10

Two t3_stamp views in `bench/reference/model-views.txt`, looking at breakables
(`misc_model_breakable`) 350 units away, captured like the single-player static-model
views, with two changes to `bench/capture-reference.sh`: the player stands 128 units
behind the eye (t3_stamp starts the player with a saber, and single player draws a
saber holder in third person, so the player's body stood around the eye), and NPCs
are frozen (`d_npcfreeze 1`; one capture without it found the barrels already
broken). Two captures with the final script gave the same numbers.

| View | Mean difference | Pixels off by > 16 | Shows |
|------|------|------|------|
| stamp_barrels | 0.16 | 0.1% | barrels (`crate_02`) and small crates (`crate1`) by a wall, a control station |
| stamp_crates | 5.00 | 13.8% | crates by a container, a corridor |

stamp_barrels differs only by an ammo rack on the wall (`misc_model_ammo_rack`, not
drawn yet). stamp_crates differs on its left, where the original draws brush
entities (stacked containers and stairs: `func_breakable`, `func_static`), which this
renderer does not draw yet, and the corridor behind them shows instead. The four
single-player static-model views were captured before the script changes and not
captured again.

## Single-player static models compared with the reference, 2026-10-10

Four single-player views in `bench/reference/model-views.txt`, captured with OpenJK's
single-player executable (patched like the multiplayer one to pin the view and game
time) at skill 0, in first person, after skipping the map's opening cutscene
(`exitview`) and moving the player to the view (`noclip`, `setviewpos`).

| View | Mean difference | Pixels off by > 16 | Shows |
|------|------|------|------|
| dpred_canopy | 1.34 | 0.0% | tree models filling the view, the ground between them |
| hoth2_fog | 0.76 | 0.0% | rock models in the cave's fog |
| rancor_plate | 0.47 | 0.3% | wall plates in a hall |
| vjun1_hall | 1.48 | 3.1% | air purifiers and light blocks |

vjun1_hall differs by the original's green laser beams (effects, not drawn here) and
highlights on the air purifiers' tops, not yet explained (their shader has no
environment stage). The three mp/ffa5 views are unchanged (0.17, 3.87 and 0.22).

Two candidate views (t2_dpred, t2_rancor) were dropped: the original left parts of
their screens undrawn, showing earlier frames, because single player never clears the
screen and draws only the areas the player's snapshot marks visible. One of them had
its eye inside a solid leaf, where single player draws no static models at all; this
renderer drew the trees there until it followed the same rule
(`single_player_draws_no_models_from_a_view_in_solid` in `crates/render/tests`).

## Renderer frame times (swjkja-render), 2026-10-07

`bench_world <GameData> bench/reference/views.txt` renders each reference view 300
times (after 20 warm-up frames) at 1280x720 on the Intel HD Graphics 630. "GPU ms" is
the GPU's own time between timestamps around the frame; "frame ms" adds recording,
submitting and waiting for each frame (the loop waits every frame, so this is not a
pipelined frame rate). Frame times vary by up to 1.6x between runs when the machine
is busy (42% CPU load from other work during these runs); GPU times stay within 10%.

| View | GPU ms, everything drawn | GPU ms, with the original's culling |
|------|------|------|
| ffa3_spawn1 | 5.92 | 3.97-4.67 |
| ffa3_spawn2 | 5.00 | 3.35-4.06 |
| ffa3_spawn3 | 4.84 | 3.38-4.12 |
| ffa3_down | 4.35 | 1.15-1.27 |
| ffa3_up | 4.87 | 2.08-2.23 |
| duel5_spawn1 | 4.53 | 4.29-4.54 |
| duel5_spawn2 | 4.17 | 4.18-4.67 |
| duel5_down | 5.58 | 4.26-5.33 |
| ctf4_spawn1 | 3.55 | 1.55-1.73 |
| ctf4_spawn2 | 4.51 | 3.03-3.59 |
| ctf4_up | 4.69 | 2.83-3.56 |

Culling (PVS, view frustum, face planes, as `R_MarkLeaves`, `R_RecursiveWorldNode`
and `R_CullSurface` do it) leaves every reference render identical to before, and
cuts GPU time by up to 3.8x where the view sees little of the map. duel5 is one open
arena, so nearly everything is in view and culling saves nothing. In the quiet run
the whole frame took 2.4-5.8 ms (170-420 FPS): 1-1.5 ms more than the GPU time,
spent recording and waiting.

## Loading map textures (swjkja-render), 2026-10-10

`load_profile <GameData> <runs> <map>...` times each loading phase. Textures are
most of a map's load: ffa3 at full quality (`r_picmip 0`, uncompressed) is 188 MB of
texture data with mipmaps. Mipmaps are now made as the original makes them
(`R_MipMap`, box filter), which brought every reference view closer (worst 0.53 ->
0.41). Decoding runs on all cores, and uploads go in a few batches instead of one
GPU round trip per texture.

Renderer load (materials, decoding, mipmaps, uploads), ms, 6 runs each, old and new
alternating on a busy machine (60-70% CPU load from other programs):

| Map | Old, best | New, best | Old, median | New, median |
|-----|------|------|------|------|
| mp/ffa3 | 998 | 531 | 2636 | 1217 |
| mp/ctf1 | 810 | 514 | 1542 | 1039 |
| mp/duel5 | 576 | 382 | 1046 | 950 |

Lesson from this measurement: mip chains must be built in ordinary memory and
written once to the staging buffer. Building them in place in the staging memory
(write-combined, very slow to read back) made loading 3x slower.

## Same camera path, both engines (world only), 2026-10-10

The reference's timedemo includes bots, models, effects and the HUD, which the new
engine does not draw yet. To compare like with like, `bench/path-reference.sh` plays
the ffa3 demo in the reference with entities and HUD off (`r_drawentities 0`,
`cg_draw2D 0`) at the screenshot comparison's settings (`r_picmip 0`, uncompressed
textures, no dynamic glow), and its `cg_swjkjaPath` patch logs every frame's view,
game time and a microsecond timestamp. Every run followed the identical path (the
script checks), which is committed as `bench/reference/ffa3.path` (1182 frames).
`bench_path` renders that path with the new renderer at the same game times. Both
frame times are the interval between the same point in consecutive frames, and both
sets of statistics come from the same code (`bench_path --stats`). The FPS computed
from the reference's timestamps matches its own timedemo result to 0.3 FPS.

Alternating runs, three each, 1280x720 on the Intel HD Graphics 630 (Windows, other
programs running: Chrome, Steam, GeForce NOW; 20-40% CPU load):

| Engine | FPS (each run) | Median ms | p99 ms | 1% low FPS | Max ms |
|------|------|------|------|------|------|
| Reference (OpenJK) | 240.3, 241.9, 232.6 | 3.81-4.02 | 11.4-15.2 | 49-61 | 28.5-30.1 |
| New renderer | 246.0, 248.6, 249.2 | 3.68-3.85 | 11.1-13.5 | 61-69 | 20.6-23.5 |

**The new renderer is 3-6% faster on average, with better 1% lows and worst frames,
on the same scenes.** That is not yet a convincing margin, and the reason is known:
the bench loop waits for every frame before recording the next, so a frame costs
recording (0.2-0.4 ms) plus GPU (2.8 ms on average) plus the wait, about 3.7 ms.
The reference overlaps CPU and GPU work. The new renderer's GPU time alone allows
about 350 FPS; a presentation loop with frames in flight (the next milestone step)
removes the serial wait.

Frame-time spikes on our side: in frames over 7 ms the recording took 0.2-0.9 ms and
the GPU 2-5 ms, yet the frame took 7-23 ms. The lost time is in the per-frame submit
and fence wait (the thread sleeps and wakes late), and in a few frames in preemption
by other programs (recording 7-14 ms instead of 0.3). The serial loop causes the
first; background load the second.

Background load matters more than anything measured here: two earlier reference runs
in the same session ran at 135 and 143 FPS (p99 43-45 ms) and then 231-233 FPS
with nothing changed. Comparisons are therefore made with alternating runs.

### With frames in flight, 2026-10-10

`FrameLoop` keeps two frames in flight: the CPU records a frame while the GPU draws
the previous one, and per-frame data (uniforms, deformed geometry, visible index
lists) has a copy per frame. `bench_path` now renders through it. Alternating runs
in one session (quieter machine than above, so the reference ran faster too):

| Engine | FPS (each run) | Median ms | p99 ms | 1% low FPS | Max ms |
|------|------|------|------|------|------|
| Reference (OpenJK) | 231.2, 255.9, 245.5 | 3.78-4.16 | 6.6-7.8 | 94-108 | 29.2-31.4 |
| New renderer, frames in flight | 401.9, 398.7, 403.9 | 2.44-2.48 | 6.2-6.5 | 139-150 | 7.7-9.8 |

**1.6-1.7x the reference's average frame rate on the same scenes, with better 1%
lows and worst frames a third as long.** Against the serial loop (from `main`,
alternating, 2 runs each of 3 launches): 258-272 FPS serial, 398-439 FPS with frames
in flight.

What limits it now is the GPU: the mean frame time (2.3-2.5 ms) equals the GPU's
own time per frame. The GPU time itself fell from 2.8 ms (serial) to 2.3 ms;
likely because the integrated GPU no longer idles between frames and keeps its
clock up (not measured: the clock is not logged). Still not compared: presenting
to a window, which the reference does and this bench does not.

### Presenting to a window, 2026-10-10

`swjkja view <GameData> mp/ffa3 --replay bench/reference/ffa3.path <out>` draws the
same path in a 1280x720 window (B8G8R8A8_UNORM swapchain, IMMEDIATE present mode,
the target copied to the swapchain image each frame) and writes its timestamps in
the path format. Alternating runs (reference, window, offscreen), three each, on a
busier machine (Chrome using about 1.4 of the 4 cores):

| Run | FPS (each run) | Median ms | p99 ms | 1% low FPS | Max ms |
|------|------|------|------|------|------|
| Reference (OpenJK, window) | 195.9, 196.8, 202.4 | 4.29-4.48 | 10.4-11.2 | 69-74 | 33-41 |
| New renderer, window | 159.0, 162.9, 160.2 | 5.57-6.35 | 16.8-18.6 | 48-51 | 26-30 |
| New renderer, offscreen | 281.3, 279.6, 289.8 | 2.86-3.12 | 10.0-12.8 | 42-92 | 12-49 |

**In a window the new renderer is slower than the original on this machine (about
0.8x), although it is 1.4x faster offscreen in the same runs.** Presenting costs
about 3 ms per frame. Where it goes, from timing each step over 500 frames: the
fence wait 2.7-3.6 ms, acquiring the image 0.6-0.9 ms, recording and submitting
2.1-2.4 ms, `vkQueuePresentKHR` 0.13 ms. The CPU waits on the GPU for half of each
frame although our own GPU work (2.8-3.2 ms with timestamps) is the same as
offscreen, so the GPU is busy with presenting: the copy to the swapchain image, and
the desktop compositor's work on the same integrated GPU. Ruled out: the event loop
(drawing outside `RedrawRequested` moved the time into the fence wait, no gain), a
fourth swapchain image (155-158 -> 164-167 FPS, within the noise). MAILBOX is not
offered by this driver (only IMMEDIATE and FIFO).

Waiting for the swapchain image only at the copy (`TRANSFER` instead of
`ALL_COMMANDS`, so drawing the world need not wait for the image) made no difference:
the two builds alternating five times each, 130-149 and 132-144 FPS.

**Window and exclusive fullscreen, 2026-10-10**, both at 1280x720, alternating
(reference window, reference fullscreen, viewer window, viewer fullscreen), three
rounds, on a busier machine still (about half of the CPU taken by Chrome, Steam and
GeForce NOW):

| Run | FPS (each run) | Median ms | p99 ms | 1% low FPS | Max ms |
|------|------|------|------|------|------|
| Reference, window | 141.2, 139.7, 136.9 | 5.60-5.70 | 25.5-26.4 | 32-36 | 38-44 |
| Reference, fullscreen | 163.6, 164.5, 165.0 | 4.77-4.97 | 17.4-19.3 | 41-44 | 36-39 |
| New renderer, window | 174.4, 160.3, 160.2 | 5.09-5.49 | 25.3-26.4 | 31-34 | 37-39 |
| New renderer, fullscreen | 206.7, 196.2, 197.3 | 3.70-4.03 | 23.5-27.0 | 27-28 | 86-141 |

- **In this load the new renderer is faster in a window too** (1.15-1.25x), and in
  the earlier alternating run of the day too (138-173 against 107-118 FPS). The
  difference from the table above is the reference, not the new renderer: the new
  renderer's window rate is the same (159-163 then, 160-174 now), while the
  reference fell from about 198 to 139 FPS. The reference is bound by the CPU (one
  thread, the OpenGL driver's work on it) and slows down with the PC's load; the new
  renderer in a window is bound by the GPU and does not. So which is faster in a
  window depends on the load: on an idle PC the original still wins.
- **The compositor costs both about 1 ms per frame**: fullscreen gains 1.0-1.2 ms
  per frame for the reference and 1.0-1.4 ms for the new renderer. It is not what
  makes our window frames slower than offscreen.
- Left over: in fullscreen our frame is still about 1-1.5 ms longer than offscreen
  (3.7-4.0 against 2.9-3.1 ms median) and than our own GPU work, so presenting on
  this driver costs GPU time even without composition, perhaps a copy of its own in
  Intel's Windows Vulkan presentation. PresentMon (its present mode per frame)
  would show it; it needs administrator rights.
- Our fullscreen runs have stalls the reference does not (worst frames 86-141 ms,
  1% lows 27-28 FPS against 41-44): explained below (one stall in the first frames
  after the mode switch).

To rerun these: `FULLSCREEN=1 bench/path-reference.sh` and `swjkja view ...
--replay ... --fullscreen` (exclusive fullscreen at 1280x720; the screen switches
to that mode).

**Where presenting goes, 2026-10-10.** `swjkja view ... --timings <file>` writes
every frame's CPU steps (event loop, fence wait, acquire, record and submit,
present) and GPU timestamps (the world, the copy to the swapchain image, the gap
since the previous frame's GPU work); `bench/timings.py` summarises them. PresentMon
2.6 (run as administrator alongside) shows how Windows presented each frame.

- **No hidden presentation copy in fullscreen.** PresentMon reports "Hardware:
  Legacy Flip" for both engines in exclusive fullscreen, and the GPU gap between our
  frames is 0.02 ms (median). In a window both engines are "Composed: Copy with GPU
  GDI", and the gap is 1.1-1.25 ms: the compositor's copy.
- **The leftover is our own copy to the swapchain image: 0.6-0.8 ms of GPU per
  frame** (0.6 in a window, 0.8 in fullscreen), plus a slightly slower world in
  fullscreen than offscreen (2.5-2.7 against 2.3-2.5 ms). A plain `vkCmdCopyImage`
  from a BGRA target costs the same 0.8 ms as the swizzling blit (two runs each).
- **Drawing straight into the swapchain image** saves the copy in a window (GPU
  period 5.7 -> 4.9 ms, world unchanged at 3.1 ms) but costs more than it saves in
  exclusive fullscreen: the world takes 4.1 ms there instead of 2.5 ms (likely
  because those images are the ones the display scans out, which the driver does
  not compress). So the viewer draws straight into the image in a window and keeps
  the copy in exclusive fullscreen (`Swapchain::direct`).
- **The fullscreen worst frames were one stall right after the mode switch**: the
  third frame waits 68-160 ms in the event loop, every run. It made the max 86-160
  ms and pulled the 1% lows down (the slowest 1% is 12 frames, and one of them was
  that stall). The original's timed demo starts seconds after its mode switch, so
  replays now draw their first view for 100 frames (about half a second) before the
  timed path starts.
- **The other slow frames (15-30 ms, every 100-200 ms) are the machine's load**,
  not presenting: their time lands in recording (one thread, no per-frame threads),
  the fence wait or the event loop, a different one each time, in window and
  fullscreen alike. Their number follows the background load (28-60 per run when
  Chrome, Steam and GeForce NOW took half the CPU, few on a quieter machine).

Before and after (old build: copy always, no warm-up; new: direct in a window, copy
in fullscreen, warm-up), alternating, three runs each, busy machine:

| Run | FPS (each run) | Median ms | p99 ms | 1% low FPS | Max ms |
|------|------|------|------|------|------|
| Old, window | 148.5, 135.9, 143.6 | 5.13-5.91 | 26.1-28.7 | 27-33 | 39-56 |
| New, window | 168.5, 159.9, 172.2 | 4.00-4.50 | 22.3-24.4 | 33-39 | 34-49 |
| Old, fullscreen | 219.1, 207.7, 191.8 | 3.43-3.97 | 25.0-26.1 | 27-33 | 46-116 |
| Direct, fullscreen | 186.1, 186.0, 177.2 | 4.05-4.33 | 28.6-29.6 | 31-33 | 33-38 |

(The last row is the rejected variant, direct drawing in fullscreen too; the new
build copies in fullscreen like the old one.) Then the new build against the
reference, alternating, three rounds, same busy machine:

| Run | FPS (each run) | Median ms | p99 ms | 1% low FPS | Max ms |
|------|------|------|------|------|------|
| Reference, window | 122.5, 137.2, 107.8 | 5.12-8.33 | 28.7-30.3 | 26-31 | 42-67 |
| New renderer, window | 157.9, 157.8, 159.3 | 4.59-4.71 | 22.4-26.6 | 35-40 | 32-37 |
| Reference, fullscreen | 142.5, 159.0, 145.1 | 4.78-5.24 | 17.4-36.0 | 24-34 | 52-58 |
| New renderer, fullscreen | 209.2, 196.8, 192.3 | 3.72-4.26 | 16.6-27.3 | 34-48 | 27-34 |

**Under load the new renderer is now 1.15-1.45x the original in a window and
1.2-1.45x in exclusive fullscreen, with better 1% lows and worst frames in both.**
The load favours us (the original is bound by its one CPU thread); in the quieter
fullscreen run earlier in the day the two were level (216.5 against 214.7 FPS,
both with the copy, before the warm-up). What limits our fullscreen frame now is
the GPU: world 2.5-2.7 ms plus the 0.8 ms copy. Not tried: doing the copy in a
compute shader.

To rerun (opens a game window per run; see `bench/lib.sh`):

    bench/path-reference.sh        # reference; RUNS=3, writes to bench/out
    cargo run --release -p swjkja-render --example bench_path -- --stats bench/out/ffa3-*.txt
    scripts/cargo-msvc.sh run --release -p swjkja-render --example bench_path -- \
        <GameData> mp/ffa3 bench/reference/ffa3.path 3

**On an idle PC, 2026-10-10** (Chrome, Steam and GeForce NOW closed, CPU load
about 4% before the runs), the new build against the reference, alternating
(reference window, reference fullscreen, viewer window, viewer fullscreen), three
rounds, both at 1280x720:

| Run | FPS (each run) | Median ms | p99 ms | 1% low FPS | Max ms |
|------|------|------|------|------|------|
| Reference, window | 267.9, 265.0, 270.7 | 3.59-3.65 | 6.18-6.93 | 104-108 | 29-31 |
| New renderer, window | 285.3, 280.9, 281.7 | 3.43-3.58 | 6.78-7.32 | 120-122 | 10-13 |
| Reference, fullscreen | 289.7, 279.3, 287.3 | 3.37-3.48 | 5.54-5.94 | 117-127 | 27 |
| New renderer, fullscreen | 305.5, 328.8, 327.9 | 2.99-3.15 | 5.33-6.00 | 110-162 | 8-23 |

**Milestone 2's speed goal is met: the new renderer is faster than the original on
the same path in both modes, on an idle PC too.** Every run of ours beats every run
of the reference: 1.04-1.08x in a window (the margin drawing straight into the
window's image bought; before it the original was ahead there) and 1.05-1.18x in
exclusive fullscreen. Our worst frames are 8-23 ms against the original's 27-31 ms;
our p99 in a window is slightly higher (6.8-7.3 against 6.2-6.9 ms). The idle
margins are much smaller than under load (1.15-1.45x), where the original, bound by
its one CPU thread, slows down and we do not; and far below offscreen (1.4-1.7x),
so presenting is still most of what separates the two in a window.

## The viewer's loading screen, 2026-10-10

`swjkja view` now loads the map on its own thread and shows a progress bar from
the start (it prints when its first frame was presented, and each loading step's
time). On the Intel HD 630, mp/ffa3 in a window:

- **First frame 260-350 ms after the window is asked for** (6 runs); before, the
  window stayed blank until the map was loaded (the times below). Before it:
  creating the window about 100 ms, the Vulkan device 100-115 ms, the swapchain
  80-100 ms, the target 40-90 ms. Drawing the first frame at the first redraw
  instead of right away cost about 100 more.
- **Loading takes 0.75-1.2 s with the game files in the file cache and 3-9 s
  without** (game data 2.4-3.2 s, textures and pipelines 0.5-5.5 s; the archives are
  on the USB drive).
- **The window stays responsive while loading**: it moves, answers messages within
  65 ms at worst, and closes 240-320 ms after `WM_CLOSE` while textures are being
  uploaded (checked with a PowerShell probe that moves and closes it).
- **Replay times are unchanged**: alternating four runs each, 255-265 FPS (median
  3.58-3.75 ms) before and 253-259 FPS (3.62-3.77 ms) after, within the run-to-run
  spread.

## Not measured yet

- **Frame-time spikes** (1% lows) **of the full game**: the path benchmark above has
  per-frame times for the world only; the reference with bots, models and effects
  still reports only the timedemo average.
- **Single-player**: the SP engine has no `timedemo`; it needs a scripted camera
  path or save-game route.
- **Linux**: the reference runs on Windows only so far.
