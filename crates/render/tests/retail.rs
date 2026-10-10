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
        swjkja_world::static_model::Rules::for_map("maps/mp/ffa3.bsp"),
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
