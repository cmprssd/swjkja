//! Builds the world of every retail map. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use swjkja_bsp::{Bsp, SurfaceKind};
use swjkja_world::collision::Collision;
use swjkja_world::static_model::Rules;
use swjkja_world::{Geometry, Options, World, npc};

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
        entity_models += swjkja_world::static_model::entity_models(
            &bsp.entities,
            rules,
            &mut Collision::at_spawn(&bsp, rules),
        )
        .len();
    }
    // All on single-player maps: 490 breakables, of which 6 start off (4 fall to the
    // floor); 106 racks, whose ammo racks place 209 ammo packs
    // and medpacks (counted independently with Python); 130 rack weapons, three on
    // each of 38 gun racks and one on each of 16 ammo racks (counted the same way);
    // 10 beacons and planted bombs, all waiting to be used.
    assert_eq!(entity_models, 484 + 106 + 209 + 130);
    // Counted independently with Python: every misc_model_static with a model spawns
    // in free-for-all.
    assert_eq!(total, 6684);
    eprintln!("{lit} of {total} static models get more than the minimum ambient light");
    assert!(lit > total / 2, "{lit} of {total}");
}

#[test]
fn single_player_npcs_find_their_looks() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let text = |folder: &str, ext: &str| {
        let files: Vec<Vec<u8>> = vfs
            .list(folder, ext)
            .iter()
            .map(|f| vfs.read(&format!("{folder}/{f}")).unwrap())
            .collect();
        npc::join_parms(files.iter().map(Vec::as_slice))
    };
    let (parms, sabers) = (
        text("ext_data/npcs", ".npc"),
        text("ext_data/sabers", ".sab"),
    );
    let mut maps: Vec<&str> = vfs
        .files()
        .filter(|f| f.ends_with(".bsp") && !f.starts_with("maps/mp/"))
        .collect();
    maps.sort_unstable();
    let (mut spawns, mut drawn) = (0, 0);
    let mut not_drawn = std::collections::BTreeSet::new();
    let mut poses = std::collections::BTreeMap::new();
    let mut held = std::collections::BTreeMap::new();
    for name in &maps {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let rules = Rules::for_map(name);
        for n in npc::npc_spawns(&bsp.entities, rules) {
            spawns += 1;
            match npc::npc_look(&parms, &n.npc_type, n.message) {
                Some(look) => {
                    drawn += 1;
                    let folder = format!("models/players/{}/", look.model);
                    assert!(
                        vfs.contains(&format!("{folder}model.glm")),
                        "{name}: {look:?}"
                    );
                }
                None => {
                    not_drawn.insert(n.npc_type.to_ascii_lowercase());
                }
            }
        }
        for m in npc::npc_models(
            &bsp.entities,
            rules,
            &parms,
            &sabers,
            &Collision::at_spawn(&bsp, rules),
        ) {
            let g = m.ghoul2.unwrap();
            let pose = g.pose.unwrap();
            *poses.entry((pose.legs, pose.torso)).or_insert(0) += 1;
            for a in g.attached {
                *held.entry(a.model).or_insert(0) += 1;
            }
        }
    }
    eprintln!("poses: {poses:?}");
    eprintln!("held: {held:?}");
    eprintln!("{spawns} NPCs spawn at load, {drawn} drawn; not drawn: {not_drawn:?}");
    // Counted independently with Python: 267 spawners without a targetname, less 21
    // vehicles, 1 left out at the easiest skill, and 0 typeless spawners. The two
    // mice (t1_surprise, t3_byss), MD3 droids in the original, are not drawn.
    assert_eq!((spawns, drawn), (245, 243));
    assert_eq!(not_drawn.iter().collect::<Vec<_>>(), ["mouse"]);
}

#[test]
fn every_retail_map_places_its_brush_entities() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let (mut sp, mut mp, mut moved, mut turned, mut held) = (0, 0, 0, 0, 0);
    for name in vfs.files().filter(|f| f.ends_with(".bsp")) {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let rules = Rules::for_map(name);
        let brushes = swjkja_world::brush_entity::brush_entities(&bsp.entities, &bsp.models, rules);
        for b in &brushes {
            let spawned = bsp.entities.iter().find(|e| {
                e.get("model")
                    .is_some_and(|m| m.strip_prefix('*') == Some(&b.model.to_string()))
            });
            let origin = spawned.and_then(|e| e.get("origin")).map_or([0.0; 3], |o| {
                let v: Vec<f32> = o.split_whitespace().map(|x| x.parse().unwrap()).collect();
                [v[0], v[1], v[2]]
            });
            moved += usize::from(b.origin != origin);
            turned += usize::from(b.axes[0] != [1.0, 0.0, 0.0]);
            held += usize::from(b.shader_anim);
        }
        match rules {
            Rules::SinglePlayer { .. } => sp += brushes.len(),
            Rules::Multiplayer { .. } => mp += brushes.len(),
        }
    }
    // Every func_ entity of the single-player maps with a model, less the 46 walls
    // and 101 usables that start off (counted from the entity text alone).
    assert_eq!(sp, 1509);
    assert_eq!(mp, 356);
    // Doors that start open (spawnflag 1) and plats (which start at the bottom).
    assert_eq!(moved, 54 + 20);
    // The func_statics with an angle.
    assert_eq!(turned, 10);
    // Locked doors and func_statics and usables with frame-controlled shaders.
    assert_eq!(held, 122);
}
