//! Prints a map's fogs, how many surfaces use each, surface kinds and entity classes;
//! without a map, lists the maps with fog volumes (fogs with a brush).
//! Usage: fogs <GameData folder> [maps/<name>.bsp]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let vfs = swjkja_fs::Vfs::mount_game(&[args[0].clone().into()], &["base"]).unwrap();
    if args.len() < 2 {
        let mut maps: Vec<&str> = vfs.files().filter(|f| f.ends_with(".bsp")).collect();
        maps.sort_unstable();
        for name in maps {
            let bsp = swjkja_bsp::Bsp::parse(&vfs.read(name).unwrap()).unwrap();
            let volumes: Vec<String> = bsp
                .fogs
                .iter()
                .filter(|f| f.brush >= 0)
                .map(|f| {
                    // R_LoadFogs's bounds: brushes list their six axial sides first.
                    let first = bsp.brushes[f.brush as usize].first_side as usize;
                    let d: Vec<f32> = (0..6)
                        .map(|k| bsp.planes[bsp.brush_sides[first + k].plane as usize].dist)
                        .collect();
                    let (lo, hi) = ([-d[0], -d[2], -d[4]], [d[1], d[3], d[5]]);
                    format!("{} {lo:?} to {hi:?}", f.shader())
                })
                .collect();
            if !volumes.is_empty() {
                println!("{name}: {}", volumes.join(", "));
            }
        }
        return;
    }
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read(&args[1]).unwrap()).unwrap();
    for (i, f) in bsp.fogs.iter().enumerate() {
        println!(
            "fog {i}: {} brush {} side {}",
            f.shader(),
            f.brush,
            f.visible_side
        );
    }
    let mut counts = std::collections::BTreeMap::new();
    for s in &bsp.surfaces {
        *counts.entry(s.fog).or_insert(0) += 1;
    }
    println!("surfaces per fog number: {counts:?}");
    let mut kinds = std::collections::BTreeMap::new();
    for s in &bsp.surfaces {
        *kinds.entry(format!("{:?}", s.kind())).or_insert(0) += 1;
    }
    println!("surface kinds: {kinds:?}");
    let mut classes = std::collections::BTreeMap::new();
    for e in &bsp.entities {
        *classes
            .entry(e.get("classname").unwrap_or("?").to_owned())
            .or_insert(0) += 1;
    }
    println!("entities: {classes:?}");
    if let Some(w) = bsp.worldspawn() {
        println!(
            "worldspawn fog keys: {:?}",
            w.pairs
                .iter()
                .filter(|(k, _)| k.to_lowercase().contains("fog"))
                .collect::<Vec<_>>()
        );
    }
}
