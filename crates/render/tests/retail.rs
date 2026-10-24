//! Renders with the retail game data. Runs only when SWJKJA_GAMEDATA points at the
//! game's GameData folder (on Windows, with a real GPU, in `.local-ci`).

use std::path::PathBuf;

use swjkja_render::{Camera, FRAMES_IN_FLIGHT, FrameLoop, Gpu, Target, TextureSet, WorldRenderer};

/// Frames through a [`FrameLoop`], each drawn while the previous one may still be on
/// the GPU, look exactly like the same frame rendered on its own. Uses the start of
/// the benchmark's camera path on mp/ffa3 (moving camera, animated shaders, flares).
#[test]
fn frames_in_flight_render_like_single_frames() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let path: Vec<(f32, Camera)> =
        swjkja_render::path::parse(include_str!("../../../bench/reference/ffa3.path"))
            .unwrap()
            .iter()
            .take(60)
            .map(|f| (f.time_ms as f32 / 1000.0, f.camera(320, 180)))
            .collect();

    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/mp/ffa3.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/mp/ffa3.bsp",
        0.0,
    )
    .unwrap();
    // Two targets, alternating, so a frame's picture survives the next frame.
    let targets = [0, 1].map(|_| Target::new(&gpu, 320, 180).unwrap());
    let depth_slots = targets
        .each_ref()
        .map(|t| textures.add_view(t.depth_copy.view));
    let clear = renderer.clear_color();

    // Render the path up to one frame past `last`, so frame `last` was in flight while
    // the next was recorded, then compare it with frame `last` rendered alone. Ending
    // on frames that use different slots checks each slot's data.
    let mut frames = FrameLoop::new(&gpu).unwrap();
    let mut gpu_times = 0;
    let runs = [path.len() - 1 - FRAMES_IN_FLIGHT, path.len() - 2];
    for last in runs {
        for (k, (time, camera)) in path[..=last + 1].iter().enumerate() {
            let t = frames
                .render(&targets[k % 2], clear, |frame| {
                    renderer
                        .draw(frame, &textures, camera, *time, depth_slots[k % 2])
                        .unwrap()
                })
                .unwrap();
            gpu_times += usize::from(t.is_some());
        }
        gpu_times += frames.finish().unwrap().len();
        let (target, depth_slot) = (&targets[last % 2], depth_slots[last % 2]);
        let pipelined = target.read_rgba(&gpu).unwrap();

        let (time, camera) = &path[last];
        target
            .render(&gpu, clear, |frame| {
                renderer
                    .draw(frame, &textures, camera, *time, depth_slot)
                    .unwrap()
            })
            .unwrap();
        let alone = target.read_rgba(&gpu).unwrap();
        let differ = pipelined.iter().zip(&alone).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "frame {last}: {differ} bytes differ");
        // Not a blank frame: the view shows the map.
        assert!(alone.chunks(4).any(|p| p[..3] != alone[..3]));
    }
    // Every submitted frame's GPU time came back exactly once.
    assert_eq!(gpu_times, runs.iter().map(|l| l + 2).sum::<usize>());
}

/// Flares are tested against the depth the world drew before them, and drawn only
/// from leaves the view sees: on t2_rancor's rancor_gates view, the ceiling flares
/// behind the corridor's wall and gates and the flares of the room beyond the left
/// gate (outside the view's PVS) leave the picture as the original's reference
/// shows it. Each box's brightest channel, measured from the reference capture.
#[test]
fn flares_behind_walls_and_outside_the_pvs_are_not_drawn() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t2_rancor.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/t2_rancor.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([-568.0, 4136.0, 1530.0], [5.0, 180.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // A 9x9 box's brightest channel around each flare's place on screen: five on
    // the ceiling behind the wall and the right gate, two in the room beyond the
    // left gate. With those flares drawn, the boxes reach 97-234.
    let brightest = |x: usize, y: usize| {
        let mut max = 0;
        for py in y - 4..=y + 4 {
            for px in x - 4..=x + 4 {
                let p = (py * width as usize + px) * 4;
                max = max.max(*rgba[p..p + 3].iter().max().unwrap());
            }
        }
        max
    };
    let boxes = [
        (1118, 70),
        (956, 156),
        (885, 186),
        (841, 206),
        (1270, 70),
        (232, 391),
        (79, 439),
    ];
    let ours = boxes.map(|(x, y)| brightest(x, y));
    let reference = [14, 32, 14, 50, 47, 0, 46];
    for ((&got, want), place) in ours.iter().zip(reference).zip(boxes) {
        assert!(
            got.abs_diff(want) <= 4,
            "box at {place:?}: {got}, the reference {want} (all: {ours:?})"
        );
    }
}

/// Single player's environment mapping (code/rd-vanilla RB_CalcEnvironmentTexCoords)
/// is not multiplayer's reflection: vjun1's glass floor tiles
/// (textures/common/glass_security_tris, an added tcGen environment stage) under the
/// view of bench/reference/model-views.txt's vjun1_hall.
#[test]
fn single_player_environment_maps_as_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/vjun1.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/vjun1.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([-7052.0, 468.0, 824.0], [10.0, 0.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // A 9x9 box's mean red around four places on the tiles. Multiplayer's
    // reflection gives 98, 38, 86 and 53.
    let red = |x: usize, y: usize| {
        let mut sum = 0u32;
        for py in y - 4..=y + 4 {
            for px in x - 4..=x + 4 {
                sum += u32::from(rgba[(py * width as usize + px) * 4]);
            }
        }
        sum / 81
    };
    let boxes = [(250, 400), (400, 400), (1000, 410), (1200, 380)];
    let ours = boxes.map(|(x, y)| red(x, y));
    let reference = [52, 54, 69, 78];
    for ((&got, want), place) in ours.iter().zip(reference).zip(boxes) {
        assert!(
            got.abs_diff(want) <= 3,
            "box at {place:?}: {got}, the reference {want} (all: {ours:?})"
        );
    }
}

/// The global fog's distance is the eye-plane distance's absolute value per vertex,
/// interpolated across the triangle, as the reference PC's GL draws fixed-function
/// fog: a triangle reaching behind the eye is foggier near it. mp/ctf4's walkway
/// (textures/rooftop/window_building_trim) runs over the view of
/// bench/reference/views.txt's ctf4_up and behind it; the fog's colour (0, 36, 47)
/// shows in its panels as blue over red.
#[test]
fn global_fog_takes_the_distance_per_vertex_as_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/mp/ctf4.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/mp/ctf4.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([544.0, 1824.0, -260.0], [-25.0, 270.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // A 9x9 box's mean blue less its mean red, at four places on the walkway's
    // panels. Fogged by the depth at each pixel, they are 4 lower.
    let blue_over_red = |x: usize, y: usize| {
        let mut sum = 0i32;
        for py in y - 4..=y + 4 {
            for px in x - 4..=x + 4 {
                let p = (py * width as usize + px) * 4;
                sum += i32::from(rgba[p + 2]) - i32::from(rgba[p]);
            }
        }
        sum as f32 / 81.0
    };
    let boxes = [(1200, 150), (1250, 60), (1150, 300), (1100, 250)];
    let ours = boxes.map(|(x, y)| blue_over_red(x, y));
    let reference = [3.6, 4.5, 0.9, 4.4];
    for ((&got, want), place) in ours.iter().zip(reference).zip(boxes) {
        assert!(
            (got - want).abs() <= 1.0,
            "box at {place:?}: {got}, the reference {want} (all: {ours:?})"
        );
    }
}

/// ComputeFinalVertexColor: a vertex-lit surface's colours are its light style's
/// colour times the vertex's, shifted down by 8 bits, so a style at full (255) takes
/// one level off each vertex. t3_stamp's crates on the right of
/// bench/reference/views.txt's stamp_crates are vertex-lit (textures/factory/s_trim2);
/// with the vertices' colours as stored they are 0.39 brighter than the reference.
#[test]
fn vertex_lit_surfaces_take_their_style_as_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t3_stamp.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/t3_stamp.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([-3248.0, -3574.0, 124.0], [10.0, 90.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The crates' mean colour (x 740-1269, y 110-409).
    let mut sum = [0u64; 3];
    for y in 110..410 {
        for x in 740..1270 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (300.0 * 530.0));
    let reference = [12.418, 12.741, 13.843];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.05,
            "the crates' mean colour {ours:?}, the reference {reference:?}"
        );
    }
}

/// ComputeFinalVertexColor sums a vertex-lit surface's styles: each style's colour
/// times the vertex's colour for it. kor2's guard statues by the throne
/// (bench/reference/npc-views.txt's kor2_hall2, `statue_guard01`/`02`) are lit by
/// styles 0 and 1, the braziers' flicker; with the first style alone the left one is
/// 13.5 darker in red than the reference.
#[test]
fn vertex_lit_surfaces_sum_their_styles_as_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/kor2.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/kor2.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([14112.0, -100.0, -3080.0], [5.0, 270.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The left statue's mean colour (x 500-559, y 220-329).
    let mut sum = [0u64; 3];
    for y in 220..330 {
        for x in 500..560 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (110.0 * 60.0));
    let reference = [66.821, 42.166, 21.471];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.05,
            "the statue's mean colour {ours:?}, the reference {reference:?}"
        );
    }
}

/// The original reflects a model's light for its specular stages
/// (`RB_CalcSpecularAlpha`) in the model's space: a unit normal about the light
/// turned into the scaled axes, so a model scaled by s reflects s times the unit
/// reflection. vjun2's alternate officer (bench/reference/npc-views.txt's
/// vjun2_officer) is scaled 105, its armour and gun specular; reflecting its scaled
/// normal about the unit light made it 0.66 brighter than the reference. A Ghoul2
/// model reflects in its space as an MD3 model does, seeing the eye from further
/// away than it is: with the world's eye, a highlight on his chest was 1.36 brighter.
/// It divides by the viewer's length with `Q_rsqrt` (one Newton step, a little short
/// of the true value): with the exact inverse square root, the highlight was 0.53
/// brighter.
#[test]
fn scaled_models_reflect_their_light_as_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/vjun2.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/vjun2.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([1264.0, 1800.0, 3300.0], [5.0, 90.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The officer's mean colour (x 590-759, y 290-609).
    let mut sum = [0u64; 3];
    for y in 290..610 {
        for x in 590..760 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (320.0 * 170.0));
    let reference = [81.576, 56.931, 40.591];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.1,
            "the officer's mean colour {ours:?}, the reference {reference:?}"
        );
    }
    // The chest's highlight (x 705-724, y 380-399).
    let mut sum = [0u64; 3];
    for y in 380..400 {
        for x in 705..725 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / 400.0);
    let reference = [92.228, 75.198, 67.598];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.3,
            "the officer's highlight {ours:?}, the reference {reference:?}"
        );
    }
}

/// An MD3 model reflects its light in its own space (`RB_CalcSpecularAlpha`): the
/// vertex unscaled, and the eye turned into the scaled axes but divided by the
/// first one's length only, so a scaled model sees the eye from further away than
/// it is. t1_fatal's generator (`f_gen.md3`, "modelscale_vec" 2.75 2.75 2.5), seen
/// from 236 units above (bench/reference/script-views.txt's fatal_ledge), was 1.75
/// times as bright as the reference with the eye taken from the drawn vertex.
#[test]
fn scaled_md3_models_see_the_eye_as_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t1_fatal.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 7.0;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/t1_fatal.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([90.0, 190.0, 4500.0], [89.0, 180.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The generator's mean colour (x 300-419, y 460-679).
    let mut sum = [0u64; 3];
    for y in 460..680 {
        for x in 300..420 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (220.0 * 120.0));
    let reference = [69.691, 65.969, 63.137];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.1,
            "the generator's mean colour {ours:?}, the reference {reference:?}"
        );
    }
}

/// An MD3 model's environment map is reckoned in its own space too
/// (`RB_CalcEnvironmentTexCoords`): its unit normal, its vertex unscaled and the
/// eye as its specular sees it. A statue's crystal in t3_rift (`statue.md3`,
/// "modelscale" 2), seen from 340 units (bench/reference/script-views.txt's
/// rift_statues), was 17 levels too bright with the world-space normal and eye.
#[test]
fn md3_environment_maps_are_reckoned_in_the_models_space() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t3_rift.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/t3_rift.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([-896.0, 3200.0, 860.0], [5.0, 90.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The crystal's mean colour (x 500-739, y 500-699).
    let mut sum = [0u64; 3];
    for y in 500..700 {
        for x in 500..740 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (200.0 * 240.0));
    let reference = [149.627, 156.326, 223.788];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.6,
            "the crystal's mean colour {ours:?}, the reference {reference:?}"
        );
    }
}

/// A turning brush entity's environment map is reckoned in its model's space
/// (`RB_CalcEnvironmentTexCoords` after `R_RotateForEntity`): its own normal and the
/// eye turned into its axes. t2_rancor's drum (a func_rotating, `textures/factory/
/// env_pipe`), seen through its grate (bench/reference/model-views.txt's
/// rancor_drum), took the world's normal and eye, half a level too bright here.
#[test]
fn turning_brush_entities_reckon_their_environment_maps_in_their_space() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t2_rancor.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/t2_rancor.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([-896.0, 2300.0, 1792.0], [0.0, 90.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The drum's mean colour (x 630-649, y 540-559).
    let mut sum = [0u64; 3];
    for y in 540..560 {
        for x in 630..650 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (20.0 * 20.0));
    let reference = [102.80, 90.68, 78.90];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.15,
            "the drum's mean colour {ours:?}, the reference {reference:?}"
        );
    }
}

/// An NPC whose script puts its weapon away draws no bolted models from then on
/// (`SET_WEAPON WP_NONE`: `G_RemoveWeaponModels`), also when another script's
/// `affect` block does it. yavin1's intro puts Rosh's training saber away; seated
/// with his hands behind his head, its hilt stood up behind his hair here
/// (bench/reference/script-views.txt's yavin1_students), 2.7 levels too dark.
#[test]
fn an_npc_put_unarmed_by_a_script_drops_its_saber() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/yavin1.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/yavin1.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([3990.0, -120.0, 30.0], [15.0, 90.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The mean colour behind Rosh's hair (x 300-319, y 444-463).
    let mut sum = [0u64; 3];
    for y in 444..464 {
        for x in 300..320 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (20.0 * 20.0));
    let reference = [20.86, 20.60, 19.93];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.15,
            "the mean colour behind Rosh's hair {ours:?}, the reference {reference:?}"
        );
    }
}

/// A script's loop of none runs once (`CheckLoop` reads the count only at the body's
/// end), and `NPC_Begin`'s movement frame counts the first pass's timers down 200 ms.
/// yavin1's leaning student (`jedi_stud_leaning`, its `random(0, 3)` loops at their
/// low end) skipped her stands and, a frame late on each animation, stood in the
/// wrong one (bench/reference/script-views.txt's yavin1_students): her hands 2.3
/// levels too dark.
#[test]
fn a_loop_of_none_runs_once_and_npcs_begin_200_ms_on() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/yavin1.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/yavin1.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([3990.0, -120.0, 30.0], [15.0, 90.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The mean colour over her hands (x 484-503, y 240-259).
    let mut sum = [0u64; 3];
    for y in 240..260 {
        for x in 484..504 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (20.0 * 20.0));
    let reference = [43.02, 41.03, 33.18];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.3,
            "the mean colour over her hands {ours:?}, the reference {reference:?}"
        );
    }
}

/// A map with a sky portal (`misc_skyportal`) shows the scene from there as its sky,
/// with the view's angles, and the view's own sky surfaces draw nothing
/// (`CG_DrawSkyBoxPortal`). t3_hevil's portal looks from a room with the sky box and
/// distant rock; hevil_ship (bench/reference/script-views.txt) looks up at its sky,
/// 24-31 levels darker than the reference there when drawn as the view's own sky box.
#[test]
fn a_sky_portal_draws_the_sky_from_where_it_stands() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t3_hevil.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 6.0;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/t3_hevil.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([-200.0, -3300.0, -600.0], [-25.0, 45.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The sky's mean colour between the walls (x 480-559, y 0-39).
    let mut sum = [0u64; 3];
    for y in 0..40 {
        for x in 480..560 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (40.0 * 80.0));
    let reference = [125.923, 141.706, 139.78];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.1,
            "the sky's mean colour {ours:?}, the reference {reference:?}"
        );
    }
}

/// Without a global fog, a frame keeps the last frame's colour where nothing opaque
/// is drawn (`RB_BeginDrawingView` clears only the depth). t1_fatal's power pipe
/// (`textures/wedge/powerpipe`) lies on the inside of a brush, and its first stage
/// (`GL_ONE GL_SRC_COLOR`) multiplies its goo by what is there: in a view held still
/// (fatal_elevator, bench/reference/script-views.txt, without its uses, which move
/// nothing here) the goo seen through the pipe's slots settles 17-19 levels redder
/// than over black.
#[test]
fn a_blend_over_nothing_takes_what_the_last_frame_left() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t1_fatal.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 7.0;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/t1_fatal.bsp",
        time,
    )
    .unwrap();
    assert_eq!(renderer.clear_color(), None);
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([64.0, -352.0, 5050.0], [89.0, 180.0, 0.0], width, height);
    // The mean colour of the two slots between the coolant tubes (y 528-555).
    let slots = |rgba: &[u8]| {
        [(662, 667), (673, 677)].map(|(x0, x1)| {
            let mut sum = [0u64; 3];
            for y in 528..556 {
                for x in x0..x1 {
                    let p = (y * width as usize + x) * 4;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u64::from(rgba[p + c]);
                    }
                }
            }
            sum.map(|s| s as f32 / (28 * (x1 - x0)) as f32)
        })
    };
    let mut frame = || {
        target
            .render(&gpu, renderer.clear_color(), |frame| {
                renderer
                    .draw(frame, &textures, &camera, time, depth_slot)
                    .unwrap()
            })
            .unwrap();
        slots(&target.read_rgba(&gpu).unwrap())
    };
    // The first frame, over the target's black.
    let first = frame();
    for _ in 0..8 {
        frame();
    }
    let held = frame();
    let reference = [[106.793, 75.65, 45.479], [101.625, 82.491, 47.991]];
    for ((first, held), want) in first.iter().zip(held).zip(reference) {
        assert!(
            (first[0] - want[0]).abs() > 15.0,
            "over black {first:?}, the reference {want:?}"
        );
        for (got, want) in held.iter().zip(want) {
            assert!(
                (got - want).abs() <= 0.25,
                "held {held:?}, the reference {want:?}"
            );
        }
    }
}

/// The rack weapons are spread and turned with the game's random numbers, replayed
/// through the spawn: t2_dpred's middle rack in bench/reference/model-views.txt's
/// dpred_guns stood its weapons in the middle of their range, so a blaster's barrel
/// showed a little off its reference, 33 levels too dark on its red lamp.
#[test]
fn rack_weapons_spread_as_the_original_spreads_them() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t2_dpred.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/t2_dpred.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([95.0, -2030.0, 625.0], [15.0, 270.0, 0.0], width, height);
    target
        .render(&gpu, renderer.clear_color(), |frame| {
            renderer
                .draw(frame, &textures, &camera, time, depth_slot)
                .unwrap()
        })
        .unwrap();
    let rgba = target.read_rgba(&gpu).unwrap();
    // The mean colour over x 290-309, y 290-309 (main: 18.56 15.31 10.74).
    let mut sum = [0u64; 3];
    for y in 290..310 {
        for x in 290..310 {
            let p = (y * width as usize + x) * 4;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += u64::from(rgba[p + c]);
            }
        }
    }
    let ours = sum.map(|s| s as f32 / (20.0 * 20.0));
    let reference = [51.72, 16.84, 24.32];
    for (got, want) in ours.iter().zip(reference) {
        assert!(
            (got - want).abs() <= 0.3,
            "the mean colour by the blaster {ours:?}, the reference {reference:?}"
        );
    }
}

/// Rain only adds light (`GLS_SRCBLEND_ONE | GLS_DSTBLEND_ONE`): vjun1's hall
/// (vjun1_hall in bench/reference/model-views.txt) drawn after 30 frames of rain is
/// nowhere darker than without it, and its streaks brighten some of the frame.
/// `render_world` leaves rain out (`draw_weather`), as the captures do.
#[test]
fn rain_adds_its_streaks_to_the_frame() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/vjun1.bsp").unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let time = 12.345;
    let mut renderer = WorldRenderer::new(
        &gpu,
        &mut textures,
        &vfs,
        &shaders,
        &bsp,
        &world,
        "maps/vjun1.bsp",
        time,
    )
    .unwrap();
    let (width, height) = (1280, 720);
    let target = Target::new(&gpu, width, height).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);
    let camera = Camera::new([-7052.0, 468.0, 824.0], [10.0, 0.0, 0.0], width, height);
    let draw = |renderer: &mut WorldRenderer, at: f32| {
        target
            .render(&gpu, renderer.clear_color(), |frame| {
                renderer
                    .draw(frame, &textures, &camera, at, depth_slot)
                    .unwrap()
            })
            .unwrap();
    };
    renderer.draw_weather = false;
    for _ in 0..10 {
        draw(&mut renderer, time);
    }
    let dry = target.read_rgba(&gpu).unwrap();
    renderer.draw_weather = true;
    for k in (0..30).rev() {
        draw(&mut renderer, time - k as f32 * 0.016);
    }
    let wet = target.read_rgba(&gpu).unwrap();
    let mut darker = 0;
    let mut brighter = 0;
    for (d, w) in dry.chunks(4).zip(wet.chunks(4)) {
        if (0..3).any(|c| w[c].saturating_add(1) < d[c]) {
            darker += 1;
        }
        if (0..3).any(|c| w[c] > d[c].saturating_add(8)) {
            brighter += 1;
        }
    }
    eprintln!("rain brightens {brighter} pixels by more than 8 levels");
    assert_eq!(darker, 0);
    assert!(brighter > 500, "{brighter}");
}
