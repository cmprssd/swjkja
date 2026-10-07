//! Builds every map's world and prints the slowest; with a map name, times just that.
//! Usage: build_all <GameData folder> [maps/mp/ffa3.bsp]
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let gamedata = args.next().expect("usage: build_all <GameData> [map]");
    let only = args.next();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let mut maps: Vec<&str> = vfs.files().filter(|f| f.ends_with(".bsp")).collect();
    maps.sort_unstable();
    let mut times = Vec::new();
    for name in maps
        .into_iter()
        .filter(|m| only.as_deref().is_none_or(|o| o == *m))
    {
        let bsp = swjkja_bsp::Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let t = Instant::now();
        let world = swjkja_world::World::load(&bsp, &Default::default());
        times.push((t.elapsed(), name, world.stitches));
    }
    let total: std::time::Duration = times.iter().map(|t| t.0).sum();
    times.sort_unstable_by(|a, b| b.cmp(a));
    println!(
        "{} maps in {total:?}; slowest: {:?}",
        times.len(),
        &times[..times.len().min(3)]
    );
}
