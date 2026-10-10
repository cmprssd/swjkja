//! Times each phase of loading maps for rendering: mounting the archives, indexing
//! the shader scripts, parsing the map, building surfaces, and resolving materials,
//! decoding and uploading textures (WorldRenderer::new). Each map loads `runs` times;
//! the first run reads files cold or warm depending on the OS cache.
//! Usage: load_profile <GameData> <runs> <map>...
use std::time::Instant;

use swjkja_render::{Gpu, TextureSet, WorldRenderer};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let runs: usize = args[1].parse().unwrap();
    let gpu = Gpu::new_headless().unwrap();
    println!(
        "{:10} {:>8} {:>8} {:>8} {:>8} {:>9} {:>8}",
        "map", "mount", "shaders", "bsp", "world", "renderer", "total"
    );
    for map in &args[2..] {
        for _ in 0..runs {
            let t = Instant::now();
            let vfs = swjkja_fs::Vfs::mount_game(&[args[0].clone().into()], &["base"]).unwrap();
            let mount = t.elapsed();
            let t = Instant::now();
            let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
            let shader_time = t.elapsed();
            let t = Instant::now();
            let bsp =
                swjkja_bsp::Bsp::parse(&vfs.read(&format!("maps/{map}.bsp")).unwrap()).unwrap();
            let bsp_time = t.elapsed();
            let t = Instant::now();
            let world = swjkja_world::World::load(&bsp, &Default::default());
            let world_time = t.elapsed();
            let t = Instant::now();
            let mut textures = TextureSet::new(&gpu).unwrap();
            let renderer =
                WorldRenderer::new(&gpu, &mut textures, &vfs, &shaders, &bsp, &world).unwrap();
            let renderer_time = t.elapsed();
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
            println!(
                "{map:10} {:8.0} {:8.0} {:8.0} {:8.0} {:9.0} {:8.0}  ms, {} textures",
                ms(mount),
                ms(shader_time),
                ms(bsp_time),
                ms(world_time),
                ms(renderer_time),
                ms(mount + shader_time + bsp_time + world_time + renderer_time),
                textures.len()
            );
            drop(renderer);
        }
    }
}
