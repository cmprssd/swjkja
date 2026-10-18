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
