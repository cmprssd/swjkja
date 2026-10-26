//! Renders views of maps with the stage renderer (materials drawn as the original
//! does) to PNGs.
//! Usage: render_world <GameData> <views.txt> <out folder>
//! Each view renders at its game time (the views file's ninth column, in ms), NPCs
//! standing as they do then. A map's first view may give, in its eleventh column,
//! names the game uses as it runs, as "name@ms" separated by commas (the capture's
//! `g_swjkjaUse`).
//! Each view is drawn [`HELD_FRAMES`] times in a row and the last frame saved, as
//! the capture holds a view still before its screenshot: on a map without a global
//! fog the colour is not cleared between frames (`WorldRenderer::clear_color`).
//! Rain is not drawn (`WorldRenderer::draw_weather`), as the captures draw none.
use std::time::Instant;

use swjkja_render::{Camera, Gpu, Target, TextureSet, WorldRenderer};

/// Frames each view is drawn before it is saved. The capture holds a view 200
/// frames (`wait 200`); a blend over what the last frame left settles within these.
const HELD_FRAMES: usize = 10;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (gamedata, views_file, out_dir) = (&args[0], &args[1], &args[2]);
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
        let first = lines.iter().find(|l| l[0] == map).unwrap();
        let uses: Vec<swjkja_world::script::Use> = first
            .get(10)
            .into_iter()
            .flat_map(|u| u.split(','))
            .map(|u| {
                let (name, at) = u.split_once('@').expect("a use is name@ms");
                swjkja_world::script::Use {
                    name: name.to_owned(),
                    at: at.parse().unwrap(),
                }
            })
            .collect();
        let mut renderer = WorldRenderer::with_uses(
            &gpu,
            &mut textures,
            &vfs,
            &shaders,
            &bsp,
            &world,
            &format!("maps/{map}.bsp"),
            // NPCs stand as they do at the map's first view's time.
            first
                .get(8)
                .map_or(0.0, |ms| ms.parse::<f32>().unwrap() / 1000.0),
            &uses,
        )
        .unwrap();
        // World effects (rain) are left out, as the reference captures leave them
        // out: where their particles fall cannot be reproduced.
        renderer.draw_weather = false;
        let load = t.elapsed();
        // One target per map; its depth copy sits in the texture array for flares.
        let target = Target::new(&gpu, 1280, 720).unwrap();
        let depth_slot = textures.add_view(target.depth_copy.view);
        for l in lines.iter().filter(|l| l[0] == map) {
            let f: Vec<f32> = l[2..8].iter().map(|v| v.parse().unwrap()).collect();
            let camera = Camera::new([f[0], f[1], f[2]], [f[3], f[4], f[5]], 1280, 720);
            let time = l
                .get(8)
                .map_or(0.0, |ms| ms.parse::<f32>().unwrap() / 1000.0);
            let t = Instant::now();
            for _ in 0..HELD_FRAMES {
                target
                    .render(&gpu, renderer.clear_color(), |frame| {
                        renderer
                            .draw(frame, &textures, &camera, time, depth_slot)
                            .unwrap()
                    })
                    .unwrap();
            }
            let draw = t.elapsed() / HELD_FRAMES as u32;
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
