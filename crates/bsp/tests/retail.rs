//! Parses every map in the retail data. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use swjkja_bsp::{Bsp, SurfaceKind};
use swjkja_fs::Vfs;

fn surface_counts(bsp: &Bsp) -> [usize; 4] {
    let count = |k| bsp.surfaces.iter().filter(|s| s.kind() == k).count();
    [
        count(SurfaceKind::Planar),
        count(SurfaceKind::Patch),
        count(SurfaceKind::TriangleSoup),
        count(SurfaceKind::Flare),
    ]
}

#[test]
fn every_retail_map_parses() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let mut maps: Vec<&str> = vfs.files().filter(|f| f.ends_with(".bsp")).collect();
    maps.sort_unstable();

    let mut parse_time = Duration::ZERO;
    for name in &maps {
        let data = vfs.read(name).unwrap();
        let start = Instant::now();
        let bsp = Bsp::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        parse_time += start.elapsed();
        assert!(bsp.worldspawn().is_some(), "{name}: no worldspawn entity");
    }
    eprintln!("parsed {} maps in {parse_time:?}", maps.len());
    // Counted independently with .NET's ZipFile: 57 unique .bsp names, 23 of them mp/.
    assert_eq!(maps.len(), 57);

    // OpenJK's renderer logged "...loaded 4426 faces, 526 meshes, 88 trisurfs,
    // 0 flares" when the baseline benchmark loaded this map.
    let ffa3 = Bsp::parse(&vfs.read("maps/mp/ffa3.bsp").unwrap()).unwrap();
    assert_eq!(surface_counts(&ffa3), [4426, 526, 88, 0]);
}
