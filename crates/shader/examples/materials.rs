//! Resolves every surface material of a map and summarizes them.
//! Usage: materials <GameData folder> <map>
use std::collections::BTreeMap;

use swjkja_shader::resolve::{Material, resolve_map_shader};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let vfs = swjkja_fs::Vfs::mount_game(&[args[0].clone().into()], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read(&args[1]).unwrap()).unwrap();
    let text = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let mut cache = std::collections::HashMap::new();
    let mut exists = |n: &str| {
        *cache
            .entry(n.to_owned())
            .or_insert_with(|| swjkja_image::load(&vfs, n).is_ok())
    };
    let mut seen: BTreeMap<String, (usize, Material)> = BTreeMap::new();
    for s in &bsp.surfaces {
        let shader = &bsp.shaders[s.shader as usize];
        let styles = if s.lightmap_num[0] == -3 {
            s.vertex_styles
        } else {
            s.lightmap_styles
        };
        let m = resolve_map_shader(
            &text,
            shader.name(),
            s.lightmap_num,
            styles,
            bsp.lightmap_pages(),
            &mut exists,
            &mut |_| false,
        );
        let key = if m.is_default {
            format!("{} (from {})", m.name, shader.name())
        } else {
            format!("{} {:?}", m.name, m.lightmaps)
        };
        seen.entry(key).or_insert((0, m)).0 += 1;
    }
    let (mut default, mut collapsed, mut multi) = (0, 0, 0);
    for (key, (count, m)) in &seen {
        if m.is_default {
            println!("default: {key}");
        }
        default += usize::from(m.is_default);
        collapsed += usize::from(m.multitexture.is_some());
        if m.stages.len() > 1 {
            multi += 1;
            let blends: Vec<String> = m
                .stages
                .iter()
                .map(|s| format!("{:?}/{:?} {:?}", s.blend_src, s.blend_dst, s.rgb_gen))
                .collect();
            println!("{count:5} {key}: sort {} | {}", m.sort, blends.join(" ; "));
        }
    }
    println!(
        "{} materials: {default} default, {collapsed} collapsed to one pass, {multi} with several passes",
        seen.len()
    );
}
