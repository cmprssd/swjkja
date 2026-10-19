//! Builds the world of every retail map. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use swjkja_bsp::{Bsp, SurfaceKind};
use swjkja_world::collision::Collision;
use swjkja_world::rand::Rand;
use swjkja_world::static_model::Rules;
use swjkja_world::{Geometry, Options, World, npc, script};

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
            let light = grid.entity_light(m.origin, &styles, [0.0; 3], false);
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
    let script_effects = std::cell::RefCell::new(std::collections::BTreeMap::new());
    let mut scripted = 0;
    for name in &maps {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let rules = Rules::for_map(name);
        let mut rand = Rand::default();
        for n in npc::npc_spawns(&bsp.entities, rules, &sabers, &mut rand) {
            spawns += 1;
            match npc::npc_look(&parms, &sabers, &n.npc_type, n.message, &mut rand) {
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
        let scripts = |name: &str| {
            let bytes = vfs.read(&script::script_path(name)?).ok()?;
            let nodes = swjkja_icarus::parse(&bytes).unwrap();
            // What the first pass leaves is all a script sets of the NPC's weapon
            // and visibility.
            let first = script::first_pass(&nodes);
            let (mut unarmed, mut invisible) = (None, None);
            sets(&nodes, &mut |name, value| match name {
                "SET_WEAPON" => unarmed = Some(value.eq_ignore_ascii_case("WP_NONE")),
                "SET_INVISIBLE" => invisible = Some(value.eq_ignore_ascii_case("true")),
                _ => {}
            });
            assert_eq!(unarmed.unwrap_or(false), first.unarmed, "{name}");
            assert_eq!(invisible.unwrap_or(false), first.invisible, "{name}");
            *script_effects
                .borrow_mut()
                .entry((first.unarmed, first.invisible))
                .or_insert(0) += 1;
            Some(nodes)
        };
        let models = npc::npc_models(
            &bsp.entities,
            rules,
            &parms,
            &sabers,
            &Collision::at_spawn(&bsp, rules),
            &scripts,
        );
        for m in models {
            let g = m.ghoul2.unwrap();
            let pose = g.pose.unwrap();
            scripted += pose.script.is_some() as usize;
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
    eprintln!(
        "{scripted} drawn run a spawnscript; (unarmed, invisible): {:?}",
        script_effects.borrow()
    );
    // Counted independently with Python over the spawners' "spawnscript" keys and
    // the scripts' set commands: 104 scripts (none a vehicle's or left out by
    // skill), 70 putting the weapon away, 9 making the NPC invisible (not drawn).
    let effects = script_effects.into_inner();
    assert_eq!(effects.values().sum::<usize>(), 104);
    assert_eq!((effects[&(true, false)], effects[&(false, true)]), (70, 9));
    assert_eq!(scripted, 95);
}

#[test]
fn the_academies_random_jedi_look_as_in_the_original() {
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
    // Logged by the reference capture (g_swjkjaRand): the random Jedi in spawn
    // order, and the colour each draws from its set, by index.
    let jedi = [
        "jedi_kdm2",
        "jedi_hm1",
        "jedi_hf2",
        "jedi_hf2",
        "jedi_zf1",
        "jedi_rm2",
        "jedi_zf1",
        "jedi_hf1",
    ];
    let logged: [(&str, &[usize]); 4] = [
        ("academy1", &[3, 2, 2, 3, 6, 0, 0, 7]),
        ("academy2", &[1, 6, 3, 4, 5, 4, 7]),
        ("academy4", &[7, 6, 2, 3, 6, 0, 7, 3]),
        ("academy6", &[7, 6, 3, 1, 5, 2, 0]),
    ];
    for (map, colors) in logged {
        let bsp = Bsp::parse(&vfs.read(&format!("maps/{map}.bsp")).unwrap()).unwrap();
        let mut rand = Rand::default();
        let spawns = npc::npc_spawns(&bsp.entities, Rules::for_map(map), &sabers, &mut rand);
        let looks: Vec<_> = spawns
            .iter()
            .map(|n| {
                (
                    n,
                    npc::npc_look(&parms, &sabers, &n.npc_type, n.message, &mut rand),
                )
            })
            .filter(|(n, _)| n.npc_type.starts_with("jedi_"))
            .map(|(n, look)| (n.npc_type.as_str(), look.unwrap().color))
            .collect();
        let expected: Vec<_> = jedi
            .iter()
            .zip(colors)
            .map(|(&name, &i)| {
                let set = name.trim_end_matches(char::is_numeric);
                let (_, palette) = npc::PALETTES.iter().find(|(n, _)| *n == set).unwrap();
                let [r, g, b] = palette[i];
                (name, [r, g, b, 255])
            })
            .collect();
        assert_eq!(looks, expected, "{map}");
    }
}

#[test]
fn single_player_look_targets_name_npcs() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let mut maps: Vec<&str> = vfs
        .files()
        .filter(|f| f.ends_with(".bsp") && !f.starts_with("maps/mp/"))
        .collect();
    maps.sort_unstable();
    let mut lookers = std::collections::BTreeMap::new();
    for name in &maps {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let spawns = npc::npc_spawns(
            &bsp.entities,
            Rules::for_map(name),
            b"",
            &mut Rand::default(),
        );
        for n in &spawns {
            let Some(nodes) = n
                .spawnscript
                .as_deref()
                .and_then(script::script_path)
                .and_then(|path| vfs.read(&path).ok())
                .map(|bytes| swjkja_icarus::parse(&bytes).unwrap())
            else {
                continue;
            };
            let mut targets = Vec::new();
            sets(&nodes, &mut |set, value| {
                if set == "SET_LOOK_TARGET" {
                    targets.push(value.to_owned());
                }
            });
            for t in targets {
                let named = |v: Option<&str>| v.is_some_and(|v| v.eq_ignore_ascii_case(&t));
                // G_Find looks by targetname first: no map entity has the name, so
                // it finds the NPC, whose targetname is its NPC_targetname.
                assert!(
                    !bsp.entities
                        .iter()
                        .any(|e| named(e.get("targetname")) || named(e.get("script_targetname"))),
                    "{name}: {t}"
                );
                let npcs = spawns.iter().filter(|s| named(s.targetname.as_deref()));
                assert_eq!(npcs.count(), 1, "{name}: {t}");
                *lookers.entry((*name, t)).or_insert(0) += 1;
            }
        }
    }
    // Counted from the scan: the academies' students, each looking at Luke, who is
    // spawned on the same map.
    let luke = |map| (map, "npc_luke1".to_owned());
    assert_eq!(
        lookers,
        [
            (luke("maps/academy1.bsp"), 11),
            (luke("maps/academy2.bsp"), 15),
            (luke("maps/academy4.bsp"), 13),
            (luke("maps/academy6.bsp"), 14),
        ]
        .into()
    );
}

/// Calls `f` with each `set` command's name and text value in `nodes`, depth first.
fn sets(nodes: &[swjkja_icarus::Node], f: &mut dyn FnMut(&str, &str)) {
    for n in nodes {
        match n {
            swjkja_icarus::Node::Command(b) if b.id == swjkja_icarus::id::SET => {
                if let (Some(name), Some(value)) = (
                    b.members.first().and_then(|m| m.text()),
                    b.members.get(1).and_then(|m| m.text()),
                ) {
                    f(name, value);
                }
            }
            swjkja_icarus::Node::Body(_, body) => sets(body, f),
            _ => {}
        }
    }
}

#[test]
fn every_retail_map_places_its_brush_entities() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let (mut sp, mut mp, mut moved, mut turned, mut held) = (0, 0, 0, 0, 0);
    let (mut sp_model2, mut mp_model2) = (0, 0);
    let mut spinning = Vec::new();
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
            if let Some(spin) = b.spin {
                spinning.push((name.to_owned(), spin));
            }
        }
        match rules {
            Rules::SinglePlayer { .. } => sp += brushes.len(),
            Rules::Multiplayer { .. } => mp += brushes.len(),
        }
        let model2 = brushes.iter().filter(|b| b.model2.is_some()).count();
        match rules {
            Rules::SinglePlayer { .. } => sp_model2 += model2,
            Rules::Multiplayer { .. } => mp_model2 += model2,
        }
        // None is sent to every view (the linked box decides).
        assert!(brushes.iter().all(|b| b.model2.is_none() || !b.broadcast));
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
    // The 103 single-player entities with a "model2" (all MD3s), less the 29 usables
    // that start off; siege_desert's three dishes (duel1's func_bobbing is not
    // drawn).
    assert_eq!((sp_model2, mp_model2), (74, 3));
    // The func_rotatings that turn, all started on in single player (no multiplayer
    // map has one): t2_rancor's drum, about its pitch, and t3_hevil's four big fans
    // about their yaw and four small ones about their pitch.
    let count = |map: &str, spin: [f32; 3]| {
        spinning
            .iter()
            .filter(|(m, s)| m == map && *s == spin)
            .count()
    };
    assert_eq!(spinning.len(), 9);
    assert_eq!(count("maps/t2_rancor.bsp", [350.0, 0.0, 0.0]), 1);
    assert_eq!(count("maps/t3_hevil.bsp", [0.0, 300.0, 0.0]), 4);
    assert_eq!(count("maps/t3_hevil.bsp", [850.0, 0.0, 0.0]), 4);
}

/// Reads script `name` from `vfs`, as the game finds it.
fn read_script(vfs: &swjkja_fs::Vfs, name: &str) -> Option<Vec<swjkja_icarus::Node>> {
    let bytes = vfs.read(&script::script_path(name)?).ok()?;
    Some(swjkja_icarus::parse(&bytes).unwrap())
}

/// The maps whose player's spawn point (or a trigger it stands in) uses a script
/// runner as the player begins, and its name.
#[test]
fn intros_start_as_the_player_begins() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let mut maps: Vec<&str> = vfs
        .files()
        .filter(|f| f.ends_with(".bsp") && !f.starts_with("maps/mp/"))
        .collect();
    maps.sort_unstable();
    let mut begun = Vec::new();
    for name in &maps {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let level = script::Level::of(&bsp, &|s| read_script(&vfs, s));
        for b in &level.begin {
            let runs = level.runners.iter().filter(|r| {
                r.targetname
                    .as_deref()
                    .is_some_and(|t| t.eq_ignore_ascii_case(b))
            });
            if runs.count() > 0 {
                begun.push(format!("{} {b}", name.trim_start_matches("maps/")));
            }
        }
    }
    // Counted from the scan: the academies' intros and most single-player maps'
    // opening scripts.
    let want = [
        "academy1.bsp t397",
        "academy2.bsp t397",
        "academy3.bsp t399",
        "academy4.bsp t397",
        "academy5.bsp t397",
        "academy6.bsp t404",
        "hoth2.bsp set",
        "hoth3.bsp t472",
        "kor2.bsp set",
        "t1_danger.bsp set",
        "t1_fatal.bsp level_start",
        "t1_inter.bsp t4",
        "t1_rail.bsp t1051",
        "t1_sour.bsp set",
        "t1_surprise.bsp set",
        "t2_dpred.bsp intro_script1",
        "t2_rogue.bsp t430",
        "t2_wedge.bsp set",
        "t3_bounty.bsp t43131140",
        "t3_hevil.bsp t1111118269",
        "t3_rift.bsp set",
        "t3_stamp.bsp set_cranes",
        "taspir1.bsp t271",
        "taspir2.bsp set",
        "vjun1.bsp introRunner",
        "yavin1.bsp start_level",
        "yavin1b.bsp start_level",
        "yavin2.bsp set",
    ];
    assert_eq!(begun, want);
}

/// Every voice line a retail script plays (`CHAN_VOICE*`) loads as a WAV or an MP3
/// with a length, but four the game lacks too.
#[test]
fn every_scripted_voice_line_has_a_length() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    fn sounds(nodes: &[swjkja_icarus::Node], out: &mut Vec<String>) {
        for n in nodes {
            match n {
                swjkja_icarus::Node::Command(b) if b.id == swjkja_icarus::id::SOUND => {
                    let text = |i: usize| b.members.get(i).and_then(|m| m.text()).unwrap_or("");
                    if text(0).to_ascii_uppercase().starts_with("CHAN_VOICE") {
                        out.push(text(1).to_owned());
                    }
                }
                swjkja_icarus::Node::Body(_, body) => sounds(body, out),
                _ => {}
            }
        }
    }
    let mut lines = Vec::new();
    for f in vfs
        .files()
        .filter(|f| f.to_ascii_lowercase().ends_with(".ibi"))
    {
        sounds(
            &swjkja_icarus::parse(&vfs.read(f).unwrap()).unwrap(),
            &mut lines,
        );
    }
    lines.sort_unstable();
    lines.dedup();
    let (mut missing, mut unread, mut total) = (Vec::new(), Vec::new(), 0.0);
    for l in &lines {
        let files = swjkja_world::sound::files(l);
        match files.iter().find_map(|f| vfs.read(f).ok()) {
            None => missing.push(l.clone()),
            Some(bytes) => match swjkja_world::sound::length_ms(&bytes) {
                Some(ms) => total += ms,
                None => unread.push(l.clone()),
            },
        }
    }
    // Counted from the scan; the missing four are missing from the game too (it
    // plays nothing and goes on).
    assert_eq!(lines.len(), 766);
    assert_eq!(
        missing,
        [
            "sound/chars/jaden_male/28jak006.mp3",
            "sound/chars/jaden_male/28jak007.mp3",
            "sound/chars/sab1/08is3002.mp3",
            "sound/chars/stofficer2/misc/anger1"
        ]
    );
    assert!(unread.is_empty(), "{unread:?}");
    // 45 minutes and 46 seconds of lines.
    assert_eq!((total / 1000.0).round(), 2746.0);
}

#[test]
fn npcs_open_the_doors_they_stand_in() {
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
    let (mut doors, mut touchable, mut named) = (0, 0, 0);
    let mut opened = Vec::new();
    for name in &maps {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let rules = Rules::for_map(name);
        let bodies: Vec<_> = npc::npc_models(
            &bsp.entities,
            rules,
            &parms,
            &sabers,
            &Collision::at_spawn(&bsp, rules),
            &|s: &str| read_script(&vfs, s),
        )
        .iter()
        .filter_map(|m| m.entity_box)
        .map(|[mins, maxs]| [mins.map(|x| x + 1.0), maxs.map(|x| x - 1.0)])
        .collect();
        let mut brushes =
            swjkja_world::brush_entity::brush_entities(&bsp.entities, &bsp.models, rules);
        swjkja_world::brush_entity::touch_doors(&mut brushes, &bodies, script::FIRST_FRAME);
        for b in &brushes {
            let Some(d) = &b.door else { continue };
            doors += 1;
            touchable += usize::from(d.touched);
            named += usize::from(d.targetname.is_some());
            if !b.slides.is_empty() {
                opened.push(format!("{name} *{}", b.model));
            }
        }
    }
    // Counted independently with Python from the entity text: the func_doors with a
    // model that spawn at the easiest skill, and those without a "targetname",
    // "health" or a flag that keeps a touch from opening them; and those with a
    // "targetname" that are not locked, key-opened or inactive (11 of which use
    // targets as they move, which is left out).
    assert_eq!((doors, touchable, named), (458, 280, 101));
    // yavin1's near aisle door, open in the original's yavin1_students view (its far
    // one, *2, has no NPC in its trigger).
    assert_eq!(
        opened,
        [
            "maps/t1_danger.bsp *67",
            "maps/t1_sour.bsp *74",
            "maps/t2_wedge.bsp *156",
            "maps/yavin1.bsp *3"
        ]
    );
}

/// t1_fatal's views of what is used (bench/reference/script-views.txt): its first
/// reactor elevator, whose runner "elev1_low" is used at 3000, and the door
/// "Sateliteroom_Door", used at 4500.
#[test]
fn t1_fatals_used_elevator_rises_and_its_used_door_closes_again() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = Bsp::parse(&vfs.read("maps/t1_fatal.bsp").unwrap()).unwrap();
    let rules = Rules::for_map("maps/t1_fatal.bsp");
    let mut brushes = swjkja_world::brush_entity::brush_entities(&bsp.entities, &bsp.models, rules);
    let mut level = script::Level::of(&bsp, &|s| read_script(&vfs, s));
    let movable: Vec<usize> = (0..brushes.len())
        .filter(|&i| brushes[i].script_name.is_some())
        .collect();
    level.movers = movable
        .iter()
        .map(|&i| script::Mover {
            name: brushes[i].script_name.clone().unwrap(),
            origin: brushes[i].origin,
            angles: brushes[i].angles,
            linear: brushes[i].linear,
        })
        .collect();
    level.uses = [("elev1_low", 3000), ("Sateliteroom_Door", 4500)]
        .map(|(name, at)| script::Use {
            name: name.to_owned(),
            at,
        })
        .to_vec();
    let t = script::timelines(&[], &level, &|_| None);
    // The runner waits 2.2 s, then the elevator rises to tag_up1 over 4 s; on its
    // turn there it waits a second and goes back down.
    let elevator = movable
        .iter()
        .position(|&i| brushes[i].script_name.as_deref() == Some("reactor_elevator1"))
        .unwrap();
    let slides: Vec<_> = t.movers[elevator]
        .slides
        .iter()
        .map(|s| (s.start, s.from[2], s.to[2], s.duration))
        .collect();
    assert_eq!(
        slides,
        [(5250, 4294.0, 4792.0, 4000), (10300, 4792.0, 4296.0, 4000)]
    );
    // The door rises 122 units in 305 ms, stands open on the frame at 4900 and
    // closes two seconds later.
    swjkja_world::brush_entity::use_doors(&mut brushes, &t.used);
    let door = brushes.iter().find(|b| b.model == 149).unwrap();
    let slides: Vec<_> = door
        .slides
        .iter()
        .map(|s| (s.start, s.from[2], s.to[2], s.duration))
        .collect();
    assert_eq!(slides, [(4550, 0.0, 122.0, 305), (6900, 122.0, 0.0, 305)]);
}
