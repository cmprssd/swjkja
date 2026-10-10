//! Frame times along a camera path recorded from the reference engine (a timedemo
//! with `cg_swjkjaPath`, see bench/path-reference.sh), so both engines draw the same
//! views at the same game times. A frame's time is the interval between the same
//! point in consecutive frames, as the reference measures it.
//!
//!     bench_path <GameData> <map> <path> [runs] [out]   render the path with the new renderer
//!     bench_path --stats <path>...                       the times in recorded paths
//!
//! With `out`, each run is also written as `<out>-<run>.txt`: the path with this
//! renderer's timestamps, in the reference's format.
//!
//! Frames go through a [`FrameLoop`] (the CPU records a frame while the GPU draws
//! the previous one) and are drawn offscreen (no window, no present); the reference
//! draws to a window with vsync off.
use std::time::Instant;

use swjkja_render::path::{self, PathFrame};
use swjkja_render::{FrameLoop, Gpu, Target, TextureSet, WorldRenderer};

/// Frame times in ms from timestamps in µs taken at the same point of every frame.
fn intervals_ms(usec: &[i64]) -> Vec<f64> {
    usec.windows(2)
        .map(|w| (w[1] - w[0]) as f64 / 1000.0)
        .collect()
}

#[derive(Debug, PartialEq)]
struct Stats {
    frames: usize,
    /// Frames divided by the total time.
    fps: f64,
    median_ms: f64,
    p99_ms: f64,
    /// "1% low": 1000 / the mean of the slowest 1% of frames (at least one).
    low1_fps: f64,
    max_ms: f64,
}

fn stats(times_ms: &[f64]) -> Stats {
    let mut t = times_ms.to_vec();
    t.sort_by(f64::total_cmp);
    let n = t.len();
    let slow = &t[n - n.div_ceil(100)..];
    Stats {
        frames: n,
        fps: n as f64 * 1000.0 / t.iter().sum::<f64>(),
        median_ms: if n % 2 == 1 {
            t[n / 2]
        } else {
            (t[n / 2 - 1] + t[n / 2]) / 2.0
        },
        p99_ms: t[(n * 99).div_ceil(100) - 1],
        low1_fps: 1000.0 * slow.len() as f64 / slow.iter().sum::<f64>(),
        max_ms: t[n - 1],
    }
}

fn print_header() {
    println!(
        "{:24} {:>6} {:>8} {:>10} {:>8} {:>10} {:>8}",
        "run", "frames", "fps", "median ms", "p99 ms", "1% low fps", "max ms"
    );
}

fn print_row(name: &str, s: &Stats) {
    println!(
        "{name:24} {:6} {:8.1} {:10.2} {:8.2} {:10.1} {:8.2}",
        s.frames, s.fps, s.median_ms, s.p99_ms, s.low1_fps, s.max_ms
    );
}

fn read_path(file: &str) -> Vec<PathFrame> {
    let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{file}: {e}"));
    let path = path::parse(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
    assert!(path.len() >= 2, "{file}: needs at least two frames");
    path
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--stats") {
        print_header();
        for file in &args[1..] {
            let usec: Vec<i64> = read_path(file).iter().map(|f| f.usec).collect();
            print_row(file, &stats(&intervals_ms(&usec)));
        }
        return;
    }
    let [game_data, map, file, rest @ ..] = &args[..] else {
        panic!("usage: bench_path <GameData> <map> <path> [runs] [out] | --stats <path>...");
    };
    let runs: usize = rest.first().map_or(3, |r| r.parse().unwrap());
    let out = rest.get(1);
    let path = read_path(file);

    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[game_data.into()], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read(&format!("maps/{map}.bsp")).unwrap()).unwrap();
    let world = swjkja_world::World::load(&bsp, &Default::default());
    let mut textures = TextureSet::new(&gpu).unwrap();
    let mut renderer =
        WorldRenderer::new(&gpu, &mut textures, &vfs, &shaders, &bsp, &world).unwrap();
    let target = Target::new(&gpu, 1280, 720).unwrap();
    let depth_slot = textures.add_view(target.depth_copy.view);

    println!("{map}, {} frames, {}, 1280x720", path.len(), gpu.name);
    print_header();
    // Run 0 warms caches and the driver and is not reported, like the reference's
    // cold run.
    let mut frames = FrameLoop::new(&gpu).unwrap();
    for run in 0..=runs {
        let mut usec = Vec::with_capacity(path.len());
        let mut gpu_ms = Vec::with_capacity(path.len());
        let start = Instant::now();
        for f in &path {
            usec.push(start.elapsed().as_micros() as i64);
            let camera = f.camera(1280, 720);
            let time = f.time_ms as f32 / 1000.0;
            gpu_ms.extend(
                frames
                    .render(&target, renderer.clear_color(), |frame| {
                        renderer
                            .draw(frame, &textures, &camera, time, depth_slot)
                            .unwrap()
                    })
                    .unwrap(),
            );
        }
        gpu_ms.extend(frames.finish().unwrap());
        if let (Some(out), true) = (out, run > 0) {
            std::fs::write(format!("{out}-{run}.txt"), path::write(&path, &usec)).unwrap();
        }
        if run > 0 {
            let gpu_mean = gpu_ms.iter().sum::<f64>() / gpu_ms.len() as f64;
            print_row(
                &format!("run {run} (GPU {gpu_mean:.2} ms)"),
                &stats(&intervals_ms(&usec)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals() {
        assert_eq!(intervals_ms(&[1000, 3500, 4000]), vec![2.5, 0.5]);
    }

    #[test]
    fn frame_stats() {
        // 199 frames of 5 ms and one of 25 ms: 1% of 200 is the two slowest.
        let mut t = vec![5.0; 199];
        t.push(25.0);
        assert_eq!(
            stats(&t),
            Stats {
                frames: 200,
                fps: 200.0 * 1000.0 / 1020.0,
                median_ms: 5.0,
                p99_ms: 5.0,
                low1_fps: 1000.0 * 2.0 / 30.0,
                max_ms: 25.0,
            }
        );
        // With fewer than 100 frames, the 1% low is the slowest frame.
        assert_eq!(stats(&[2.0, 4.0, 8.0]).low1_fps, 125.0);
        assert_eq!(stats(&[2.0, 4.0, 8.0]).median_ms, 4.0);
        assert_eq!(stats(&[2.0, 4.0]).median_ms, 3.0);
    }
}
