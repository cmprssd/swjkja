//! Renders views of maps with the stage renderer (materials drawn as the original
//! does) to PNGs.
//! Usage: render_world <GameData> <views.txt> <out folder> [time in seconds]
use std::time::Instant;

use swjkja_render::{Camera, Gpu, Target, TextureSet, WorldRenderer};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (gamedata, views_file, out_dir) = (&args[0], &args[1], &args[2]);
    let time: f32 = args.get(3).map_or(0.0, |t| t.parse().unwrap());
    let text = std::fs::read_to_string(views_file).unwrap();
    let lines: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().collect())
        .collect();
    let mut maps: Vec<&str> = lines.iter().map(|l| l[0]).collect();
    maps.dedup();

    let gpu = Gpu::new_headless().unwrap();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    for map in maps {
        let t = Instant::now();
        let bsp = swjkja_bsp::Bsp::parse(&vfs.read(&format!("maps/{map}.bsp")).unwrap()).unwrap();
        let world = swjkja_world::World::load(&bsp, &Default::default());
        let mut textures = TextureSet::new(&gpu).unwrap();
        let mut renderer =
            WorldRenderer::new(&gpu, &mut textures, &vfs, &shaders, &bsp, &world).unwrap();
        let load = t.elapsed();
        // One target per map; its depth copy sits in the texture array for flares.
        let target = Target::new(&gpu, 1280, 720).unwrap();
        let depth_slot = textures.add_view(target.depth_copy.view);
        for l in lines.iter().filter(|l| l[0] == map) {
            let f: Vec<f32> = l[2..8].iter().map(|v| v.parse().unwrap()).collect();
            let camera = Camera::new([f[0], f[1], f[2]], [f[3], f[4], f[5]], 1280, 720);
            let t = Instant::now();
            target
                .render(&gpu, renderer.clear_color(), |frame| {
                    renderer
                        .draw(frame, &textures, &camera, time, depth_slot)
                        .unwrap()
                })
                .unwrap();
            let draw = t.elapsed();
            let rgba = target.read_rgba(&gpu).unwrap();
            let out = std::path::Path::new(out_dir).join(format!("{}.png", l[1]));
            let mut enc = png::Encoder::new(
                std::io::BufWriter::new(std::fs::File::create(&out).unwrap()),
                1280,
                720,
            );
            enc.set_color(png::ColorType::Rgba);
            enc.write_header().unwrap().write_image_data(&rgba).unwrap();
            println!(
                "{map} -> {}: {} materials, {} triangles, load {load:?}, render {draw:?}",
                out.display(),
                renderer.material_count,
                renderer.triangles
            );
        }
    }
}
