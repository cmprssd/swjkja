//! Prints a map's fogs, how many surfaces use each, surface kinds and entity classes.
//! Usage: fogs <GameData folder> <map>
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let vfs = swjkja_fs::Vfs::mount_game(&[args[0].clone().into()], &["base"]).unwrap();
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
