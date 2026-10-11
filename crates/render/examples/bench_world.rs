//! Frame times of the stage renderer on the views of a views file: each view is
//! rendered `frames` times after a warm-up. "frame" is record + submit + GPU + wait
//! (this loop waits for every frame); "GPU" is the GPU's own time from timestamps.
//! Usage: bench_world <GameData> <views.txt> [frames]
use std::time::Instant;

use swjkja_render::{Camera, Gpu, Target, TextureSet, WorldRenderer};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let frames: usize = args.get(2).map_or(300, |f| f.parse().unwrap());
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let lines: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().collect())
        .collect();
    let mut maps: Vec<&str> = lines.iter().map(|l| l[0]).collect();
    maps.dedup();
    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[args[0].clone().into()], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    println!(
        "{:16} {:>9} {:>9} {:>9} {:>9}  ({frames} frames, {}, 1280x720)",
        "view", "frame ms", "p99 ms", "fps", "GPU ms", gpu.name
    );
    for map in maps {
        let bsp = swjkja_bsp::Bsp::parse(&vfs.read(&format!("maps/{map}.bsp")).unwrap()).unwrap();
        let world = swjkja_world::World::load(&bsp, &Default::default());
        let mut textures = TextureSet::new(&gpu).unwrap();
        let mut renderer = WorldRenderer::new(
            &gpu,
            &mut textures,
            &vfs,
            &shaders,
            &bsp,
            &world,
            swjkja_world::static_model::Rules::for_map(&format!("maps/{map}.bsp")),
            0.0,
        )
        .unwrap();
        let target = Target::new(&gpu, 1280, 720).unwrap();
        let depth_slot = textures.add_view(target.depth_copy.view);
        for l in lines.iter().filter(|l| l[0] == map) {
            let f: Vec<f32> = l[2..8].iter().map(|v| v.parse().unwrap()).collect();
            let camera = Camera::new([f[0], f[1], f[2]], [f[3], f[4], f[5]], 1280, 720);
            let (mut times, mut gpu_times) =
                (Vec::with_capacity(frames), Vec::with_capacity(frames));
            for i in 0..frames + 20 {
                let t = Instant::now();
                let gpu_ms = target
                    .render_timed(&gpu, renderer.clear_color(), |frame| {
                        renderer
                            .draw(frame, &textures, &camera, i as f32 / 60.0, depth_slot)
                            .unwrap()
                    })
                    .unwrap();
                if i >= 20 {
                    times.push(t.elapsed().as_secs_f64() * 1000.0);
                    gpu_times.push(gpu_ms);
                }
            }
            times.sort_by(f64::total_cmp);
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            let p99 = times[times.len() * 99 / 100];
            let gpu_mean = gpu_times.iter().sum::<f64>() / gpu_times.len() as f64;
            println!(
                "{:16} {mean:9.2} {p99:9.2} {:9.0} {gpu_mean:9.2}",
                l[1],
                1000.0 / mean
            );
        }
    }
}
