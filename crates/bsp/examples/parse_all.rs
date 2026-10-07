//! Parses every map in a game install and reports the time per map.
//! Usage: parse_all <GameData folder>
use std::time::{Duration, Instant};

fn main() {
    let gamedata = std::env::args()
        .nth(1)
        .expect("usage: parse_all <GameData>");
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let mut maps: Vec<&str> = vfs.files().filter(|f| f.ends_with(".bsp")).collect();
    maps.sort_unstable();
    let (mut read, mut parse, mut bytes) = (Duration::ZERO, Duration::ZERO, 0);
    for name in &maps {
        let t = Instant::now();
        let data = vfs.read(name).unwrap();
        read += t.elapsed();
        bytes += data.len();
        let t = Instant::now();
        swjkja_bsp::Bsp::parse(&data).unwrap();
        parse += t.elapsed();
    }
    println!(
        "{} maps, {} MB: read {read:?}, parse {parse:?} ({:.1} ms per map)",
        maps.len(),
        bytes / 1_000_000,
        parse.as_secs_f64() * 1000.0 / maps.len() as f64
    );
}
