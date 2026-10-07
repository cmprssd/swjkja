//! Renders a preview of a map to a PNG: each surface's first texture times its
//! lightmap, from the first spawn point or a given view.
//! Usage: render_preview <GameData> <map> <out.png> [width height [x y z pitch yaw roll]]
use std::collections::HashMap;
use std::time::Instant;

use swjkja_render::{Camera, Gpu, Light, Target, Texture, TextureSet, WorldPreview};
use swjkja_shader::{ShaderText, Texture as StageTexture};
use swjkja_world::{LIGHTMAP_BY_VERTEX, Options, World};

/// The image a surface's shader shows first: its first stage with a texture, or the
/// shader name itself when no script defines it (the original's implicit shader).
fn base_image(shaders: &ShaderText, name: &str) -> Option<String> {
    match shaders.parse(name) {
        Some(Ok(def)) => def.stages.iter().find_map(|s| match &s.texture {
            StageTexture::Map { name, .. } => Some(name.clone()),
            StageTexture::Animated { names, .. } => names.first().cloned(),
            _ => None,
        }),
        Some(Err(_)) | None => Some(name.to_owned()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [gamedata, map, out] = [&args[0], &args[1], &args[2]];
    let num = |i: usize, default: f32| args.get(i).map_or(default, |a| a.parse().unwrap());
    let (width, height) = (num(3, 1280.0) as u32, num(4, 720.0) as u32);

    let t = Instant::now();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read(map).unwrap()).unwrap();
    let world = World::load(&bsp, &Options::default());
    let shaders = ShaderText::load(&vfs).unwrap();

    let camera = if args.len() >= 11 {
        Camera::new(
            [num(5, 0.0), num(6, 0.0), num(7, 0.0)],
            [num(8, 0.0), num(9, 0.0), num(10, 0.0)],
            width,
            height,
        )
    } else {
        let spawn = bsp
            .entities
            .iter()
            .find(|e| {
                matches!(
                    e.get("classname"),
                    Some("info_player_deathmatch" | "info_player_start")
                )
            })
            .expect("a spawn point");
        let o: Vec<f32> = spawn
            .get("origin")
            .unwrap()
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        let yaw = spawn.get("angle").map_or(0.0, |a| a.parse().unwrap());
        Camera::new([o[0], o[1], o[2] + 36.0], [0.0, yaw, 0.0], width, height)
    };

    let gpu = Gpu::new_headless().unwrap();
    let mut textures = TextureSet::new(&gpu).unwrap();
    let white = textures.add(Texture::new(&gpu, 1, 1, &[255; 4], false).unwrap());
    let lightmaps: Vec<u32> = bsp
        .lightmaps
        .chunks(128 * 128 * 3)
        .map(|page| {
            let rgba: Vec<u8> = page
                .chunks(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect();
            textures.add(Texture::new(&gpu, 128, 128, &rgba, false).unwrap())
        })
        .collect();
    let mut cache: HashMap<String, u32> = HashMap::new();
    let (mut missing, mut loaded) = (0, 0);
    let looks: Vec<_> = world
        .surfaces
        .iter()
        .map(|s| {
            let shader = &bsp.shaders[s.shader];
            const SKY: i32 = 0x2000;
            const NODRAW: i32 = 0x20_0000;
            if shader.surface_flags & (SKY | NODRAW) != 0 {
                return None;
            }
            let texture =
                match base_image(&shaders, shader.name()) {
                    Some(name) => *cache.entry(name.clone()).or_insert_with(|| {
                        match swjkja_image::load(&vfs, &name) {
                            Ok(l) => {
                                loaded += 1;
                                textures.add(
                                    Texture::new(
                                        &gpu,
                                        l.image.width,
                                        l.image.height,
                                        &l.image.rgba,
                                        true,
                                    )
                                    .unwrap(),
                                )
                            }
                            Err(_) => {
                                missing += 1;
                                white
                            }
                        }
                    }),
                    None => white,
                };
            let light = match s.lightmaps[0] {
                n if n >= 0 && (n as usize) < lightmaps.len() => {
                    Light::Lightmap(lightmaps[n as usize])
                }
                LIGHTMAP_BY_VERTEX => Light::Vertex,
                _ => Light::None,
            };
            Some((texture, light))
        })
        .collect();
    let preview = WorldPreview::new(&gpu, &textures, &world, &looks).unwrap();
    let load = t.elapsed();

    let target = Target::new(&gpu, width, height).unwrap();
    let t = Instant::now();
    target
        .render(&gpu, [0.0, 0.0, 0.0, 1.0], |cmd| {
            preview.draw(&gpu, cmd, &textures, &camera)
        })
        .unwrap();
    let draw = t.elapsed();
    let rgba = target.read_rgba(&gpu).unwrap();

    let file = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
    let mut enc = png::Encoder::new(file, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    println!(
        "{map}: {} triangles, {loaded} textures ({missing} missing), load {load:?}, render {draw:?} on {}; camera {:?} {:?}",
        preview.triangles(),
        gpu.name,
        camera.origin,
        camera.angles
    );
}
