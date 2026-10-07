//! Builds the world of every retail map. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use swjkja_bsp::{Bsp, SurfaceKind};
use swjkja_world::{Geometry, Options, World};

#[test]
fn every_retail_map_builds() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let mut maps: Vec<&str> = vfs.files().filter(|f| f.ends_with(".bsp")).collect();
    maps.sort_unstable();
    let mut build = Duration::ZERO;
    for name in &maps {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let start = Instant::now();
        let world = World::load(&bsp, &Options::default());
        build += start.elapsed();
        assert_eq!(world.surfaces.len(), bsp.surfaces.len(), "{name}");
        for s in &world.surfaces {
            if let Geometry::Grid(g) = &s.geometry {
                assert_eq!(g.verts.len(), g.width * g.height, "{name}");
                assert!(
                    g.width >= 2 && g.height >= 2 && g.width <= 65 && g.height <= 65,
                    "{name}"
                );
            }
        }
    }
    eprintln!("built {} worlds in {build:?}", maps.len());

    // mp/ffa3: OpenJK logged "4426 faces, 526 meshes, 88 trisurfs, 0 flares".
    let bsp = Bsp::parse(&vfs.read("maps/mp/ffa3.bsp").unwrap()).unwrap();
    let world = World::load(&bsp, &Options::default());
    let count =
        |f: &dyn Fn(&Geometry) -> bool| world.surfaces.iter().filter(|s| f(&s.geometry)).count();
    assert_eq!(count(&|g| matches!(g, Geometry::Face { .. })), 4426);
    assert_eq!(count(&|g| matches!(g, Geometry::Triangles { .. })), 88);
    let patches = bsp
        .surfaces
        .iter()
        .filter(|s| s.kind() == SurfaceKind::Patch)
        .count();
    let skipped_patches = world
        .surfaces
        .iter()
        .zip(&bsp.surfaces)
        .filter(|(w, b)| b.kind() == SurfaceKind::Patch && matches!(w.geometry, Geometry::Skip))
        .count();
    assert_eq!(patches, 526);
    assert_eq!(
        count(&|g| matches!(g, Geometry::Grid(_))) + skipped_patches,
        526
    );
    eprintln!(
        "ffa3: {} grids ({skipped_patches} nodraw patches skipped), {} stitches",
        526 - skipped_patches,
        world.stitches
    );
}
