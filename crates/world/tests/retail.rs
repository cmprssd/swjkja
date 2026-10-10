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

#[test]
fn every_retail_map_has_a_light_grid_and_spawns_its_static_models() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let (mut total, mut lit, mut entity_models) = (0, 0, 0);
    for name in vfs.files().filter(|f| f.ends_with(".bsp")) {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let grid = swjkja_world::light_grid::LightGrid::load(&bsp, 0);
        let styles = swjkja_world::style::LightStyles::spawn(&bsp.entities).colors(0);
        assert!(grid.exists(), "{name}: no light grid");
        let mp = swjkja_world::static_model::Rules::Multiplayer { gametype: 0 };
        let models = swjkja_world::static_model::static_models(&bsp.entities, mp);
        if name == "maps/mp/ffa5.bsp" {
            assert_eq!(models.len(), 84);
        }
        for m in &models {
            let light = grid.entity_light(m.origin, &styles, [0.0; 3]);
            lit += usize::from(light.ambient.iter().any(|&a| a > 32.0));
        }
        total += models.len();
        let rules = swjkja_world::static_model::Rules::for_map(name);
        entity_models += swjkja_world::static_model::entity_models(&bsp.entities, rules).len();
    }
    // All on single-player maps: 490 breakables, of which 6 start off and 4 fall to
    // the floor (left out for now); 106 racks, whose ammo racks place 209 ammo packs
    // and medpacks (counted independently with Python); 130 rack weapons, three on
    // each of 38 gun racks and one on each of 16 ammo racks (counted the same way);
    // 10 beacons and planted bombs, all waiting to be used.
    assert_eq!(entity_models, 480 + 106 + 209 + 130);
    // Counted independently with Python: every misc_model_static with a model spawns
    // in free-for-all.
    assert_eq!(total, 6684);
    eprintln!("{lit} of {total} static models get more than the minimum ambient light");
    assert!(lit > total / 2, "{lit} of {total}");
}
