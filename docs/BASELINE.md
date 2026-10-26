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

## Single-player NPCs compared with the reference, 2026-10-11

Three views in `bench/reference/model-views.txt` look at NPCs that spawn with the
map, captured with NPCs frozen from the first frame (`d_npcfreeze 1` before the map
runs: their AI does not run, their movement frames do). The NPCs hold their
weapons, bolted to their hands, dropped to the floor by a box trace against the
map's brushes and solid entities, and stand as they do at the view's time: after
five seconds standing still they play their stance's idle animation
(`BOTH_STAND3IDLE1` for a gun), each bone at the frame ghoul2's timing has reached,
lerped toward the next. Specular stages (`alphaGen lightingSpecular`) light their
armour and weapons as the original does.

The captures pin the original's ghoul2 clock to the view's time (12345 ms), as they
pin the shader clock: before, each NPC's frame depended on how long the capture
ran. They were captured again for this; "Before" is the previous step (each part at
its stance's first frame) against the same captures.

| View | Mean difference | Before | Around the NPCs | Before |
|------|------|------|------|------|
| dpred_troopers | 1.13 | 1.17 | 4.55 / 5.81 | 5.87 / 6.29 |
| vjun2_officer | 0.58 | 0.60 | 5.89 | 8.74 |
| kor2_troopers | 0.39 | 0.49 | 2.81 | 5.98 |

"Around the NPCs" is the bounding box of the largest group of 16-pixel blocks with
more than a tenth of their pixels off by more than 16 in either render (the same
box for both renders). On dpred that group is now the foreground grass the
reference draws and this renderer does not (14.69 here, 14.72 before), so the table
gives a box around each of its two stormtroopers in front (x 540-630, y 390-640 and
x 720-820, y 380-610). These numbers are not comparable with the earlier table's:
the references changed.

The patched cgame logs each NPC's animation timing in the capture (one
`<map>-npcs.txt` per map): the idle starts at 4989-5003 ms on the client's clock for
NPCs still from the start, 6538-6544 for those that land on a slope, as the rule
here gives (5000, 6550). Poses now match; what still differs around them: the
original draws blob shadows under NPCs, and an NPC that lands on a slope stops a
little higher there (up to half a unit) than the box trace straight down puts it.

The NPC views were first captured after about 600 frames of AI: the original had
turned some NPCs (vjun2's officer by 90 degrees), which nothing at spawn decides.

The other model views are unchanged (dpred_canopy 1.34, dpred_racks 7.14,
dpred_guns 6.72, stamp_barrels 0.13, stamp_crates 5.00, ffa5_lamps 0.17, ffa5_pipes
3.87, ffa5_rings 0.22; against the earlier captures, pixel for pixel the same but
stamp_barrels, whose two NPCs behind the wall now stand in their idle).

Posing NPCs every frame, skinned on the GPU, leaves these numbers as they are: the
GPU's rounding moves 180-260 edge pixels per NPC view (dpred's first box 4.55 ->
4.54), and the other views are pixel for pixel the same (hoth2's, t2_rancor's and
vjun1's not checked: they lose the device on this GPU, on main too). Frame times with
`bench_world` on the three NPC views stay at main's (Intel HD 630: dpred 6.7-6.8
ms, vjun2 5.2-5.3, kor2 8.4-8.9).

Blending between animations (100 ms when a part's animation changes) leaves the
three views pixel for pixel the same: at 12345 ms no NPC in them is blending. Their
blends would not show anyway: `BOTH_STAND1` and `BOTH_STAND3` (with
`TORSO_WEAPONIDLE3`, the same frames) lie within 0.06 units of their idles' first
and last frames, bone for bone. vjun2's imperial (entity 34) holds a pistol in
`TORSO_WEAPONIDLE2`, 24 units from its idle at the hands; a view in front of it
(`vjun2 vjun2_imp_a 626 1654 3095 20 135 0`) was captured once at 5039 ms, 47 ms
into its blend (the log: the idle started at 4992 on the client's clock), and
rendered at 5047 (47 ms into ours, which starts at 5000):

| | Mean difference | NPC's box (x380-880 y150-720) | Box pixels off by > 32 |
|------|------|------|------|
| main (no blend: the idle's pose) | 1.53 | 3.84 | 3.6% |
| blended | 1.07 | 2.33 | 1.6% |

The game's log gives each part's `blendFrame`; every legs value in the three maps'
logs is the frame `Animation::blend_from` gives (dpred 15297.06 at 5003, kor2
14651.90 at 4995, 15296.04 at 6552), and the imperial's torso 15456.00. The
torso's `TORSO_WEAPONIDLE3` values are not (15296.24 against our 15297.12 at 5003):
the original's torso stance starts about 15 ms later than 1550, or 19 ms earlier;
its two frames are within 0.05 units, so it does not show. `bench_world` on the
three NPC views: within the HD 630's run-to-run spread of main's (two runs each,
both ways).

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

## Brush entities moved by the level's scripts, 2026-10-19

The first of the movers' motion: brush entities that a level script moves. Across
the 34 single-player maps, the runners their player's spawn point starts move brush
entities on two (a throwaway survey of every runner's script):
t3_stamp's cranes (`t3_stamp/crane_move`, run by the player's spawn point) and
t3_hevil's two clip brushes (no surfaces). On t3_stamp, every 7 s from 9100 ms a
crane (a `func_static` drawing `s_crane02.md3` as its "model2") and the crate it
carries (another, with surfaces) shuttle between two `ref_tag`s 6,144 units apart:
back at once, then 28 s forward, slowing to a stop (`TR_NONLINEAR_STOP`), for good.

ICARUS's `move` (`Lerp2Pos`) is now modelled for the brush entities scripts name:
from where the entity is to the tag (or point) over the move's time, its task
completing on the mover's first turn there (`G_MoverTeam`), so the next move starts
a frame later. `affect` finds a mover by its "script_targetname", the last spawned
of a name (the name map keeps the last registered): t3_stamp has two func_statics
named for each crane, and the one spawned first stays. The original's ICARUS trace
of the capture (`g_ICARUSDebug 4`) gives the same times: crane_1 back at 9100 and
forward from 9150, crane_2 at 16150 and 16200. A moving entity is drawn by a bone
placing its model where it is (its "model2", baked where it spawns, by a
translation), and culled, linked and lit there.

A new view, stamp_cranes, looks along the crane line at 22000 ms, crane_1 and its
crate 400 units ahead and still moving (13 units a frame there). Captured with
scripts running (`SCRIPTS=1 ENTITIES=1`):

| View | Mean difference (cranes where they spawn, as main) | Exact pixels |
|------|------|------|
| stamp_cranes | 0.01 (2.61) | 97.8% (82.9%) |

- The capture needed two additions to the capture patch. t3_stamp's intro camera
  never ends with the AI frozen (it waits on navgoals), and the server built the
  snapshot from the camera: the first capture showed no entities in the corridor at
  all. A view pinned by `cg_swjkjaView` is now the snapshot's eye too
  (`SV_BuildClientSnapshot`). And SP's cgame placed a moving mover at the real
  `cg.time`, not the pinned one: the second capture showed crane_1 where ours is at
  20500 ms (2.67 against ours at 22000). A mover's position is now evaluated at
  `cg_swjkjaTime` as its angles were. The other captures predate both and were not
  taken again.
- What is left is spread evenly (the crane's region 0.010, the rest 0.007-0.013;
  129 pixels off by more than 2): the move itself is exact.
- The 47 other views render pixel for pixel as on main (no other view's scripts
  move a drawn brush entity).
- `bench_world` (600 frames, alternating with main, three runs each): t3_stamp's
  view at 9000 ms (nothing moved yet, the same scene as main) 6.91-7.08 ms against
  main's 7.04-7.13, GPU 5.30-5.34 against 5.26-5.29; stamp_cranes 6.88-7.08 against
  main's 6.91-7.07 (main's cranes stand elsewhere); yavin1_students (its door's
  slide now linked where it is) 10.41-10.56 against 10.46-10.49.
- Left out: `move`'s angles and `rotate` (t3_hevil's intro ship turns as it lands,
  and is a `misc_model_breakable`, not a brush entity: also left out), a Ghoul2
  "model2" moving, a moving model's fog (taken where it spawns), and scripts run by
  what the map's triggers and buttons do later (elevators on t1_fatal, hoth2 and
  others).

## Scripted turns and breakables, 2026-10-19

The second piece of the movers' motion: what scripts turn, and the breakable models
they move. t3_hevil's player spawn point starts t3_hevil/playerSpawn, whose intro
ship (`playerIntroShip`, a `misc_model_breakable` drawing `z_95.md3` at yaw 360)
waits 1500 ms, then turns to yaw 135 over 4 s (`rotate`) and lands on the
`ref_tag` playerShipIntroLand over 5 s (`move`), both easing out
(`TR_NONLINEAR_STOP`).

ICARUS's `rotate` (`Lerp2Angles`) is now modelled for the entities scripts move: from
its angles to the new ones the short way (`AngleSubtract`), its task completing as
its time runs out (`anglerCallback`, a think). So are `move`'s angles (`Lerp2Pos`
with angles: `AngleDelta`, in 65536ths of a turn), its task completing as the move
gets there. A breakable with a "script_targetname" is a mover scripts can name. A
moving model is drawn by a bone turning it about where it spawned and moving it
there, culled where it is and lit at its origin; its linked box moves but does not
turn (`SV_LinkEntity` turns only brush models' boxes). The original's ICARUS trace
(`g_ICARUSDebug 4`) gives the same times: the ship's `rotate` and `move` at 3600.

A new view, hevil_ship, looks up at the ship at 6000 ms, 70% of the way down and
109 degrees into its turn. Captured with scripts running (`SCRIPTS=1 ENTITIES=1`):

| View | Mean difference (the ship where it spawns, as main) | Exact pixels |
|------|------|------|
| hevil_ship | 0.55 (1.19) | 97.5% (95.8%) |

- The ship itself is exact but for 6 pixels off by more than 2 (a 360 x 200 box
  around it: 0.018).
- The rest is the sky, brighter in the reference by about 23 levels (0.540 of the
  0.544 is in the top 200 rows). This is the first t3_hevil view that shows sky; not
  examined. The map's two `env/lbolt1` effects (lightning, `fx_runner`s that play
  from the start) are a candidate.
- The first capture showed the ship in place but fully turned: the capture patch
  evaluated a mover's angles at the pinned time only for `TR_LINEAR` turns
  (func_rotatings), and a script's turn at cgame's real clock, past its end. Every
  mover's angles now take the pinned time (DECISIONS.md), and the reference engine
  was rebuilt with it.
- The 48 other views render pixel for pixel as on main (no other view's scripts
  move a breakable or turn anything).
- Left out: a moved breakable's scale (the original's `CG_Mover` drops it), a turned
  "model2"'s "modelAngles", the linked box of a brush entity that turns from zero
  angles (the original links it by a cube around it), a rotate's or move's angles
  given as a tag, and a breakable no `move` has made a mover turning more than once
  (its server angles stay where it spawned).

## What triggers start, and used doors closing, 2026-10-19

The third piece of the movers' motion. Elevators such as t1_fatal's and hoth2's are
`func_static`s that scripts move (`common/elevator`, `T1_Fatal/Elevators/*`,
`Hoth2/Elevators/*`), run by a `target_scriptrunner` that a `trigger_multiple` uses
when the player walks into it. The player does not move here, so what a trigger or
button does is pinned as the name it uses: a level takes uses, each made in the
first frame at or after its time, before that frame's entities run, so a runner
without a delay runs its script in that frame. The reference capture makes the same
uses (`g_swjkjaUse`, a patch addition calling what the `use` command calls), and the
views file gives them in a map's first view's eleventh column.

Doors used by name (by a pinned use or a script's `use`) now open, stand open
"wait" seconds (2 when 0) and close again: a team on its first door's times, the
end reached on the first frame at or after the move's time runs out, the close on
the first frame "wait" after that. A use while it stands open puts the closing off;
a toggled door waits for the next use; one with a negative wait stays open. Of the
458 single-player doors, 101 open by use (counted with Python from the entity
text).

Two new views at 7000 ms, `elev1_low` used at 3000 and `Sateliteroom_Door` at 4500,
captured with scripts running (`SCRIPTS=1 ENTITIES=1`):

| View | Mean difference (nothing used, as main) | Pixels off by > 16 (main) |
|------|------|------|
| fatal_elevator | 0.03 (1.01) | 0.0% (2.6%) |
| fatal_door | 0.01 (18.44) | 0.0% (32.0%) |

- fatal_elevator looks down the first reactor shaft at `reactor_elevator1`: the
  runner waits 2.2 s, the elevator rises 498 units over 4 s from 5250 (the
  original's ICARUS trace gives the same frame for its `move`). 98.6% of pixels are
  exact. Two spots differ by more than 2: a ledge at the right edge, 4-5 levels
  brighter in the reference (4,280 pixels; the same with nothing used, so not the
  motion), and 409 pixels of the platform's red light panel, up to 29 levels
  brighter in the reference (not examined).
- fatal_door looks at `Sateliteroom_Door` closing: it rises 122 units in 305 ms
  from 4550, stands open on the frame at 4900 and closes from 6900; at 7000 it is
  100 ms into closing. 98.0% of pixels are exact, none off by more than 2.
- The first capture crashed on the first frame: the patch's parsing of
  `g_swjkjaUse` needs a parse session in single player (`COM_ParseSession`).
- The 48 other views render byte for byte as on main (none uses anything).
- Left out: the player's box touching triggers (milestone 4: its touches come to
  these same uses), a use of a door while it moves (the original turns it back), a
  locked door's unlocking, the targets a door uses as it moves (11 of the 101), and
  a runner used again ("count", "wait").

## Sky portals, 2026-10-19

hevil_ship's sky, 23 levels brighter in the reference, was not the lightning: the
sky there is darker in places too, and t3_hevil has a `misc_skyportal`. On a map
with one, the original draws the scene from the portal first, with the view's
angles and field of view (`CG_DrawSkyBoxPortal`, `RDF_SKYBOXPORTAL`), then clears
the depth and draws the view, whose own sky surfaces draw nothing (`RB_EndSurface`,
`RB_StageIteratorSky`). Sky portal rooms are small boxes: t3_hevil's holds its sky
box, two `cloudlayer_test` cloud patches 40 units above the eye and gradient
walls; t1_danger's, its sky box and a sand patch shaped like distant dunes. 12
retail maps have one (5 single-player views' maps among them: kor2, t1_danger,
vjun2, yavin1, t3_hevil; and mp/duel9). Multiplayer's portal does the same; no
retail map uses its extras (`misc_skyportal_orient`, "onlyfoghere", "fov").

This renderer now does the same: `swjkja_world::sky_portal` finds the last
`misc_skyportal` spawned, and `WorldRenderer::draw` draws its scene (the world's
surfaces and skies, from a second copy of the per-frame data), clears the depth,
then the view without its skies.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| hevil_ship | 0.00 (0.55) | 99.6% (97.5%) |
| danger_rock | 0.01 (5.19) | 98.4% (94.7%) |
| kor2_troopers | 0.00 (0.01) | 99.5% (99.5%) |

- danger_rock's two distant peaks (5.19, "as on main" since it was added) were the
  portal's dunes, which the view's sky box drew over.
- kor2_troopers changed in 222 pixels of sky; the 46 other views render byte for
  byte as on main (their maps have no portal, or the view shows no sky).
- `a_sky_portal_draws_the_sky_from_where_it_stands` (`crates/render/tests/retail.rs`)
  checks a patch of hevil_ship's sky against the reference's mean colour, within
  0.01 (24-31 levels darker without the portal).
- `bench_world` (600 frames, alternating with main, three runs each): hevil_ship
  9.26-9.35 ms against main's 6.07-6.27, GPU 7.22-7.24 against 4.46-4.52;
  danger_rock 4.48-4.60 against 3.79-3.88, GPU 3.30-3.36 against 2.71-2.73;
  ffa3_spawn1 (no portal) 4.96-5.05 against 4.87-5.09. The portal's scene is 15
  surfaces on t3_hevil and 7 on t1_danger, but fills the screen: the cloud patches'
  two blended stages each, over the sky box. The original draws the same.
- Left out: the entities the original flags as in the portal at 1050 ms
  (`G_PortalifyEntities`: in its PVS with a clear line to it), which the server
  always sends and the portal's scene draws, and the effects that play there. Of the
  six maps above, only yavin1's portal room holds one (a `misc_model_breakable`,
  `yportal.md3`, 60 units from the portal; the others have no drawn entity within
  1,000 units, counted with Python from the entity text); yavin1_students shows no
  sky.

## Scaled models' specular, and a static model culled from solid, 2026-10-19

vjun2_officer's 0.07 was mostly the officer's specular: his armour and gun
(`models/players/stormtrooper/*`, `golan_arms`) were up to 60 levels brighter on
their highlights, the rest of him exact. stofficeralt.npc scales him 105, and
`RB_CalcSpecularAlpha` reflects in the model's space: a unit normal about the light
turned into the scaled axes, so a model scaled by s reflects s times the unit
reflection. This renderer reflected the world normal, which carries the scale, about
the unit light: 2s²(n·l)n - l, more than s(2(n·l)n - l), and the fourth power makes
it larger still. The specular now divides the normal's length out and scales the
light by it (`shaders/world.wgsl`). Diffuse light needs nothing: n·l is the same in
either space. A smaller directed light and a lower light point were tried first;
each left the body's bias at zero but made fewer pixels exact.
`scaled_models_reflect_their_light_as_the_original` (`crates/render/tests/retail.rs`)
checks the officer's mean colour: the reference's to 0.03, 0.66 brighter without the
fix.

ffa5_lamps' 0.09 was two things above the screen's top edge. A `lamp_rotated.md3`
at the top left, drawn here, is left out by the original: its cull point is in a
wall (cluster -1), and multiplayer's `R_inPVS` does not check for that, reading bit
7 of the byte before the view cluster's row, the previous row's padding past the
last cluster (clear). `Visibility::r_in_pvs` (`crates/bsp`) reads the same bit, and
multiplayer's static models cull by it.
`ffa5_static_model_culled_from_solid_reads_the_pvs_padding`
(`crates/render/tests/models.rs`) checks both lamps. The other, at the top right, is
door `*12` in a far doorway: the reference shows the room behind it. The door was
not open (an open door's 24-unit lip would show; none does): the server never sent
it. Multiplayer captures pin only the view (`cg_swjkjaView`); the snapshot is built
from where the capture's player spawned, and two of ffa5's spawn points (one is
ffa5_pipes' eye, 2216 -1080 664) see door `*13` but not `*12`. Rendered without
`*12`, the doorway matches to 0.0003.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| vjun2_officer | 0.04 (0.07) | 95.5% (95.1%) |
| ffa5_lamps | 0.07 (0.09) | 98.0% (97.9%) |

- Five views change: these two, and dpred_troopers and kor2_troopers by 29 and 10
  pixels (normals not quite unit length), level. The 42 others render pixel for
  pixel as on main. All 47: median 0.01, 43 under 0.5, 45 under 1.0, 21 at 0.00.
- What is left on ffa5_lamps is the door (0.06 of its 0.07); the rest is 0.009.
- What is left on vjun2_officer: its ammo rack's items, turned at random by the
  original (as dpred_racks'), half of it; and pixels one level brighter here, never
  darker, each channel on its own, over some faces of the door frame on the left
  (`cell_door_frame.md3`, scaled 1 2 1.34, `rgbGen lightingDiffuse`), a third of it.
  Its light point is the original's; not found yet. What is known: the pixels are
  scattered, as a vertex colour one level higher in one channel makes them (about
  t/255 of a texel t), so the frame's per-vertex light is higher, not its texture
  (`R_MipMap`, normal decoding, `RB_CalcDiffuseColor` and the grid's trilerp were
  checked line by line). Its four lower grid points are dark (style 0, no light),
  so it is lit by the upper four alone (ambient 63.43 59.38 54.91, directed 65.74
  58.13 48.83). The grid's ambient (before the +32) 1% lower, directed unchanged,
  takes the frame's region (x < 300) from 0.061 to 0.009; per channel the original's
  ambient is 63.06-63.23 red and 54.64-54.68 blue. Green has no single value: two
  sets of faces want it at most 59.18 and above 59.23, so their incoming light also
  differs, by about 0.1%. Moving the light point (x 0.5-2, y ±1, z 0 to -1) or
  scaling the directed light fitted worse. No mechanism found yet for an ambient
  1% lower with the directed light the same.
- Left out: a non-uniformly scaled model's specular uses the normal's length, not
  each axis's scale; and the original's viewer for a scaled model points from the
  unscaled vertex (origin plus the turned model-space point), not the drawn one.
  Both done for MD3 models, 2026-10-21 ("Scaled MD3 models' specular eye" below).

## Vertex-lit surfaces through their light style, 2026-10-18

The next five views' 0.10-0.14 (hoth2_fog, rancor_gates, rancor_drum, stamp_crates,
kor2_troopers) were the same thing: pixels one level brighter than the reference,
never darker, each channel on its own, over whole surfaces (hoth2's
`textures/hoth/h_outground_2_clip`, t3_stamp's crates in `textures/factory/s_trim2`,
t2_rancor's `halfwall_top` and `pillar_front`). Every such surface was vertex-lit
(`LIGHTMAP_BY_VERTEX`); the lightmapped surfaces beside them matched. The original
builds a vertex-lit surface's colours in `ComputeFinalVertexColor`
(`tr_surface.cpp`): each style's colour times the vertex's, summed and shifted down
by 8 bits, so a style at full (255) takes one level off every vertex. This renderer
drew the stored colours. The vertex shader now applies the first style
(`shaders/world.wgsl`, `Stage.style.zw`). Truncating the shaded colour to 8 bits
instead fitted hoth2 and t3_stamp but overshot t2_rancor; anisotropy and LOD bias
were ruled out first. `vertex_lit_surfaces_take_their_style_as_the_original`
(`crates/render/tests/retail.rs`) checks the crates' mean colour: the reference's
to 0.001, 0.39 brighter without the shift.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| hoth2_fog | 0.00 (0.13) | 99.8% (76.7%) |
| rancor_gates | 0.04 (0.14) | 97.5% (77.5%) |
| rancor_drum | 0.01 (0.11) | 97.8% (78.2%) |
| stamp_crates | 0.00 (0.10) | 99.5% (79.3%) |
| kor2_troopers | 0.01 (0.10) | 99.5% (83.7%) |
| rancor_plate | 0.01 (0.08) | 97.6% (83.9%) |
| hevil_wall_fan5 | 0.02 (0.07) | 98.1% (86.1%) |
| bounty_bomb | 0.03 (0.06) | 99.1% (90.8%) |
| hevil_wall_fan | 0.00 (0.05) | 99.6% (87.3%) |

- 24 views change, every one closer or level (ffa5_pipes, ffa5_lamps and
  dpred_canopy by a few hundred pixels); the 23 others render pixel for pixel as on
  main. All 47: median 0.01 (0.03), 43 under 0.5, 45 under 1.0, 21 at 0.00 (16).
- Left out: a vertex-lit surface's second to fourth styles (25-50 surfaces each on
  kor1, kor2 and t3_stamp), which the original adds; they are still not drawn.
- What is left on rancor_gates is its known spots (a thin line, an orange speck, a
  blue patch), now most of its difference.

## GL fog's distance per vertex, and model cull points on a plane, 2026-10-18

ctf4_up's 0.19 was the global fog on one surface: the walkway overhead
(`textures/rooftop/window_building_trim`) was fogged more in the reference than its
depth gives, by up to 4% near the eye and not at all 1,800 units away; every other
surface in the view matched exactly. The original draws the global fog with GL
fog (`GL_EXP2`, `r_drawfog 2`), and the reference PC's driver takes the fog
distance per vertex as the eye-plane distance's absolute value and interpolates
it. The walkway's long triangles reach behind the eye, where that value grows
again, so near the eye they are foggier than their depth. This renderer
interpolated the signed depth, exact everywhere. It now passes `abs(w)` per
vertex (`shaders/world.wgsl`). Fog from the radial distance, per-vertex fog
factors and screen-linear depth were tried first; each made the fog views worse.
`global_fog_takes_the_distance_per_vertex_as_the_original`
(`crates/render/tests/retail.rs`) checks four boxes on the walkway's panels.

rancor_plate's 0.16 was one water pump (`models/map_objects/rocky_ruins/water_pump.md3`)
left out: its cull point (origin raised 1 unit) lies on a plane of the BSP whose
back is solid. The models' and entities' PVS tests (`R_inPVS`, `SV_inPVS`, the
server's snapshots) find the leaf with `CM_PointLeafnum`, which sends a point on a
plane to its front; `WorldVis::cluster` used `R_PointInLeaf`'s rule (to the back),
which only the world's leaf marking uses. `a_cull_point_on_a_plane_goes_to_its_front`
(`crates/render/tests/models.rs`) checks the pump.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| ctf4_up | 0.01 (0.19) | 98.9% (85.2%) |
| rancor_plate | 0.08 (0.16) | 83.9% (83.7%) |
| ctf4_spawn1 | 0.00 (0.04) | 99.9% (91.3%) |
| ctf4_spawn2 | 0.00 (0.01) | 99.7% (97.0%) |

- Five views change (ffa2_fog_above too, 282 pixels), all closer; the 42 others
  render pixel for pixel as on main. All 47: median 0.03 (0.04), 43 under 0.5, 45
  under 1.0, 16 at 0.00 (13). The multiplayer world views are now 0.00-0.03.
- hevil_fan_low's 0.16 is rain over the fan (fx_rain, not drawn), as on
  hevil_fan_west: the reference's streaks are the bright pixels ours lacks.
- What is left on rancor_plate is pixels one level off over the whole frame
  (16% of them), as before; the pump's own pixels differ by 0.1 on average.

## Blending in 10 bits, as the reference PC's framebuffer, 2026-10-18

The academy classes' 0.25-0.33 was not the NPCs: 75-99% of each view's difference
was pixels one level off, half the frame, in stripes of +1 beside -1 along the
light shafts (`textures/common/dark_dust`, a gradient added at 35/255, several
layers deep). Blurred, the error vanished (no shift, no scale): both engines drew
the same values and rounded them differently. Where the reference crosses a level,
its pixels flip between the two at random with the wall's texture under them, as
if the wall had not been rounded to 8 bits before the dust was added.

It had not: on the reference PC the original blends in 10 bits a channel. OpenJK
asks for and reports 24-bit colour (`PIXELFORMAT: color(24-bits)`), but the Intel
HD 630's OpenGL driver gives it a 10:10:10:2 surface, and screenshots round that to
8 bits. This renderer's target is now `A2B10G10R10_UNORM_PACK32`, read back rounded
to 8 bits (`Target::read_rgba`); windows draw straight into a 10-bit swapchain
image when the surface offers one, else the target is copied to an 8-bit one.
`blending_keeps_ten_bits_like_the_original_framebuffer` (`crates/render/tests/bringup.rs`)
adds a quarter of a level eight times: 2 in 10 bits, 0 in 8.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| academy6_class | 0.09 (0.33) | 96.4% (50.0%) |
| academy4_class | 0.07 (0.31) | 96.8% (50.6%) |
| academy2_class | 0.04 (0.28) | 97.7% (50.8%) |
| academy1_class | 0.01 (0.25) | 98.5% (51.0%) |
| ffa3_spawn1 | 0.00 (0.21) | 99.2% (55.3%) |
| duel9_fog_in | 0.00 (0.08) | 99.7% (84.3%) |
| yavin1_students | 0.05 (0.13) | 92.2% (72.6%) |

- Every one of the 47 views comes closer; 13 are at 0.00, among them every
  multiplayer fog view. All 47: median 0.04 (0.14), 42 under 0.5, 45 under 1.0.
- A 16-bit target does worse on every view (academy1_class 0.08, ffa3_spawn1
  0.09): the reference rounds at 10 bits, not merely above 8.
- It costs GPU time on the HD 630: the ffa3 camera path offscreen took 3.02-3.06
  ms of GPU a frame against 2.71-2.74 ms in 8 bits (two alternating rounds of two
  runs, a busy PC: about 300 FPS against 335). The FPS tables above were measured
  in 8 bits. The owner chose 10 bits everywhere, so the viewer runs what the views
  verify.
- Not measured: whether this machine's Vulkan surface offers a 10-bit format (a
  window was not opened), and so whether windows draw direct or copy.
- What is left on the academy classes is the NPCs (academy2/4/6's students'
  `BOTH_CIN_9` starts, from client frame jitter).

## JPEG decoded by libjpeg 9a, 2026-10-18

ctf1_fog_above's 0.40 was texture grain: no pixel off by more than 8, half of them
off by one, in the shape of the walls' grainy texture and strongest where the walls
are near. The reference decodes JPEGs with the libjpeg OpenJK bundles and builds on
Windows, 9a, which scales 4:2:0 chroma up inside its inverse DCT (`jpeg_idct_16x16`,
picked in `jdmaster.c`) rather than with libjpeg 6b's triangle filter. zune-jpeg,
like libjpeg-turbo, follows 6b, so every JPEG texture differed by a level here and
there (up to 5 of 255). The 2026-10-07 decision had left it there because the
original re-compresses textures to S3TC by default, but the reference captures turn
that off (`r_ext_compress_textures 0`).

swjkja-image now decodes JPEGs with a Rust port of 9a's decoder
(`crates/image/src/jpeg.rs`: Huffman baseline and progressive scans, the integer 8x8
and 16x16 inverse DCTs, YCbCr to RGB, and 9a's handling of data that ends early).
All 4,271 retail JPEGs decode bit for bit as 9a built from OpenJK's source does
(per-file CRC-32s; `tests/retail.rs` keeps three, and `tests/decode.rs` checks
small progressive, grey, restart-marker and truncated files against 9a). All 47
views were rendered three ways (9a's own pixels swapped in, 9a's C source built in,
and the port), and the three render pixel for pixel alike.

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| ctf1_fog_above | 0.17 (0.40) | 0.0% (0.0%) |
| academy6_class | 0.33 (0.39) | 0.1% (0.1%) |
| academy4_class | 0.31 (0.37) | 0.1% (0.1%) |
| academy2_class | 0.28 (0.35) | 0.0% (0.0%) |
| academy1_class | 0.25 (0.32) | 0.0% (0.0%) |
| ffa3_spawn1 | 0.21 (0.29) | 0.0% (0.0%) |
| ffa3_spawn3 | 0.14 (0.26) | 0.1% (0.1%) |
| yavin1_students | 0.13 (0.24) | 0.0% (0.0%) |
| duel5_spawn1 | 0.07 (0.17) | 0.0% (0.0%) |

- Every view changes, and 46 of 47 come closer on their changed pixels. The one
  that does not is duel9_fog_in, by rounding: 5,282 pixels, 0.28 to 0.31. The fog
  views change least (578 to 15,730 pixels), and their means move by under 0.01.
- All 47 views: median 0.14 (0.21), 41 under 0.5, 44 under 1.0 (as on main). The
  largest unexplained views are now the academy classes (0.25-0.33).
- Loading is as fast as before: decoding every retail texture takes 34-39 s (36 s
  with zune-jpeg; 66 s with 9a's C source).
- Not supported, each reported as a decode error: arithmetic coding, 12-bit and
  lossless JPEG, sampling other than equal or half both ways (4:2:2 and the like,
  for which 9a picks a 16x8 or 8x16 inverse DCT), and files that leave out their
  Huffman tables. No retail texture is any of these.

## Curved surfaces with fewer rows at a distance, 2026-10-18

ffa3_spawn1's 0.41 was its curved surfaces: the arch's outline a pixel off along
its curve, and the round tower's upper part a field of texture-shaped specks. This
renderer drew every patch grid at full detail; the original's `RB_SurfaceGrid`
skips the rows and columns whose error (one over their distance from the coarser
curve, `R_SubdividePatchToGrid`) is above `r_lodCurveError` (250) over the grid's
distance along the view less its LOD radius (`LodErrorForVolume`), the same in both
games. The grids keep all their vertices; each view now picks the rows and columns
when it copies the visible surfaces' indexes (`a_grid_drops_its_columns_as_the_view_moves_away`
in `crates/world/tests/patches.rs`).

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| ffa3_spawn1 | 0.29 (0.41) | 0.0% (0.1%) |
| ffa3_spawn3 | 0.26 (0.34) | 0.1% (0.1%) |
| ffa3_up | 0.08 (0.18) | 0.0% (0.1%) |
| ffa3_spawn2 | 0.18 (0.20) | 0.0% (0.0%) |
| ctf4_spawn2 | 0.09 (0.12) | 0.0% (0.0%) |
| ctf4_up | 0.25 (0.26) | 0.0% (0.0%) |
| duel5_spawn2 | 0.15 (0.16) | 0.0% (0.0%) |
| kor2_troopers | 0.22 (0.23) | 0.0% (0.0%) |
| rancor_plate | 0.29 (0.30) | 0.3% (0.3%) |

- 14 views changed, every one closer to the reference on the changed pixels
  (ffa3_spawn1's 63,381: 2.19 to 0.48; duel5_spawn2's 992: 8.47 to 0.30; the hevil
  fans', rancor_gates' and ffa3_spawn2's a few thousand each, 2-10 to 0.1-0.5).
  The other 33 render pixel for pixel as on main.
- What is left on ffa3_spawn1 is rounding: 90% of the difference is pixels one
  level off.
- Grids in materials with deforms (rebuilt every frame) are still drawn at full
  detail.
- All 47 views: median 0.21, 41 under 0.5, 44 under 1.0 (as on main). The largest
  unexplained multiplayer world view is now ctf1_fog_above (0.40).

## Single player's environment mapping, 2026-10-18

vjun1_hall's 1.46 (3.0% of pixels off by more than 16) was the glass floor tiles
under the air purifiers (`textures/common/glass_security_tris`), not the purifiers'
highlights the 2026-10-10 section below suspected: the tiles add a `tcGen
environment` stage, and its gradients sat in different places, brighter on one side
of each tile in the original and on the other here. Single player's renderer
(`code/rd-vanilla` `RB_CalcEnvironmentTexCoords`) does not reflect the view about
the normal as multiplayer's does: it takes `normal.xy * d - 0.5 * viewer.xy`
(`d` the normal's dot with the direction to the eye), with no 0.5 offset. This
renderer used multiplayer's formula on every map; single-player maps now use theirs
(`single_player_environment_maps_as_the_original` in `crates/render/tests`).

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| vjun1_hall | 0.58 (1.46) | 1.0% (3.0%) |
| hevil_fan_west | 0.75 (0.99) | 0.2% (0.8%) |
| hevil_fan_low | 0.27 (0.39) | 0.1% (0.3%) |
| rancor_drum | 0.23 (0.29) | 0.0% (0.0%) |
| yavin1_students | 0.24 (0.27) | 0.0% (0.0%) |
| hevil_wall_fan | 0.23 (0.26) | 0.0% (0.0%) |
| hevil_wall_fan5 | 0.24 (0.26) | 0.0% (0.0%) |
| stamp_crates | 0.20 (0.23) | 0.0% (0.0%) |

- kor2_troopers changed in 1,540 pixels, closer to the reference (0.44 from 0.69 on
  them) but not enough to move its 0.23. The 38 other views render pixel for pixel
  as on main (the multiplayer ones keep multiplayer's formula).
- What is left on vjun1_hall is the green rain streaks (`fx_rain`, an effect this
  renderer does not draw); on hevil_fan_west, its rain.
- hoth2_fog (0.14) and vjun1_hall join the reference set, now 47 views: median 0.21
  (main 0.23), 41 under 0.5, 44 under 1.0 (main 43). Above 1 are only danger_rock
  (5.33), ffa5_pipes (3.87) and dpred_guns (1.02), all explained.

## NPCs' box 32 units wide, as single player's, 2026-10-18

vjun2_imp_stance's 0.44 was an outline around the imperial: his whole body drawn
2.5 pixels low, head to boots. The game's log has him at z 3037.13; this renderer
put him at 3036.879. He spawns half inside a trim brush whose top slopes 14 degrees
(normal 0, 0.243, 0.970), and the spawn lift (`G_CheckInSolid`) stops where his box's
corner meets that slope. The box was multiplayer's, 30 units wide (`playerMins`
-15); single player's `DEFAULT_MINS_0` is -16, so the original's corner is one unit
further up the slope: 0.25 units higher, which is exactly the log's height.

The same unit explains "an NPC that lands on a slope stops a little higher there"
(the NPC views' section below): on every slope in the logs this renderer's NPCs were
lower by the slope's tangent (dpred's stormtrooper2 by 0.12, kor2's three by
0.17-0.31), within the logs' tenth of a unit.

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| vjun2_imp_stance | 0.14 (0.44) | 0.0% (0.5%) |
| kor2_troopers | 0.23 (0.36) | 0.0% (0.2%) |

- The other 43 views render pixel for pixel as on main.
- All 45 views: median 0.23 (main 0.25), 40 under 0.5, 42 under 1.0. Single-player
  NPC views now 0.10-0.39 (the academy classes are the highest).
- What is left on vjun2_imp_stance is single scattered pixels.

## Flares hidden as the original hides them, 2026-10-18

rancor_gates' 0.69 was almost all flares that this renderer drew and the original
did not: four orange, three blue and four white ones in the room beyond the left
gate, 2000-3300 units out, and five white ones on the ceilings behind the
corridor's right wall and gate, 1000-1500 units out. dpred_racks' and dpred_guns'
"world flares the original doesn't draw" were the same. Two causes:

- **The depth test never saw any depth.** `RB_TestZFlare` is done in the vertex
  shader, which reads the depth copy from the texture array, but the array's
  binding was declared for the fragment stage only. Vulkan leaves that read
  undefined; on the Intel HD 630 it gave 0, "nothing drawn", so every flare in the
  view passed. That is why the stray flares came and went with the texture slots'
  numbering (the blob shadows' section below). A debug render had every flare
  reading 0, even those in front of the floor. With the binding visible to the
  vertex stage, the ceiling flares fall behind the wall.
- **Flares were drawn from every leaf.** The original adds a flare surface like any
  other, from the leaves the view's cluster sees (no per-surface cull,
  `R_CullSurface` skips `SF_FLARE`). This renderer drew all of a material's flares.
  The far room's flares are outside rancor_gates' PVS; now only flares of the
  visible surfaces (and of the brush entities the view draws) are built.

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| rancor_gates | 0.25 (0.69) | 0.0% (1.1%) |
| dpred_racks | 0.57 (7.03) | 1.6% (12.3%) |
| dpred_guns | 1.02 (6.44) | 3.0% (12.2%) |
| vjun2_imp_stance | 0.44 (0.50) | 0.5% (0.6%) |
| ffa5_rings | 0.19 (0.22) | 0.0% (0.1%) |

- With the PVS alone: rancor_gates 0.36 (the five ceiling flares left), dpred_racks
  0.57, dpred_guns 1.02, ffa5_rings 0.19. The depth test then hides the ceiling
  flares and a flare at the top of vjun2_imp_stance, and small ones on rancor_drum
  and rancor_plate (a few hundred pixels, the same score). Every pixel that
  changed now differs from the reference by 0.1-0.2 on average (4.5-33 on main).
  The 38 other views render pixel for pixel as on main, duel5_spawn2's flares,
  which the original draws, among them.
- What is left on rancor_gates is three spots under 0.1% of the picture.
  dpred_guns' 1.02 is the rack weapons the original turns at random.
- All 45 views: median 0.25, 40 under 0.5 (main 38), 42 under 1.0 (main 41). Above
  2 are only danger_rock (5.33) and ffa5_pipes (3.87).
- `bench_world` was not run: per frame the change adds one pass over the flares
  and a copy of the visible surfaces' flags.
- A Vulkan validation layer would have reported the binding; the Vulkan SDK is not
  installed on this machine.
- hoth2 and vjun1, which lost the Vulkan device on the Windows GPU, render again:
  hoth2_fog 0.14, vjun1_hall 1.46 (3.0% of pixels off by more than 16).

## Academy students placed as STARTINSOLID leaves them, 2026-10-18

academy6_class's 0.85 was a thin outline around every student, held poses as much
as looping ones (jedi_rm2 2.33, in a two-frame hold): each body a fraction of a
pixel off. The capture log gives the cause. It has academy6's students at z 672.0
and this renderer had them at 672.125. academy1's and academy2's are at 672.1 in
the log, and four of academy4's at 672.0 (four of its five worst students). Every
spawner at 672.0 has spawnflag 128 (STARTINSOLID: 224-228), and none of those at
672.1 has it. `NPC_Begin` runs `G_CheckInSolid` with fix only without that flag. Its
trace from the origin down to the box's bottom hits the floor the spawner stands on
and puts the NPC on the trace's end, 0.125 (`SURFACE_CLIP_EPSILON`) higher. With the
flag, nothing moves the NPC, and the first movement frames leave it there.
`npc::settle` now skips the lift for those spawners.

The log also has every animation set in the frame after the player begins (2050)
at 2050: academy1 (2), academy4 (6), academy6 (8) and t2_dpred (7). Here they started
a frame earlier, at 2000. `PM_SetAnimFinal` takes `cg.time` once the client has
drawn, and the level's time before that. The client's clock is still at 0 until it
draws its first frame, which comes after that server frame, as on the first frame
(1550). Animations set in frames up to 2050 now start at the level's time.

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| academy6_class | 0.39 (0.85) | 0.1% (0.8%) |
| academy4_class | 0.37 (0.48) | 0.1% (0.2%) |
| yavin1_students | 0.27 (0.40) | 0.0% (0.2%) |

- With the placement alone: academy6 0.39, academy4 0.38, yavin1_students 0.27.
  The clock gives academy4's last 0.01 (kyle and the player, in `BOTH_CIN_*` loops
  from the intro). The 42 other views render pixel for pixel as on main.
- All 45 views: median 0.26, 38 under 0.5 (main 37), 41 under 1.0.
- What is left on academy6 is mostly jedi_kdm2 (1.44) and jeditrainer (1.01). The
  capture starts their `BOTH_CIN_9` at 12137 on the client's clock (academy4's at
  12141, academy2's at 12138), and this renderer starts it at 12150, a frame behind
  the server's 12200. The original's lag depends on its frame timing. Rendered 13 ms
  later, so their animation lines up, they fall to 0.30 and 0.25.
- `bench_world` was not run: the change moves NPCs once, as they spawn, and sets
  different start times, with nothing added per frame.

## ffa3_spawn3's lift: missing from the world-only reference, 2026-10-17

The lift that #69 found standing lower in the original than here is `*4` (lift2:
`func_door`, start open, angle -2, lip 42, sunk to a top at z -55 in its pit), not
`*5`-`*8`, which are 3400 units to the left of the view. Its place is the original's:
`SP_func_door` and `InitMover` (codemp) give the same spawn position, over the same
bounds spread by a unit. The server would also send it to every view in ffa3. All
spawn points, the view and the lift's box are in area 0. The box touches 17
clusters, so the overflow slip sends it to any view that can't see cluster 85. It is
1300-3500 units inside the default `distanceCull` of 6000 (ffa3 sets none).

The difference is the reference: ffa3_spawn3 is one of the 19 world views, captured
with `r_drawentities 0` (see "Static models compared with the reference"), so the
original draws no lift there. On the 1556 pixels the lift covers here, the reference
differs by 0.07 from this renderer with the lift left out, and by 14.3 with it drawn.
With the lift lowered for a test, the difference falls with every 2 units (1.79 at 2
lower, 0.45 at 10) and reaches the left-out figure at 16 lower, when the pit's rim
hides the lift. That is why #69's 2 units read as "lower still". Leaving every brush
entity out of the 19 world views changes ffa3_spawn3 alone (0.34 -> 0.32 mean, 0.13%
-> 0.05% off by > 16); the other 18 render pixel for pixel the same. Nothing changes
in the renderer: its world views keep drawing entities, and this view keeps the lift
as a known difference.

## dpred's grass: surface sprites, 2026-10-17

`textures/imperial/dp_ground`'s second stage is `surfaceSprites flattened 48 24 45
400` (fade to 3000, variance 1 2, wind 0.8): the original skips it as a stage and
scatters grass over the ground's upward-facing triangles each frame
(`tr_surfacesprites.cpp`). This renderer now does the same for vertical and
flattened sprites (`crates/render/src/sprites.rs`): each triangle's random numbers
start from its corners (`randomchart`, seed and interval from coordinate sums), its
step from its area (`Q_rsqrt`, the one-step approximation), each sprite faded by
its distance and a drawn fade start, sized by a drawn variance, swaying idly at the
view's time, grey from its vertices' blue. A separate Python port of
`RB_DrawVerticalSurfaceSprites` gives the same 9 sprites for a 200-unit triangle.
All 405 dp_ground surfaces are planar faces of the world model, so the triangles
are the original's exactly.

Two things only the captures showed:

- **The old capture had no lean.** A sprite's top left corner leans a fifth of its
  width away from the view (`ssfwdvector`). With the time pinned, the original's
  sprite update runs only when the time changes; the capture holds the game's time
  before the grass first shows, so the update ran once, while the view vector was
  still zero, and never again. The reference patch now runs it every frame while
  `cg_swjkjaTime` is set (a debugging build logged forward (-1, 0, 0) and origin
  (-900, -4300, 90), the view's), and dpred_troopers was captured again (0.36
  from the old capture, the near grass leaning).
- **The quad's split.** The lean makes the quad not flat, and `GL_QUADS` leaves
  the split to the driver: the reference GPU's splits along the top right to
  bottom left diagonal. Split along the other one, the near grass moves visibly.

Against the new capture (main 0.86):

| dpred_troopers | Mean difference | Pixels off by > 16 | Near grass (480x220) | Field (460x160) |
|------|------|------|------|------|
| main (no grass) | 0.86 | 1.4% | 3.84 | 2.98 |
| no lean | 0.54 | 0.8% | 1.13 | 2.65 |
| lean, split top left to bottom right | 1.02 | 1.8% | 4.50 | 3.51 |
| **lean, split top right to bottom left** | **0.33** | **0.5%** | **0.13** | **2.54** |

- The view's time fits: rendered at 16 times across a sway period (4189 ms), the
  near grass is closest at 12345 ms. The grass's colour matches (mean of its
  pixels within 1%).
- What is left is mostly the field on the left, 400-3000 units away, where
  sprites fade out; not explained yet.
- The 44 other reference views render pixel for pixel as on main. All 45: median
  0.26, 37 under 0.5 (main 36), 41 under 1.0.
- `bench_world` (300 frames, alternating with main, three runs each, the view on an
  idle PC): frame 6.37-6.65 ms against main's 5.75-5.88, GPU 4.79-4.91 ms against
  4.38-4.41. The view draws 518 sprites over 71 visible grass surfaces (at most
  10,799), scattered in about 0.09 ms on the CPU; the rest is the GPU drawing
  large alpha-tested quads near the eye. Later runs on a busy PC were noisier
  (main 6.5-13.2 ms), the GPU's difference 0.3-1.1 ms.
- Left out: oriented and effect sprites (28 of the 70 retail lines), wind (no
  weather system or wind cvars on the maps viewed), wind points, fog passes over
  sprites in fog volumes, and sprites on brush entities, models and deformed
  surfaces. The far field was the range scale (next section).

## dpred's far grass: the range scale of the intro's camera, 2026-10-17

The field left of dpred_troopers' path, 400-3000 units out, had less grass in this
renderer than in the original, and grass of other heights. Scaling both fade
distances by one factor fits it to the pixel at 1.19: 0.26 at 1.1, **0.15 at
1.1918**, 0.23 at 1.28, 0.29 at 1.4. That is tan 45° / tan 40°.
`R_SurfaceSpriteFrameUpdate` takes the first field of view that sprites are drawn
with (between 50 and 135 degrees) as the standard one, and lengthens the shader's
ranges by `tan(standard / 2) / tan(fov / 2)` for any other field of view. With
`SCRIPTS=1`, t2_dpred's intro runs as a cutscene. Its first command enables the
camera (`camera` type 62, `TYPE_ENABLE`), which draws the view at
`CAMERA_DEFAULT_FOV`, 90 degrees, until a later zoom to 80 (its block 112). So the
grass's first frame was at 90, and the pinned view at `cg_fov` 80 reaches 1.19
times further: 476 units to the fade's start, 3575 to its end.

The renderer now keeps that standard (`sprites::range_scale`): the first field of
view that draws sprites, or 90 when the map's level scripts enabled a camera
before the view's time (`script::Timelines::camera`; the camera is taken to have
drawn sprites first, as t2_dpred's did).

| dpred_troopers | Mean difference | Pixels off by > 16 |
|------|------|------|
| main (range scale 1) | 0.33 | 0.5% |
| **range scale 1.1918** | **0.15** | **0.0%** |

- The 44 other reference views render pixel for pixel as on main. All 45: median
  0.26, 37 under 0.5, 41 under 1.0 (as on main).
- Left out: the camera's own frames (cameras are not modelled): a camera that
  zooms before any sprites show, or one that never sees sprites, would set
  another standard.

## Brush entities' bounds spread as the collision model's, 2026-10-17

yavin1_students' last difference at its open aisle door was a column 1-2 pixels
wide down the doorway's right edge: the open door's dark leading face here, the
wall's grey in the original. With the open door moved for a test 1 unit further
along its way, the column matched the original's (49 of 255 against 49), and 1 unit
back made it wider: the original's door travels further. `CMod_LoadSubmodels`
spreads every inline model's bounds by a unit each way ("spread the mins / maxs by
a pixel", in both executables), and `SetBrushModel` gives the game those, so a
door's size along its way is 2 units more than the BSP's bounds say, and it opens 2
units further. This renderer read the BSP's bounds; it now spreads them, which also
moves what reads them: a `func_plat` without a "height" starts 2 units lower, the
boxes entities link (and the server sends them by) are a unit larger each way, and
so are the door triggers spawned around them.

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| yavin1_students | 0.40 (0.41) | 0.2% (0.2%) |
| ffa3_spawn3 | 0.34 (0.35) | 0.1% (0.1%) |

- At yavin1's doorway the 163 pixels that changed now differ by 0.1 on average
  (37.4 before).
- ffa3_spawn3's lift now stands 2 units lower; the changed pixels differ by 12.1
  instead of 16.1. The reference has no lift at all (a world-only capture): see
  "ffa3_spawn3's lift: missing from the world-only reference".
- The 43 other reference views render pixel for pixel as on main.
- `bench_world` was not run: the change moves entities by 2 units and adds nothing
  to draw.

What is left of yavin1_students is the NPCs' outlines. dpred_troopers' (0.85) is its
grass: `textures/imperial/dp_ground`'s `surfaceSprites` stage, which the original
scatters over the ground and sways in the wind (`tr_surfacesprites.cpp`); this
renderer parses those stages and draws none of them yet.

## Single-player models lit with single player's ambient scale, 2026-10-17

Every model in a single-player view stood a little brighter than the original's:
dpred_canopy's trees by about 8% all over, bounty_bomb's weapon caches by 12%
(ours/original 0.877 in every brightness band, so a scale, not an offset). The
caches' light grid (sampled independently in Python: styles 0 and 44 lit, 32
switched off) is almost all ambient, and the light grid's ambient is scaled by
`r_ambientScale`, whose default differs between the two executables: 0.6 in
multiplayer's renderer (`codemp/rd-vanilla/tr_init.cpp`), 0.5 in single player's
(`code/rd-vanilla/tr_init.cpp`). Everything else in `R_SetupEntityLighting` and
`RB_CalcDiffuseColor` is the same in both. This renderer used 0.6 everywhere; the
light grid now takes the map's rules (`LightGrid::with_rules`).

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| dpred_canopy | 0.16 (1.34) | 0.0% (0.0%) |
| bounty_bomb | 0.27 (1.17) | 0.0% (0.5%) |
| yavin1_students | 0.41 (0.94) | 0.2% (0.3%) |
| dpred_troopers | 0.85 (1.02) | 1.4% (1.4%) |
| vjun2_imp_stance | 0.50 (0.82) | 0.6% (0.7%) |
| vjun2_officer | 0.17 (0.49) | 0.1% (0.1%) |
| rancor_gates | 0.69 (0.94) | 1.1% (1.1%) |
| rancor_plate | 0.30 (0.47) | 0.3% (0.3%) |
| rancor_drum | 0.29 (0.44) | 0.0% (0.0%) |
| academy1_class | 0.32 (0.36) | 0.0% (0.0%) |
| academy2_class | 0.35 (0.45) | 0.0% (0.0%) |
| academy4_class | 0.48 (0.56) | 0.2% (0.2%) |
| academy6_class | 0.85 (0.90) | 0.8% (0.8%) |
| danger_rock | 5.33 (5.52) | 3.7% (3.7%) |
| dpred_guns | 6.44 (6.72) | 12.2% (12.3%) |
| dpred_racks | 7.03 (7.14) | 12.3% (12.3%) |
| kor2_troopers | 0.36 (0.38) | 0.2% (0.2%) |
| stamp_barrels | 0.10 (0.13) | 0.0% (0.0%) |
| stamp_crates | 0.23 (0.25) | 0.0% (0.0%) |
| hevil_wall_fan | 0.26 (0.27) | 0.0% (0.0%) |
| hevil_wall_fan5 | 0.26 (0.28) | 0.0% (0.0%) |

- Every single-player view with a model in it moved closer; none moved away. The
  24 others (the 22 multiplayer views, and hevil_fan_low and hevil_fan_west, whose
  fans are brush entities lit by lightmaps) render pixel for pixel as on main.
- All 45 views: median 0.26 (main 0.35), 41 under 1.0 (main 38).
- dpred_troopers' 0.85 is now nearly all grass, the foreground's and the field's on
  the left, which the original draws (and sways) and this renderer does not.
- `bench_world` was not run: the change is one constant per map, read once per
  entity per frame as before.

## Single-player NPC views captured with scripts running, 2026-10-17

The four views of NPCs that spawn with their maps (dpred_troopers, vjun2_officer,
kor2_troopers, stamp_barrels) were captured with every NPC frozen from the first
frame and no scripts (`d_npcfreeze 1`), while the new renderer runs the maps'
scripts. They are now captured as the script views are (`SCRIPTS=1 ENTITIES=1
bench/capture-reference.sh reference/npc-views.txt`: the AI alone frozen, script
randoms pinned, the game stopped at 12345 ms) and moved from `model-views.txt` to
`bench/reference/npc-views.txt`. Main against both captures:

| View | Mean difference | Before (frozen capture) | Old capture against new |
|------|------|------|------|
| dpred_troopers | 1.02 | 1.02 | 0.36 |
| vjun2_officer | 0.49 | 0.52 | 0.06 |
| kor2_troopers | 0.38 | 0.39 | 0.05 |
| stamp_barrels | 0.13 | 0.32 | 0.20 |

No NPC in these views stands differently with scripts running: the logs give
the same idles as before (starting at 4987-4989 ms on the client's clock), and the
NPCs' pixels match between the two captures. What changed is elsewhere in the
shot: dpred's foreground grass, which the original sways and this renderer
does not draw, and a door at stamp_barrels' right edge, open in the last frozen
capture (whose game was not stopped at the view's time) and closed in the new one, as
here.

## yavin1's aisle door opened by its students, 2026-10-17

yavin1_students' largest remaining difference was its aisle door: closed here, open
in the original. yavin1 has two unnamed `func_door`s across the aisle (models `*3`
and, behind it, `*2`), each spawning a trigger around itself that reaches 120 units
further along its thinnest axis (`Think_SpawnNewDoorTrigger`). Two seated
students (`jedi_student02` and `03`) stand in `*3`'s, and NPCs touch triggers
from their first think (`ClientThink` runs with the AI frozen too), so `*3` opens 50
ms later and slides its 72 units in 180 ms (`TR_NONLINEAR_STOP`), and every touch
after that puts off its closing. No NPC stands in `*2`'s; it stays shut, as in the
reference. Doors NPCs stand in now open as the map begins:

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| yavin1_students | 0.94 (1.19) | 0.3% (1.2%) |

- What is left at the door is a column 1-2 pixels wide down the doorway's right
  edge, where this renderer draws the open door's side face dark (about 11 of 255)
  and the original shows the wall's grey (about 45). (Explained and fixed later the
  same day: the collision model's spread bounds, above.)
- Across the 34 single-player maps, 280 of the 458 doors open by touch (counted
  independently from the entity text with Python), and NPCs stand in four of them
  as the maps begin: yavin1's `*3`, t1_danger's `*67`, t1_sour's `*74` and
  t2_wedge's `*156`. No other reference view shows one.
- The 44 other reference views render pixel for pixel as on main.
- `bench_world` (300 frames, alternating with main, three runs each, the yavin1
  view): frame 10.14-10.23 ms against main's 10.12-10.32, GPU 8.49-8.58 ms against
  8.42-8.60.

## yavin1's intro and the saber on its seat, 2026-10-17

A new view, `yavin1_students` in `bench/reference/script-views.txt`, looks down
the shuttle's aisle at the seated students at 12345 ms, captured with `SCRIPTS=1`
(the intro's camera running: the forced view is drawn over it). Two things differed:

- **Rosh stood behind the students** instead of sitting in the front seat, hands
  behind his head. yavin1's opening script (`yavin1/opening`, which its player
  start uses) plays a video and then `run`s `yavin1/intro`, which this renderer
  left out; the intro gives Rosh `BOTH_CIN_11` (its root motion carries him into
  the seat) when the pilot's block signals, about 7.7 s in (the capture logs 7693
  ms). `run` now runs the named script in place (`CSequencer::CheckRun`), in the
  same update.
- **Five students wore other colours.** The original draws one more random number
  before the NPCs spawn: the player's saber lying on a seat (a `weapon_saber`,
  saberType "player") reads its block as it finishes spawning (1300 ms,
  `FinishSpawningItem`), drawing its random colour, so every NPC's
  `customRGBA` and saber draw came one later in the original. The capture's draw
  log (`g_swjkjaRand`) and the new renderer's now match draw for draw.

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| yavin1_students | 1.19 (2.09) | 1.2% (3.9%) |

- What is left is the far door: the original's stands open (an unnamed `func_door`
  opens when something touches its trigger; two students sit in it), this
  renderer's closed (movers' motion is left out).
- kor1's statue views (`kor1_statue`, `kor1_statue2`) were captured at 12345 and
  at 60000 ms; neither shows the func_statics there (the three statues, the coffin
  slab). kor1's encounter keeps the intro's camera on until `npc_fakePlayer1`
  reaches a navgoal, which it never does with the AI frozen, and while a camera
  runs the server sends the entities visible from the camera, not from the player
  (`SV_BuildClientSnapshot`), a room above. They are not reference views.
- The 40 other reference views render pixel for pixel as on main.

## Single-player NPCs drawn with more light, 2026-10-17

The academies' students stood darker than the original's, every one of them, by
1.15-1.76 times, the most where they stood darkest: the shape of light added to the
ambient, not of a tint. Their spawnscripts (academy2's `studentspawn` to
`studentspawn5`, which academy4 and 6 run too) and the intros' teachers start with
`SET_MORELIGHT true`; cgame then draws the NPC with `RF_MORELIGHT`
(`SCF_MORELIGHT`, read as it draws), and `R_SetupEntityLighting` adds 96 to its
ambient light instead of 32. academy1's seated students (`academy1/studentspawn`)
do not set it; its intro's teachers do. Rendered against the same captures:

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| academy1_class | 0.36 (0.46) | 0.0% (0.3%) |
| academy2_class | 0.45 (1.27) | 0.0% (2.8%) |
| academy4_class | 0.56 (1.39) | 0.2% (3.0%) |
| academy6_class | 0.90 (1.99) | 0.8% (4.9%) |

- What is left on academy6 is at the students' edges (poses), not their shading: the
  three groups of pixels still off by more than 12 are within 0.91-1.04 of the
  original's brightness.
- The 40 other reference views render pixel for pixel as on main.
- `bench_world` (300 frames, alternating with main, three runs each, the four
  academy views): within main's spread (academy2 7.31-7.73 ms against 7.30-7.45,
  academy6 8.35-8.36 against 7.15-8.48, academy1 7.10-8.37 against 7.33-8.37).

## Single-player script waits pinned in the captures, 2026-10-16

A script's random numbers (`wait( random( a, b ) )`, random loop counts) come from
`CQuake3GameInterface::Random`, which calls the C library's `rand()`, seeded from
the clock in `InitGame` (`srand( randomSeed )`, `com_frameTime`): every run of the
original waits differently, so three of academy2's students (`studentSpawn5`, also
run on academy4 and 6) stood in different animations from capture to capture. The
capture patch now holds them at their range's low end (`g_swjkjaScriptRand`, set
with `SCRIPTS=1`), as the new renderer takes them. academy2, 4 and 6 captured again,
twice; renders unchanged (the change is the reference's):

| View | Before (mean, > 16 off) | Pinned, run 1 | Pinned, run 2 |
|------|------|------|------|
| academy2_class | 1.61, 3.3% | 1.27, 2.8% | 1.27, 2.8% |
| academy4_class | 2.23, 4.4% | 1.40, 3.0% | 1.39, 3.0% |
| academy6_class | 2.45, 5.5% | 1.98, 4.9% | 1.99, 4.9% |

- The two pinned runs log the same animations for every NPC; their start times differ
  by 2 to 13 ms (the client's clock, a frame either way), and their screenshots by
  0.02-0.03 mean (0.02-0.03% of pixels by more than 8).
- What is left on the classroom views is mostly the students' shading (several drawn
  darker or tinted differently), not their poses: their scripts' `SET_MORELIGHT`
  (see "Single-player NPCs drawn with more light").
- The other single-player views were not captured again (unchecked whether any of
  their scripts draws a random number before its time).

## Single-player NPCs' spines turned against their "Motion" bone, 2026-10-16

cgame turns the lumbar and thoracic bones of an NPC whose legs and torso play
different animations back against the angles of its "Motion" bone
(`cg_motionBoneComp` 2, `CG_G2ClientSpineAngles`). A new view,
`vjun2_imp_stance` in `bench/reference/script-views.txt`, looks at vjun2's imperial
at 3000 ms, captured with `SCRIPTS=1` so the game stops before its idle (from 4990
ms, on both parts). He stands with his legs in `BOTH_STAND3` and his torso in
`TORSO_WEAPONIDLE2`, and main drew him twisted about 30 degrees, pistol to the side:

| | Mean difference | Pixels off by > 16 | His box (x380-880 y150-720) | Box pixels off by > 32 |
|------|------|------|------|------|
| main | 1.86 | 3.6% | 4.88 | 6.3% |
| spine turned | 0.82 | 0.7% | 1.51 | 0.7% |

- Which pairs turn: on the humanoid skeleton "Motion" hangs off the pelvis, so its
  angles are the legs' pelvis with the torso's own "Motion" frames. A part's
  animation over its own legs leaves it within 0.06 degrees for every standing
  animation a retail script splits (`TORSO_RAISEWEAP4`, `BOTH_CONSOLE1`,
  `BOTH_TALKCOMM1`, ...), and the rifle stance (`BOTH_STAND3` with
  `TORSO_WEAPONIDLE3`) within 0.02. Mixed pairs turn it: the pistol stance up to 30
  degrees of yaw, those scripted splits over a stance up to 42.
- The 39 other reference views render pixel for pixel as on main: none of their
  NPCs plays a pair that turns at their time (the troopers hold rifles; the
  academies play the same animation on both parts).
- `bench_world` (300 frames, alternating with main, three runs each, with the new
  view): vjun2_imp_stance 6.7-6.8 ms (main 6.8-7.6), the other NPC views within
  main's spread (academy2 7.8-8.4 against 7.4-8.0, dpred_troopers 7.1-7.3 against
  7.0-7.2; one kor2 run each way was an outlier, 11.5 and 19.6). `load_profile` of
  vjun2 and academy2: within main's spread (vjun2 1739-1896 ms, main 1738-1850).

## Single-player NPCs' blob shadows, 2026-10-16

NPCs now cast the original's blob shadow (`cg_shadows 1`, the captures' setting):
a 14 x 14 x 2 box traced 128 units down from each NPC the server sends within 1000
units of the view, a 32-unit `markShadow` square laid on the floor it hits, faded
with the height, and chopped onto the world's faces, patches and triangle soups
(`R_MarkFragments`). Against the same captures:

| View | Mean difference | main | Pixels off by > 16 | main |
|------|------|------|------|------|
| academy1_class | 0.46 | 0.57 | 0.3% | 0.6% |
| academy2_class | 1.61 | 2.14 | 3.3% | 4.8% |
| academy4_class | 2.23 | 2.77 | 4.4% | 5.9% |
| academy6_class | 2.45 | 3.07 | 5.5% | 7.2% |

- An NPC playing a cinematic animation casts its shadow from its eyes' tag, not its
  root: `CG_PlayerShadow` passes `rootBone` (a bone list number, 0) as a bolt
  number, and bolt 0 is "*head_eyes". The captures agree: from the NPCs' origins
  instead, academy2, 4 and 6 come out at 1.73, 2.42 and 2.61. From that height the
  shadows of the standing students are fainter (about half alpha against four
  fifths).
- What is left on the academy views is mostly the poses of the students whose
  scripts wait a random time.
- Of the 39 other reference views, dpred_troopers (1.13 to 1.02) and vjun2_officer
  (0.58 to 0.52) come closer with their NPCs' shadows; rancor_gates changes (0.85 to
  0.94) because two of the stray flares the original does not draw there now pass
  their depth test. That comes from texture numbering, not shadows: one more
  texture in the set, with shadows off, gives the same image. The rest render pixel
  for pixel as on main.
- `bench_world` (600 frames, alternating with main, three runs, then three more of
  academy6 and dpred_troopers): academy1 6.2-7.9 ms (main 6.7-7.3), academy2
  6.7-7.7 (7.0-8.1), academy4 7.5-8.1 (7.4-12.9), academy6 7.2-8.4 (6.6-8.4),
  dpred_troopers 6.7-7.1 (6.0-6.9), vjun2_officer 5.4-5.5 (5.4-5.6), kor2_troopers
  8.3-9.0 (8.1-11.0): within main's spread. A shadow is laid again only when its
  origin or facing changes; before that cache, laying them every frame cost
  0.3-0.7 ms. Loading builds the shadows' data in under 8 ms (academy2, kor2);
  loads alternated with main fall within their run-to-run spread.

## Single-player random picks as the original's, 2026-10-16

Single player never seeds the game's random numbers, so what a map's spawn picks at
random is the same every run. The capture patch can now log every draw with its
caller (`g_swjkjaRand`): replaying the generator through the spawn gives the
academies' random Jedi (kdm2, hm1, hf2, hf2, zf1, rm2, zf1 and on academy1 and 4
hf1) and their tints as logged (a retail test), and NPCs draw with their colour
(`customRGBA`) where they drew with none. The four academy views were captured
again; main against the same captures:

| View | Mean difference | main | Pixels off by > 16 | main |
|------|------|------|------|------|
| academy1_class | 0.57 | 0.95 | 0.6% | 1.6% |
| academy2_class | 2.14 | 2.64 | 4.8% | 5.8% |
| academy4_class | 2.77 | 3.46 | 5.9% | 7.1% |
| academy6_class | 3.07 | 4.24 | 7.2% | 9.2% |

- The students' heads, clothes and tints now match the captures one for one; what
  is left is lighting (blob shadows, not drawn here) and the poses of the students
  whose scripts wait a random time: those draws come after the client starts,
  whose every frame draws from the same generator, so they vary between captures.
- The 39 other reference views render pixel for pixel as on main (the NPCs of
  dpred_troopers, vjun2_officer and kor2_troopers have no shaders that use the
  colour). `bench_world` (three runs of 300 frames) on the academy views is within
  main's spread (6.6 to 9.1 ms, main 6.4 to 15.5). Loading academy1 and academy2
  takes about 350 ms more (1.40 s against 1.05, three runs each): the random Jedi
  are now six models, not one, with 35 more textures (148 against 113 on academy1),
  which the original loads too.

## Single-player level scripts and their cutscenes, 2026-10-16

A map's script runners now run with its NPCs: the academies' intros start as the
player enters the world (2000 ms) and run as cutscenes, handing blocks to NPCs
(`affect`), signalling, and waiting on voice lines that last their sound files.
academy1's, 4's and 6's Luke stand in BOTH_STAND4 from 2050 ms, Kyle in his
cinematic stance; academy2's Luke talks from 10600 ms and stands again from 12050,
as the patched game's script trace (`g_ICARUSDebug 4`) has it. The captures now
pin the player's entry (`sv_swjkjaBegin`) and let the cutscene run, and wait until
the game reaches the view's time; the four academy views were captured again.

| View | Mean difference | main | Pixels off by > 16 | main |
|------|------|------|------|------|
| academy1_class | 0.95 | 0.99 | 1.6% | 1.7% |
| academy2_class | 2.65 | 4.02 | 5.9% | 7.6% |
| academy4_class | 3.26 | 5.26 | 6.9% | 9.3% |
| academy6_class | 4.28 | 6.38 | 9.2% | 12.0% |

- academy1's 11 students look as logged at our own Luke, within a tenth of a
  degree (a retail test); academy2's 10 that do not wait at random, within 0.05.
- 28 single-player maps start a script runner as the player begins (a retail test
  lists them); every voice line in the retail scripts loads with a length but four
  the game lacks too (766 lines, 45 minutes 46 seconds).
- The 39 other reference views render pixel for pixel as on main. `bench_world`
  (three runs of 600 frames) on the academy views is within main's spread (6.9 to
  8.4 ms on both); loading academy2 takes about 25 ms more (985 ms against 960,
  the scripts run together for the timeline's hour, 55 ms), academy6 within main's
  spread.

## Captures that no longer kill Luke, 2026-10-16

The capture script stands the player 128 units behind each view's eye, and
`setviewpos` kills whatever the player's box lands on (`G_KillBox`): on the
classroom views at y -1200 (academy2, 4 and 6) that was Luke, who then played a
death animation from about 11.8 s, and the students' logged looks followed his
falling eyes. Those views now stand the player 64 units behind (a views file's
optional tenth column), and were captured again: Luke stands, his eyes where ours
are (within 0.01 units on academy2), and 12 of academy2's students look as logged
within 0.03 degrees (a retail test checks 0.05; left out: the two whose scripts
wait a random time and the one whose look target the intro sets). Mean difference
academy2_class 2.70, academy4_class 4.10, academy6_class 4.71: the original picks
its random Jedi looks anew each run, so these views move by a few hundredths to
tenths between captures.

## Single-player NPCs on their maps' cinematic skeletons, 2026-10-16

NPCs playing a map's cinematic animations (`BOTH_CIN_*`) now play them on the
map's cinematic skeleton (`models/players/_humanoid_<map>/`), as the original
does: academy2's six students stand in their cinematic stances, as do academy1's
teachers and academy4's and academy6's students. Two new views of the same
classroom (`bench/reference/script-views.txt`, captured with `SCRIPTS=1`):

| View | Mean difference | Before | Pixels off by > 16 | Before |
|------|------|------|------|------|
| academy1_class | 0.95 | 0.99 | 1.6% | 1.7% |
| academy2_class | 2.68 | 4.06 | 5.9% | 7.6% |
| academy4_class | 3.95 | 5.09 | 7.9% | 9.1% |
| academy6_class | 4.89 | 6.78 | 10.0% | 12.2% |

- The poses now match the captures; most of what is left is the students' random
  Jedi looks (heads, skins and tints the original picks per NPC, not done here).
- academy2: given the original's eye point for Luke, the four students in
  cinematic stances whose scripts wait no random time and who look at Luke are
  within 0.02 degrees of the logged looks (a retail test checks 0.05).
- Every retail cinematic skeleton has `_humanoid.gla`'s bones, parents and base
  poses, and its animations fit its frames (a retail test).

The 39 other reference views render pixel for pixel as on main. `bench_world`
(three runs of 600 frames) on the NPC views is within main's spread (academy2
6.72-7.59 ms, main 7.02-8.14; academy6 7.03-7.31, main 7.22-8.03); loading
(`load_profile`, three runs, twice) academy2 and academy6 within main's spread,
kor2 about 45 ms more (its cinematic skeleton is 10 MB).

## Single-player NPCs looking at their look targets, 2026-10-16

The academies' students look at Luke (their spawnscripts' `SET_LOOK_TARGET`): their
head, neck and upper spine turn toward his eyes, at the look the original settles
at. The capture patch now also logs each NPC's look (`renderInfo.lastHeadAngles`)
100 frames after the view's time is pinned, so the looks can be compared angle by
angle, not only by pixels (`SCRIPTS=1`, both academy views captured again).

| View | Mean difference | Before | Pixels off by > 16 | Before |
|------|------|------|------|------|
| academy1_class | 0.99 | 1.05 | 1.7% | 1.8% |
| academy2_class | 4.06 | 4.10 | 7.6% | 7.6% |

- academy1: all 11 students' looks (pitch, yaw) within 0.075 degrees of the logged
  ones (a retail test checks them within 0.1).
- academy2: given the original's eye point for Luke, the 7 students whose pose
  matches the original's are within 0.02 degrees. The rest differ for reasons
  other than the look: six play the map's cinematic animations (`BOTH_CIN_*`,
  from `models/players/_humanoid_academy2/`, a per-map skeleton not loaded yet)
  and stand here; two look at other students, and Luke plays an animation, set by
  other scripts (`affect`, not run here). Luke's eyes are 22 units higher here, so
  the students look further up.

The 39 earlier reference views (all but hoth2's and vjun1's) render pixel for pixel
as on main. Posing the NPCs takes 0.05 ms more a frame on academy1 and 0.11 ms on
academy2 (the look's steps pose only the bones that place the eyes); `bench_world`
(three runs of 600 frames) on the academy, t2_dpred, vjun2 and kor2 NPC views is
within main's spread (academy1 5.99-7.33 ms, main 5.94-7.34; academy2 6.71-7.82,
main 6.96-8.00), and loading academy2 and kor2 (`load_profile`, three runs) too.

## Single-player NPCs running their spawnscripts, 2026-10-15

Two views in `bench/reference/script-views.txt` look at academy classes, captured
with scripts running and the AI frozen, the game stopped at the view's time
(`SCRIPTS=1`, see the views file). "Before" is main, where every NPC stands in its
stance holding its weapon.

| View | Mean difference | Before | Pixels off by > 16 | Before |
|------|------|------|------|------|
| academy1_class | 1.05 | 4.74 | 1.8% | 6.2% |
| academy2_class | 4.06 | 5.87 | 7.6% | 9.8% |

The capture log gives each scripted animation's start: 1550 ms on the client's clock
for those a script's first pass sets, as here. What still differs: the original
picks a random Jedi for `NPC_Jedi`'s random flag (here the first of twelve) and
random tints, draws blob shadows, and three of academy2's students wait random times
(here their range's low end: one capture had them at `BOTH_CIN_2` from 1550,
`BOTH_CIN_10` from 7690 and `BOTH_CIN_12` from 8988).

The 39 earlier reference views (all but hoth2's and vjun1's) render pixel for pixel
as on main. `bench_world` (three runs of 600 frames): academy1 5.95-7.11 ms (main
8.67-9.67), academy2 7.32-8.33 (8.54-8.88), with fewer triangles drawn (weapons put
away, invisible NPCs left out); dpred_troopers, vjun2_officer and kor2_troopers,
whose NPCs now stand by the same timeline, within main's spread. Loading academy2 and
kor2 (`load_profile`, three runs) stays at main's (0.99-1.05 s and 1.36-1.51 s;
main 1.06-1.08 s and 1.34-1.64 s).

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

## Colour kept between frames, 2026-10-19

fatal_elevator's "red light panel" (409 pixels, up to 29 levels brighter in the
reference) was not the panel: it is t1_fatal's power pipe (`textures/wedge/powerpipe`,
z 4224) seen in the slots between the two coolant tubes at the bottom of the shaft.
The pipe lies on the inside of a brush (the face below it, `vjun/basemetal4`, faces
down), so nothing is drawn behind it, and its first stage, `ggoo1` blended
`GL_ONE GL_SRC_COLOR`, multiplies the goo by what is there. The original clears only
the depth each frame unless the map has a global fog (`RB_BeginDrawingView`,
`r_fastsky` 0), so "what is there" is the last frame's picture, and in a view held
still (the capture holds each view 200 frames) the slots settle on their own
brighter picture. This renderer cleared to black.

Found by elimination: the extra was proportional to the goo's colour, not changed by
anisotropy (off) or LOD bias (±0.5), nor by leaving out the surfaces under the pipe;
one extra draw of the pipe's four stages brought the slots from 2.8 to 0.57 mean
difference, and drawing the view 30 times without clearing the colour to 0.06.

Now `WorldRenderer::clear_color` is `None` without a global fog and frames keep the
last frame's colour (the target starts black; a window image drawn into keeps its
last frame); `render_world` draws each view 10 times before saving it (the same
picture as 30 and 100 frames on every view).

| View | Mean difference (main) | Pixels off by > 2 (main) |
|------|------|------|
| fatal_elevator | 0.03 (0.03) | 4,280 (4,690) |
| rancor_gates | 0.038 (0.043) | 1,770 (1,778) |

- fatal_elevator's slots: mean difference 0.06 (2.8), none off by more than 2.
- rancor_gates changes in 1,581 pixels around its gates (1,088 closer, 454
  further), all of them already about 23 levels off (its last spots). The 47 other
  views render pixel for pixel as on main.
- `a_blend_over_nothing_takes_what_the_last_frame_left`
  (`crates/render/tests/retail.rs`) checks the two slots' mean colours against the
  reference within 0.25 after ten frames (17-19 levels redder than the first frame).
- The ledge at fatal_elevator's right edge (4,280 pixels, 4-5 levels brighter in the
  reference): two surfaces, not one; see "fatal_elevator's ledge" below.
- Not observed in a window: the viewer keeps colour the same way (a swapchain image
  keeps the last frame drawn into it); only offscreen targets were checked.

## fatal_elevator's ledge, 2026-10-19 (found 2026-10-21: the generator's specular)

The 4,280 pixels at fatal_elevator's right edge (mean difference 0.03 over the view,
4-5 levels on the ledge) are what the reference draws brighter there; the cause is
not found. What is known:

- They are two world surfaces, not one: the darkfloor (8554, lightmap 14) and the
  brace beside it (`textures/factory/ll_longbrace`, 7667, sloped, lightmap 15),
  found by casting each pixel's ray through the map's triangles. The brightening is
  one smooth field across both, unbroken at their shared edge: a factor of 1.05-1.11,
  the same in red, green and blue (fitting `ref = a × ours + b` gives b within 1.5
  levels of 0), largest at the ledge's visible near edge and towards +y, gone about
  50 units further on. Its sharp edges are other surfaces' outlines in front.
- It depends on where the eye is. A new view looking straight down on the ledge
  from 236 units (`90 190 4500 89 180`, captured once for this, not kept) matches
  pixel for pixel across the same floor: the reference's lighting there (its normal
  picture divided by an `r_fullbright 1` capture of it) equals the lightmap's texels
  to within 0.3 levels.
- Not a dynamic light: a capture with `r_dynamiclight 0` is byte for byte the
  reference (`RE_RenderScene` then drops every dlight). t1_fatal has no
  `misc_dlight`, no entity with a "light" key and no fx_runner within 1,600 units.
- Not this renderer's mip levels, fog, models or movers: mip level 0 everywhere
  (`max_lod` 0) and leaving out every static model each leave the band as it is;
  t1_fatal's one fog volume lies 3,700 units below; no brush entity comes within 200
  units; the three surfaces are in both eyes' PVS.
- Under `r_fullbright 1` from fatal_elevator's eye, the band is about 0.65 of the
  plain texture (the top-down view: 1.0); under `r_lightmap 1`, it is a blue field
  with a white streak running across the floor, the brace and the wall beside them
  (the top-down view: the darkfloor's grey lightmap). No single surface drawn with
  other lighting gives all three pictures, so the original draws something more
  over those pixels from that eye. A ray cast through the map's surfaces finds
  nothing in front of them but the back of the shaft's ceiling (7987, culled).
- Moving the eye down to `64 -352 4900` (below that ceiling) leaves the ledge out of
  sight, so it does not tell.

Found on the way: the generator beside the ledge (`models/map_objects/factory/
f_gen.md3`, "modelscale_vec" 2.75 2.75 2.5) is drawn 1.5-1.9 times as bright here as
in the reference, in both new views (0.54 and 0.66 of ours over the pixels off by
more than 8). No kept view shows it.

Both were the generator's specular: see "Scaled MD3 models' specular eye" below.

## Scaled MD3 models' specular eye, 2026-10-21

t1_fatal's generator (`f_gen.md3`, a `misc_model_static` at 224 32 4264,
"modelscale_vec" 2.75 2.75 2.5) was 1.75 times as bright here as in the reference,
and it was also fatal_elevator's ledge. Each of its four shaders ends in a
`detail` stage with `alphaGen lightingSpecular`, added over the rest
(`blendFunc GL_SRC_ALPHA GL_ONE`). `RB_CalcSpecularAlpha` works in the model's own
space: `tess.xyz` is the MD3's vertex, unscaled, and `R_RotateForEntity` turns the
eye into the scaled axes but divides it by the first axis's length only, so the eye
is as far from the origin as it is in the world while the vertex is 2.75 times
closer, which turns the viewer's direction most for an eye close to the model.
This renderer took the viewer from the drawn vertex.
Now each MD3 model's entity carries its origin, unit axes and scales, and the
shader reflects in the model's space as the original does (`model_specular`,
`shaders/world.wgsl`): the unit normal, the light turned into the scaled axes (each
axis by its own scale), the vertex unscaled. Ghoul2 models keep the earlier path
(vjun2_officer's test).

The ledge: the generator lies over fatal_elevator's ledge from that eye. Left out
of the branch's render, the ledge's 4,280 pixels come out 28 levels darker; with
it, 4,208 of them now match the reference within 2 levels. The diagnosis on
2026-10-19 cast rays through the map's surfaces, not its models.

| View | Mean difference (main) | Pixels off by > 2 (main) |
|------|------|------|
| fatal_ledge (new) | 0.05 (6.70) | 0.0% > 16 (10.4%) |
| fatal_elevator | 0.008 (0.03) | 137 (4,280) |
| fatal_elev_low (not kept) | 0.03 (0.65) | 0.1% > 16 (1.2%) |

- fatal_ledge (`bench/reference/script-views.txt`, captured 2026-10-19 for the
  ledge with normal settings) is kept: the generator's top in its lower left. Its
  mean colour over x 300-419, y 460-679 is 69.70 65.97 63.14 here, 69.69 65.97
  63.14 in the reference, 142.2 134.9 129.3 on main.
  `scaled_md3_models_see_the_eye_as_the_original` (`crates/render/tests/retail.rs`)
  checks it to 0.1.
- The 49 other kept views render pixel for pixel as main did (PR #89's full
  render, the last code change): none shows a scaled
  MD3 model's specular, and an unscaled one comes out the same either way.
- All 52 kept views: median 0.01, the largest ffa5_pipes (3.75), dpred_guns (0.97),
  hevil_fan_west (0.63), dpred_racks (0.50), vjun1_hall (0.43), as before.
- Left out: a Ghoul2 model's specular keeps the world-space viewer (its vertices
  carry its scale through its bones); `RB_CalcEnvironmentTexCoords` takes the same
  model-space eye and vertex, not done yet (no kept view shows a scaled model
  with an environment map).

## Vertex-lit surfaces' other light styles, 2026-10-21

The original sums a vertex-lit surface's styles (`ComputeFinalVertexColor`): each
style's colour times the vertex's colour for it, up to the first unused style,
then shifted down by 8 bits. This renderer drew the first style alone. Each world
vertex now carries its second to fourth colours (`Vertex::styled`, attributes 6-8)
and each vertex-lit stage its four styles (`Stage.vertex_styles`,
`shaders/world.wgsl`).

Such surfaces are on kor1, kor2 and t3_stamp. On kor1 and t3_stamp they are brushes
whose other colours are mostly 0-4; kor2's statues and braziers take the braziers'
flicker (style 1) at up to 255. No kept view showed one: kor2_troopers, stamp_barrels,
stamp_crates and stamp_cranes render pixel for pixel as on main. kor2_hall2
(`bench/reference/npc-views.txt`, captured 2026-10-21) is new: kor2's hall at the
throne, its guard statues beside it.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| kor2_hall2 (new) | 0.00 (0.22) | 99.7% (95.5%) |

- The left statue's mean colour over x 500-559, y 220-329 is 66.82 42.17 21.47
  here and in the reference, 53.36 36.06 19.91 on main.
  `vertex_lit_surfaces_sum_their_styles_as_the_original`
  (`crates/render/tests/retail.rs`) checks it to 0.05.
- All 53 kept views: median 0.01, the largest ffa5_pipes (3.75), dpred_guns (0.97),
  hevil_fan_west (0.63), dpred_racks (0.50), vjun1_hall (0.43), as before.

## MD3 models' environment maps, 2026-10-21

`RB_CalcEnvironmentTexCoords` reads `tess.xyz` and `tess.normal`, which hold an MD3
model's vertices and normals in its own space, and `backEnd.ori.viewOrigin`, the eye
as `R_RotateForEntity` turns it into the scaled axes (the eye the specular sees, see
"Scaled MD3 models' specular eye" above). Both renderers do (single player's
`normal.xy * d - viewer.xy / 2`, multiplayer's reflection). This renderer took the
world's normal and eye, which differ for any turned or scaled model. Now the
shader reckons an MD3 model's environment map in its space (`model_space`, shared
with `model_specular`, `shaders/world.wgsl`).

Retail maps' turned or scaled MD3 models with an environment map: t3_rift's crystals
(175: statues, pillars, lamps, scaled 1.4-2), t1_surprise's and vjun3's protocol
droid parts (23), hoth3's infirmary tubes (9), vjun2's windows (5) and t1_fatal's
`mech.md3` (1). No kept view showed one: all 53 render pixel for pixel as on main.
Two new views (`bench/reference/script-views.txt`, captured 2026-10-21) are kept:

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| rift_statues (new) | 0.06 (2.04) | 0.1% (5.6%) |
| rift_pillars (new) | 0.03 (0.54) | 0.0% (1.8%) |

- rift_statues: a statue's crystal (`statue.md3`, "modelscale" 2). Its mean colour
  over x 500-739, y 500-699 is 149.31 155.92 223.59 here, 149.63 156.33 223.79 in
  the reference, 166.90 172.34 227.77 on main.
  `md3_environment_maps_are_reckoned_in_the_models_space`
  (`crates/render/tests/retail.rs`) checks it to 0.6. What is left is a thin edge
  at the crystal's top, not examined (`VectorNormalizeFast`'s precision would be
  the first thing to try).
- All 55 kept views: median 0.01, the largest ffa5_pipes (3.75), dpred_guns (0.97),
  hevil_fan_west (0.63), dpred_racks (0.50), vjun1_hall (0.43), as before.
- Left out: Ghoul2 models and turning brush entities keep the world's normal and
  eye for their environment maps, as for their specular.

## Ghoul2 models' specular and environment maps, 2026-10-21

A Ghoul2 model's bones carry no scale: the original scales it by its axes
(`ScaleModelAxis`, `nonNormalizedAxes`), as an MD3 model, and `RB_SurfaceGhoul`
fills `tess.xyz` and `tess.normal` in the model's space. So `RB_CalcSpecularAlpha`
and `RB_CalcEnvironmentTexCoords` see a Ghoul2 model as they see an MD3 one: the
unit normal, the vertex unscaled, the light turned into the scaled axes, and the eye
divided by the first axis's length only. This renderer reflected a Ghoul2 model's
light in the world, about the world's eye. Now every model's entity carries its
origin and axes, and the shader reckons both in the model's space (`model_space`,
`model_specular`, `shaders/world.wgsl`).

Three kept views change, the 52 others render pixel for pixel as on main:

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| vjun2_officer | 0.037 (0.038) | 95.53% (95.49%) |

- vjun2_officer: the officer (`stofficeralt.npc`, scaled 105, his armour and gun
  specular). 1,239 pixels change by up to 3 levels; their summed difference from
  the reference falls from 3,315 to 1,716 (940 closer, 264 further). His chest's
  highlight, mean colour over x 705-724, y 380-399, is 92.76 75.68 68.10 here,
  92.23 75.20 67.60 in the reference, 93.59 76.50 68.93 on main.
  `scaled_models_reflect_their_light_as_the_original`
  (`crates/render/tests/retail.rs`) now checks it to 0.8 too.
- dpred_troopers and kor2_troopers: 29 and 10 pixels by one level, as many closer
  as further (rounding).
- What is left of the highlight (0.5 brighter) is not examined. A first thing to
  try: `RB_CalcSpecularAlpha` divides by the viewer's length with `Q_rsqrt` (one
  Newton step, a little short of the true value), which this shader does not.
- All 55 kept views: median 0.01, the largest as before.
- Ghoul2 environment maps take the same path; no kept view changes by more than
  rounding but the officer's, whose change is his specular.

## The original's fast inverse square root, 2026-10-21

`RB_CalcSpecularAlpha` divides by the viewer's length with `Q_rsqrt`, and
`RB_CalcEnvironmentTexCoords` (and the specular's fixed light) normalise with
`VectorNormalizeFast`, which is the vector times its `Q_rsqrt` length. `Q_rsqrt` is
plain C in both renderers (`shared/qcommon/q_math.c`): one Newton step from
`0x5f3759df`, up to 0.175% under the true value, and never over it. This shader took
the exact `inverseSqrt` and `normalize`. Now it has both of the original's helpers
(`q_rsqrt`, `normalize_fast`, `shaders/world.wgsl`), used where the original uses
them.

22 kept views change, the 33 others render pixel for pixel as on main. Mean
difference from the reference, four places:

| View | Here | Main |
|------|------|------|
| ffa5_pipes | 3.7337 | 3.7462 |
| vjun1_hall | 0.4189 | 0.4293 |
| stamp_cranes | 0.0075 | 0.0091 |
| ffa5_rings | 0.0006 | 0.0021 |
| rancor_drum | 0.0082 | 0.0094 |
| rift_statues | 0.0563 | 0.0553 |
| fatal_elevator | 0.0085 | 0.0084 |

- 19 views come closer; none changes its count of pixels off by more than 16, except
  hevil_fan_west (one fewer) and rift_statues (three more).
- vjun2_officer: his chest's highlight (x 705-724, y 380-399) is 92.46 75.41 67.82
  here, 92.23 75.20 67.60 in the reference, 92.76 75.68 68.10 on main.
  `scaled_models_reflect_their_light_as_the_original` now checks it to 0.3 (main
  fails). What is left (0.2) is not examined.
- rift_statues: the crystal (`textures/rift/env_crystal`, one additive
  `tcGen environment` stage) was already darker than the reference where it
  changes, and comes out darker still (summed over its changed pixels, -0.72 to
  -1.18 levels a pixel); its mean colour over x 500-739, y 500-699 is 149.27 155.88
  223.57 (reference 149.63 156.33 223.79). The shorter viewer moves its texture
  coordinates by about a quarter texel. So the crystal's top edge is not
  `VectorNormalizeFast`'s precision; its cause is not known.
- fatal_elevator: rounding (+0.0001).
- All 55 kept views: median 0.01, the largest as before.

## Turning brush entities' environment maps, 2026-10-21

A brush entity is drawn as an entity (`CG_Mover` gives it the inline model as its
`hModel`), so `R_RotateForEntity` turns the eye into its axes, and `tess.xyz` and
`tess.normal` hold its surfaces in its model's space. `RB_CalcEnvironmentTexCoords`
reckons there. This renderer took the world's normal and eye; for a brush entity
that only slides the two agree, but not for one that turns. Now a turning brush
entity's environment map turns the normal and the eye back by its bone
(`bone_unapply`, `shaders/world.wgsl`).

A survey of the retail maps' brush entities (their surfaces' shaders): 373 have a
`tcGen environment` stage, one an `alphaGen lightingSpecular` stage. Of those with an
environment map, one turns: t2_rancor's drum (func_rotating `*43`,
`textures/factory/env_pipe`), in the kept rancor_drum view. The others are doors,
plats, glass, breakables, statics and t1_rail's rail_movers, which only slide.
rancor_drum is the only kept view that changes; the 54 others render pixel for pixel
as on main.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| rancor_drum | 0.0059 (0.0082) | 98.47% (98.03%) |

- rancor_drum: 4,097 pixels change; their summed difference from the reference
  falls from 6,329 levels to 21. The drum's mean colour over x 630-649, y 540-559
  is 102.84 90.70 78.92 here, 102.80 90.68 78.90 in the reference, 103.28 91.10
  79.26 on main. `turning_brush_entities_reckon_their_environment_maps_in_their_space`
  (`crates/render/tests/retail.rs`) checks it to 0.15.
- All 55 kept views: median 0.01, the largest as before.
- Left out: the specular. `RB_CalcSpecularAlpha` takes a brush entity for a model
  (its `hModel`) and so reads its `lightDir`, which `R_SetupEntityLighting` never
  fills for one (only for a `misc_bsp` instance): it holds whatever the last lit
  model in its slot of `backEndData->entities` left, zero if none. The one retail
  brush entity with a specular stage is t1_fatal's `*85` (func_breakable
  "objects", `textures/factory/wall10`), some 3,700 units from the kept t1_fatal
  views; this renderer gives it the world's fixed light.

## A weapon put away by another script's affect block, 2026-10-21

yavin1_students' largest difference (mean 0.04) sat in two places. One was a thin
dark wedge standing up behind Rosh's hair (x 293-333, y 450-512), drawn here and not
in the original: his training saber's hilt, in his hands clasped behind his head.
yavin1's intro puts it away from an `affect` block on `npc_roshintro1`
(`SET_WEAPON WP_NONE`: `G_SetWeapon` calls `G_RemoveWeaponModels`), but this
renderer only dropped an NPC's bolted models when its own spawnscript's first pass
did it. Now each NPC's script timeline records when `SET_WEAPON` puts its weapon
away ("WP_NONE" or "drop") or brings one back (any other weapon), from whichever
script runs it, and an NPC draws no bolted models while unarmed
(`StaticModels::piece_shown`). A weapon brought back shows the models it spawned
with, not the named weapon's.

Two kept views change; the 53 others render pixel for pixel as on main.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| yavin1_students | 0.0360 (0.0380) | 93.70% (93.69%) |
| academy4_class | 0.0619 (0.0696) | 96.82% (96.80%) |

- yavin1_students: 104 pixels change; their summed difference from the reference
  falls from 5,704 levels to 22. The mean colour over x 300-319, y 444-463 is
  20.85 20.60 19.97 here, 20.86 20.60 19.93 in the reference, 18.13 17.97 17.42 on
  main. `an_npc_put_unarmed_by_a_script_drops_its_saber`
  (`crates/render/tests/retail.rs`) checks it to 0.15.
- academy4_class: Kyle's hilt, held through his crossed arms, is gone as in the
  reference; 149 pixels change, their summed difference falling from 21,411 levels
  to 3.
- What is left of yavin1_students: about a fifth is `jedi_student01` (the leaning
  student at the back, x 480-520, y 200-280), whose hands and head stand a pixel or
  two from the original's (`yavin1/jedi_stud_leaning` loops `BOTH_STAND10` and its
  two talks a random number of times); not examined further. The rest is spread
  thin, a level here and there.
- All 55 kept views: median 0.01, the largest as before.

## A loop of none, and the first pass's timers at NPC_Begin, 2026-10-21

About a fifth of yavin1_students' difference was `jedi_student01`, the leaning
student at the back (x 480-520, y 200-280): her hands and head a pixel or two from
the original's. Her script (`yavin1/jedi_stud_leaning`) loops forever over a
`random(0, 3)` loop of `BOTH_STAND10`, `BOTH_STAND10_TALK1`, another such loop and
`BOTH_STAND10_TALK2`, each a `do` and a `wait`. Two causes, both ours:

- **A loop of none runs once.** `CheckLoop` enters a loop's body before it reads
  the count, and only ends it at the body's end when the count is 0; the captures
  pin `random(0, 3)` to 0. Here a count of 0 skipped the body, so she never stood
  between her talks.
- **`NPC_Begin`'s movement frame counts the first pass's timers down 200 ms.** It
  runs `ClientThink` with a zeroed command, whose time `ClientThink` raises to the
  level's less 1000 (550 ms) against a `commandTime` of 0, and `Pmove` caps the
  550 at 200. Later frames count 50 ms each (the AI frozen, as in the captures, the
  commands' times stay 1000 ms behind the level's), so an animation the first pass
  starts runs out 200 ms early, and every one chained after it too. Here they
  counted 0 there.

The capture log has her in `BOTH_STAND10_TALK1` from 10,802 ms (the log numbers this
range's animations two below `anims.h`'s order, as its timings for the other
students show); she starts it at 10,800 here (11,000 with the loop fixed alone).

Only yavin1_students changes; the 54 others render pixel for pixel as on main.

| View | Mean difference (main) | Exact pixels (main) |
|------|------|------|
| yavin1_students | 0.0297 (0.0360) | 93.78% (93.70%) |

- yavin1_students: 914 pixels change; their summed difference from the reference
  falls from 17,929 levels to 563. The mean colour over her hands (x 484-503, y
  240-259) is 43.22 41.17 33.28 here, 43.02 41.03 33.18 in the reference, 40.77
  39.97 32.78 on main. `a_loop_of_none_runs_once_and_npcs_begin_200_ms_on`
  (`crates/render/tests/retail.rs`) checks it to 0.3.
- Not modelled: `NPC_Begin` sets the first `NPC_Think` 100 ms plus `Q_irand(0, 100)`
  on, so a script's later updates start then, not on the next frame (this
  renderer runs them from the next). Not checked against the kept views.
- All 55 kept views: median 0.01, the largest as before.

## Rack goods spread with the original's random numbers, 2026-10-21

dpred_guns' 0.97, the largest after ffa5_pipes, was its weapons' outlines (as the
2026-10-18 flares section says): the original spreads each rack item with the game
module's random numbers, and this renderer stood them in the middle of their range.
Single player never seeds those (see DECISIONS.md, 2026-10-16), and a capture with
`g_swjkjaRand` logged t2_dpred's draws: up to the ammo racks, only the
stormtroopers' coin flips (already replayed) and the gun racks draw, five numbers
per weapon (aside, forward, pitch, yaw, roll); the ammo racks draw at 1100 ms. The
spawn's replay now draws the racks' numbers too, and the goods stand where the
original puts them.

| View | Mean difference (main) | Pixels off by > 16 (main) |
|------|------|------|
| dpred_guns | 0.01 (0.97) | 0.0% (3.0%) |
| dpred_racks | 0.00 (0.50) | 0.0% (1.6%) |
| vjun2_officer | 0.02 (0.04) | 0.0% (0.1%) |
| stamp_barrels | 0.00 (0.01) | 0.0% (0.0%) |

- Only the four views with rack goods change; the 51 others render pixel for pixel
  as on main. vjun2_officer's and stamp_barrels' ammo racks, which no log covers,
  now match too ("its ammo rack's items, turned at random", half of
  vjun2_officer's 0.04 in the 2026-10-18 section).
- The mean colour by a blaster on dpred_guns' middle rack (x 290-309, y 290-309) is
  51.72 16.87 24.31 here, 51.72 16.84 24.32 in the reference, 18.56 15.31 10.74 on
  main. `rack_weapons_spread_as_the_original_spreads_them`
  (`crates/render/tests/retail.rs`) checks it to 0.3, and
  `t2_dpreds_rack_goods_spread_as_in_the_original` (`crates/world/tests/retail.rs`)
  the goods' places against the logged numbers.
- All 55 kept views: median 0.01. The largest are ffa5_pipes (3.73), hevil_fan_west
  (0.63) and vjun1_hall (0.42); none other above 0.2.

## vjun1_hall recaptured without rain, 2026-10-26

vjun1_hall's 0.42 was the original's acid rain, which falls where neither side can
reproduce (DECISIONS.md, 2026-10-24). The reference is recaptured with
`r_swjkjaNoWeather 1`, and `render_world` leaves rain out too. Two capture fixes
were needed on the way: vjun1's intro script opens with a video (ja07), which ended
the cutscene skip and covered the view (`cl_swjkjaNoVideo` now keeps scripts'
videos from playing); and the skip itself never ended, as the intro waits on Kyle
and Jaden, whose scripts do not run with NPCs frozen, and the client draws nothing
while a skip runs (a second `exitview` now ends it).

| View | Mean difference (before) | Pixels off by > 16 (before) |
|------|------|------|
| vjun1_hall | 0.0004 (0.42) | 0.0% (1.0%) |

- The new reference differs from the October one in 19001 pixels, and the old one
  is brighter or equal in every one of them: only the rain is gone. 1159 pixels
  differ from this renderer's view now (at most 24 in a channel).
- Only vjun1_hall was recaptured and rendered; the other 54 kept views are as in
  the section above. The largest are now ffa5_pipes (3.73) and hevil_fan_west
  (0.63, `bespin/dust` effects, not rain: DECISIONS.md, 2026-10-26).

## Voice lines a frame later, 2026-10-26

academy4's intro, traced in the original (`g_ICARUSDebug 4`), ends each voice line
a frame later than this renderer did (DECISIONS.md, 2026-10-26): its lines start
here at 3100, 15,700, 19,450, 23,150, 32,000 and 44,500 ms, in the original at
3100, 15,750, 19,550, 23,300 (23,350 in a second run), 32,200 and 44,800. With
lines a frame later, the first five start here as in the first run, and Kyle's at
44,750: Luke's fourth line ended a frame later still in both runs (the mixer runs
in real time; no fixed lag fits all six).

- No kept view changes: the 33 single-player views render pixel for pixel as
  before (none reaches the end of a line its intro waits on); the 22 multiplayer
  views run no scripts.
- `a_voice_line_holds_its_task_group_until_it_has_played`
  (`crates/world/tests/script.rs`) checks a line's task completing a frame later.

## NPCs turned by their scripts, 2026-10-26

academy4's intro turns Jaden (`npc_fakePlayer1`) toward Luke as Luke's fifth line
starts ("jaden turn": `SET_DYAW 270`), at yaw speed 90: 16.5 degrees a frame from
325, here from 47,750 to 47,900 ms, in the traced original from 47,800 to 47,950
(its voice lines end a frame apart from run to run; DECISIONS.md, 2026-10-26).
academy4_jaden_turned (`bench/reference/turn-views.txt`) looks at him from the
front at 48,500 ms, his head turned to Luke.

| View | Mean difference (unturned) | Pixels off by > 16 (unturned) |
|------|------|------|
| academy4_jaden_turned | 0.03 (1.13) | 0.0% (2.1%) |

- The mean colour over his elbow (x 670-689, y 410-429) is 143.74 148.51 103.84
  here, 143.74 148.41 103.84 in the reference, 35.90 38.65 36.12 unturned (the
  floor behind). `a_script_turns_an_npc_to_its_yaw` (`crates/render/tests/retail.rs`)
  checks it to 0.3.
- No kept view changes: the 33 single-player views render pixel for pixel as
  before (none reaches a turn); the 22 multiplayer views run no scripts.
- The original's eyes face 276.9 degrees in the capture log (270, and 6.0 of his
  look at Luke).
