//! Builds the world of every retail map. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use swjkja_bsp::{Bsp, SurfaceKind};
use swjkja_world::collision::Collision;
use swjkja_world::rand::Rand;
use swjkja_world::static_model::Rules;
use swjkja_world::{Geometry, Options, World, nav, npc, script};

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
            b"",
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

#[test]
fn t2_dpreds_rack_goods_spread_as_in_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let files: Vec<Vec<u8>> = vfs
        .list("ext_data/sabers", ".sab")
        .iter()
        .map(|f| vfs.read(&format!("ext_data/sabers/{f}")).unwrap())
        .collect();
    let sabers = npc::join_parms(files.iter().map(Vec::as_slice));
    let bsp = Bsp::parse(&vfs.read("maps/t2_dpred.bsp").unwrap()).unwrap();
    let rules = Rules::for_map("t2_dpred");
    let models = swjkja_world::static_model::entity_models(
        &bsp.entities,
        rules,
        &sabers,
        &mut Collision::at_spawn(&bsp, rules),
    );
    let at = |origin: [f32; 3]| {
        models
            .iter()
            .find(|m| {
                m.origin
                    .iter()
                    .zip(origin)
                    .all(|(a, b)| (a - b).abs() < 0.01)
            })
            .unwrap_or_else(|| panic!("nothing at {origin:?}"))
    };
    // Logged by the reference capture (g_swjkjaRand): dpred_guns' middle rack (at
    // 127 -2106 590, facing +y, its right +x) draws the map's 38th to 52nd numbers,
    // after 31 stormtroopers' coin flips and the first two racks. Each weapon draws
    // its spread aside, then forward (up to 2 units), then its pitch, yaw and roll.
    let logged = [
        (-9.0, -0.129578, -0.597534, 23.0),
        (0.0, -0.749939, -0.19397, 24.5),
        (9.0, 0.542114, 0.356384, 23.0),
    ];
    for (right, aside, fwd, up) in logged {
        at([127.0 + right + aside * 2.0, -2106.0 + fwd * 2.0, 590.0 + up]);
    }
    let blaster = at([127.0 - 9.0 - 0.259156, -2106.0 - 1.195068, 613.0]);
    // Within the log's six digits and a step of the angles' 65536ths of a turn.
    let turned = swjkja_world::static_model::angles_to_axis([
        0.9328 * 4.0 - 75.0,
        270.0 - 0.96582 * 14.0,
        0.937744 * 4.0,
    ]);
    for (a, b) in blaster.axes.iter().flatten().zip(turned.iter().flatten()) {
        assert!(
            (a - b).abs() < 1e-3,
            "{:?} against {turned:?}",
            blaster.axes
        );
    }
    // The first ammo rack (at -2022 -4346 30, facing +y) places its three blaster
    // packs 100 ms in, after every gun rack: the first is the 122nd draw.
    at([-2030.0, -4346.0 - 0.661621 * 0.5, 37.0]);
}

/// Every single-player map's world effects, and its outside scan. Prints them
/// (run with `--nocapture`): which maps rain, and where (t3_hevil's fan pit and
/// vjun1's hall show streaks the original draws).
#[test]
fn every_single_player_maps_weather_reads_and_scans() {
    use swjkja_world::weather::{Command, Outside, PointContents, commands};
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
    // Each weather map's zone count and its other commands, in order.
    let expected: &[(&str, usize, &[&str])] = &[
        (
            "maps/hoth2.bsp",
            10,
            &[
                "constantwind ( -3000.000000 -0.000262 -0.000000 )",
                "gustingwind",
                "snow",
                "fog",
            ],
        ),
        (
            "maps/t1_rail.bsp",
            10,
            &[
                "constantwind ( -5000.000000 -0.000437 -0.000000 )",
                "heavyrain",
                "heavyrainfog",
                "outsideShake",
            ],
        ),
        (
            "maps/t2_wedge.bsp",
            3,
            &[
                "constantwind ( 212.132034 212.132034 -0.000000 )",
                "gustingwind",
                "light_fog",
            ],
        ),
        ("maps/vjun1.bsp", 45, &["acidrain"]),
        ("maps/yavin2.bsp", 5, &["lightrain"]),
    ];
    let mut found = Vec::new();
    for name in maps {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let cmds = commands(&bsp.entities, &bsp.models, 0);
        if cmds.is_empty() {
            continue;
        }
        let parsed: Vec<Command> = cmds.iter().filter_map(|c| Command::parse(c)).collect();
        assert_eq!(parsed.len(), cmds.len(), "{name}: {cmds:?}");
        let zones: Vec<_> = parsed
            .iter()
            .filter_map(|c| match *c {
                Command::Zone { mins, maxs } => Some((mins, maxs)),
                _ => None,
            })
            .collect();
        let others: Vec<String> = cmds
            .iter()
            .filter(|c| !c.starts_with("zone "))
            .cloned()
            .collect();
        found.push((name, zones.len(), others));
        let world = bsp.models[0];
        let contents = PointContents::new(&bsp);
        let outside = Outside::scan(|p| contents.at(p), &zones, (world.mins, world.maxs))
            .unwrap_or_else(|_| panic!("{name}"));
        // The cache the map ships (its header, version 1 and the map's checksum,
        // matches every one of these maps, so the original reads it), byte for byte.
        let file = vfs.read(&name.replace(".bsp", ".weather")).unwrap();
        assert_eq!(file[..4], 1i32.to_le_bytes(), "{name}");
        assert!(
            outside.cache_bytes() == file[8..],
            "{name}: the scan differs from its .weather"
        );
    }
    let expected: Vec<_> = expected
        .iter()
        .map(|&(n, z, o)| (n, z, o.iter().map(|s| s.to_string()).collect::<Vec<_>>()))
        .collect();
    assert_eq!(found, expected);
}

/// t1_sour's nav graph takes its intro walkers to introNav1 by bay_tre_13, as the
/// original's first think steers them (traced: Kyle heads straight for it, at
/// 288.43 degrees from where he stands).
#[test]
fn t1_sours_intro_walkers_path_to_their_navgoal_by_the_nearest_waypoint() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = Bsp::parse(&vfs.read("maps/t1_sour.bsp").unwrap()).unwrap();
    let world = Collision::new(&bsp);
    let g = nav::Graph::build(&bsp, 1, &world);
    let named = |name: &str| {
        g.nodes
            .iter()
            .position(|n| n.name.as_deref() == Some(name))
            .unwrap()
    };
    let (goal, by) = (named("introNav1"), named("bay_tre_13"));
    let (mins, maxs) = ([-16.0, -16.0, -24.0], [16.0, 16.0, 40.0]);
    for at in [[6248.0, -2552.0, 24.125], [6200.0, -2488.0, 24.125]] {
        let Some(nav::Nearest::Node(start)) = g.nearest(&world, at, None, Some(goal)) else {
            panic!("no nearest point to {at:?}");
        };
        assert_eq!(start, by, "{at:?}");
        // STEER::SafeToGoTo's graph test does not let them go straight.
        let goal_at = g.nodes[goal].point;
        assert!(
            !(g.neighboring(start, goal)
                && g.in_safe_radius(at, start, goal)
                && g.in_safe_radius(goal_at, goal, start)),
            "{at:?}"
        );
        let path = g.find_path(&world, at, mins, maxs, start, goal).unwrap();
        let points: Vec<usize> = path.iter().map(|p| p.node).collect();
        assert_eq!(points, [goal, by], "{at:?}");
        assert_eq!(path[1].point, [6272.0, -2624.0, 24.0]);
    }
}

/// Where the original walks `names` on `map` (traced with `d_npcfreeze 3`, each
/// frame as (who, level time, origin) to the trace's four decimals), here a frame
/// earlier: the frames they are not there.
fn walks_off_the_trace(
    vfs: &swjkja_fs::Vfs,
    map: &str,
    names: &[&str],
    traced: &[(usize, i32, [f32; 3])],
) -> Vec<(usize, i32, Option<[f32; 3]>)> {
    let bsp = Bsp::parse(&vfs.read(map).unwrap()).unwrap();
    let rules = Rules::for_map(map);
    let Rules::SinglePlayer { skill } = rules else {
        panic!("{map} is not single player");
    };
    let text = |folder: &str, ext: &str| {
        let files: Vec<Vec<u8>> = vfs
            .list(folder, ext)
            .iter()
            .filter_map(|f| vfs.read(&format!("{folder}/{f}")).ok())
            .collect();
        npc::join_parms(files.iter().map(Vec::as_slice))
    };
    let sabers = text("ext_data/sabers", ".sab");
    let mut world = Collision::at_spawn(&bsp, rules);
    swjkja_world::static_model::entity_models(&bsp.entities, rules, &sabers, &mut world);
    let scripts = |s: &str| read_script(vfs, s);
    let placements = npc::npc_models(
        &bsp.entities,
        rules,
        &text("ext_data/npcs", ".npc"),
        &sabers,
        &world,
        &scripts,
    );
    let mut level = script::Level::of(&bsp, &scripts);
    level.collision = world;
    level.nav = Some(nav::Graph::build(&bsp, skill, &Collision::new(&bsp)));
    let holds = |_: &str| None;
    let poses: Vec<_> = placements
        .iter()
        .filter_map(|p| p.ghoul2.as_ref()?.pose.as_ref())
        .collect();
    let npcs: Vec<script::Npc> = poses
        .iter()
        .map(|pose| script::Npc {
            stance: pose,
            script: pose.script.as_deref().map(Vec::as_slice),
            holds: &holds,
        })
        .collect();
    let voice = |name: &str| {
        let bytes = swjkja_world::sound::files(name)
            .iter()
            .find_map(|f| vfs.read(f).ok())?;
        swjkja_world::sound::length_ms(&bytes).map(|ms| ms.ceil() as i32)
    };
    let t = script::timelines(&npcs, &level, &voice);
    let walkers: Vec<_> = names
        .iter()
        .map(|&name| {
            let i = poses
                .iter()
                .position(|p| p.name.as_deref() == Some(name))
                .unwrap();
            (poses[i].origin, &t.npcs[i])
        })
        .collect();
    let mut off = Vec::new();
    for &(who, level, traced) in traced {
        let (start, timeline) = walkers[who];
        let at = timeline
            .moves
            .iter()
            .find(|(l, _)| *l == level - FRAME_MS)
            .map(|(_, o)| [0, 1, 2].map(|k| start[k] + o[k]));
        if !at.is_some_and(|at| (0..3).all(|k| (at[k] - traced[k]).abs() < 0.001)) {
            off.push((who, level, at));
        }
    }
    off
}

/// A level frame (ms).
const FRAME_MS: i32 = 50;

/// t1_sour's intro walkers as the original walks them, every traced frame (both
/// from rest, their first think at 2100): the player's stand-in, thinking first,
/// takes the nav graph toward bay_tre_13 and steers left of Kyle standing in its
/// way; Kyle heads straight for bay_tre_13. From the next think on, a clear trace
/// sends both straight to introNav1, until at 3400 the stand-in, closing on Kyle,
/// steers left of him again. Here they start a frame earlier: the original updates
/// the intro's runner after them (by entity number), these runners first.
#[test]
// The traced positions as the trace logs them.
#[allow(clippy::excessive_precision)]
fn t1_sours_intro_walkers_walk_as_in_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let names = ["npc_fakePlayerIntro", "npc_fakeKyleIntro"];
    let traced = [
        (0, 2100, [6201.9834, -2489.8318, 24.1250]),
        (1, 2100, [6248.8540, -2554.5615, 24.1250]),
        (0, 2150, [6204.8486, -2492.4778, 24.1250]),
        (1, 2150, [6250.0874, -2558.2615, 24.1250]),
        (0, 2200, [6205.7314, -2496.6584, 24.1250]),
        (1, 2200, [6249.4443, -2562.8105, 24.1250]),
        (0, 2250, [6205.6123, -2501.3774, 24.1250]),
        (1, 2250, [6248.1631, -2567.2490, 24.1250]),
        (0, 2300, [6205.0459, -2505.9539, 24.1250]),
        (1, 2300, [6246.5210, -2571.4756, 24.1250]),
        (0, 2350, [6204.1709, -2510.4199, 24.1250]),
        (1, 2350, [6244.6357, -2575.5483, 24.1250]),
        (0, 2400, [6203.0767, -2514.8103, 24.1250]),
        (1, 2400, [6242.5781, -2579.5146, 24.1250]),
        (0, 2450, [6201.8354, -2519.1472, 24.1250]),
        (1, 2450, [6240.4067, -2583.4080, 24.1250]),
        (0, 2500, [6200.4922, -2523.4475, 24.1250]),
        (1, 2500, [6238.1289, -2587.2932, 24.1250]),
        (0, 2550, [6199.0806, -2527.7229, 24.1250]),
        (1, 2550, [6235.7988, -2591.1450, 24.1250]),
        (0, 2600, [6197.6216, -2531.9810, 24.1250]),
        (1, 2600, [6233.4600, -2594.9316, 24.1250]),
        (0, 2650, [6196.1313, -2536.2275, 24.1250]),
        (1, 2650, [6231.0967, -2598.7026, 24.1250]),
        (0, 2700, [6194.6191, -2540.4661, 24.1250]),
        (1, 2700, [6228.6895, -2602.5049, 24.1250]),
        (0, 2750, [6193.0923, -2544.6992, 24.1250]),
        (1, 2750, [6226.2710, -2606.3000, 24.1250]),
        (0, 2800, [6191.5557, -2548.9287, 24.1250]),
        (1, 2800, [6223.8447, -2610.0901, 24.1250]),
        (0, 2850, [6190.0122, -2553.1558, 24.1250]),
        (1, 2850, [6221.4136, -2613.8770, 24.1250]),
        (0, 2900, [6188.4814, -2557.3342, 24.1250]),
        (1, 2900, [6218.9790, -2617.6614, 24.1250]),
        (0, 2950, [6186.9473, -2561.5115, 24.1250]),
        (1, 2950, [6216.5420, -2621.4443, 24.1250]),
        (0, 3000, [6185.3940, -2565.7349, 24.1250]),
        (1, 3000, [6214.1030, -2625.2261, 24.1250]),
        (0, 3050, [6183.8394, -2569.9578, 24.1250]),
        (1, 3050, [6211.6631, -2629.0071, 24.1250]),
        (0, 3100, [6182.2837, -2574.1802, 24.1250]),
        (1, 3100, [6209.2222, -2632.7876, 24.1250]),
        (0, 3150, [6180.7271, -2578.4023, 24.1250]),
        (1, 3150, [6206.7808, -2636.5679, 24.1250]),
        (0, 3200, [6179.1875, -2582.5774, 24.1250]),
        (1, 3200, [6204.3394, -2640.3479, 24.1250]),
        (0, 3250, [6177.6475, -2586.7524, 24.1250]),
        (1, 3250, [6201.8975, -2644.1277, 24.1250]),
        (0, 3300, [6176.0898, -2590.9744, 24.1250]),
        (1, 3300, [6199.4556, -2647.9075, 24.1250]),
        (0, 3350, [6174.5322, -2595.1963, 24.1250]),
        (1, 3350, [6197.0132, -2651.6870, 24.1250]),
        (0, 3400, [6175.7051, -2599.5071, 24.1250]),
        (1, 3400, [6194.5708, -2655.4666, 24.1250]),
        (0, 3450, [6178.3062, -2603.6035, 24.1250]),
        (1, 3450, [6192.1284, -2659.2461, 24.1250]),
        (0, 3500, [6180.0645, -2606.6218, 24.1250]),
        (1, 3500, [6189.6860, -2663.0256, 24.1250]),
        (0, 3550, [6180.8418, -2609.4375, 24.1250]),
        (1, 3550, [6187.2437, -2666.8052, 24.1250]),
    ];
    let off = walks_off_the_trace(&vfs, "maps/t1_sour.bsp", &names, &traced);
    assert!(
        off.is_empty(),
        "{} frames off, the first {:?}",
        off.len(),
        off.first()
    );
}

/// academy3's Luke walks to lukeNav1 by the nav graph's first think as the
/// original does (traced from 14,850), here a frame earlier (the voice line before
/// ends a frame apart: DECISIONS.md, 2026-10-26).
#[test]
#[allow(clippy::excessive_precision)]
fn academy3s_luke_walks_as_in_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let traced = [
        (0, 14850, [-1343.6440, -1339.1774, 744.1250]),
        (0, 14900, [-1343.2881, -1340.3547, 744.1250]),
        (0, 14950, [-1342.9408, -1341.5034, 744.1250]),
        (0, 15000, [-1342.5935, -1342.6521, 744.1250]),
        (0, 15050, [-1342.1160, -1344.2314, 744.1250]),
        (0, 15100, [-1341.5951, -1345.9545, 744.1250]),
        (0, 15150, [-1341.0308, -1347.8210, 744.1250]),
        (0, 15200, [-1340.4231, -1349.8312, 744.1250]),
        (0, 15250, [-1339.7720, -1351.9849, 744.1250]),
        (0, 15300, [-1339.0774, -1354.2822, 744.1250]),
        (0, 15350, [-1338.3481, -1356.6945, 744.1250]),
        (0, 15400, [-1337.5841, -1359.2216, 744.1250]),
        (0, 15450, [-1336.7883, -1361.8539, 744.1250]),
        (0, 15500, [-1335.9926, -1364.4862, 744.1250]),
        (0, 15550, [-1335.1968, -1367.1185, 744.1250]),
        (0, 15600, [-1334.4010, -1369.7509, 744.1250]),
        (0, 15650, [-1333.6196, -1372.3353, 744.1250]),
        (0, 15700, [-1332.8383, -1374.9198, 744.1250]),
        (0, 15750, [-1332.0425, -1377.5521, 744.1250]),
        (0, 15800, [-1331.2467, -1380.1844, 744.1250]),
        (0, 15850, [-1330.4509, -1382.8168, 744.1250]),
        (0, 15900, [-1329.6552, -1385.4491, 744.1250]),
        (0, 15950, [-1328.8738, -1388.0336, 744.1250]),
        (0, 16000, [-1328.0924, -1390.6180, 744.1250]),
        (0, 16050, [-1327.2966, -1393.2504, 744.1250]),
        (0, 16100, [-1326.5009, -1395.8827, 744.1250]),
        (0, 16150, [-1325.7051, -1398.5150, 744.1250]),
        (0, 16200, [-1324.9093, -1401.1473, 744.1250]),
        (0, 16250, [-1324.1135, -1403.7797, 744.1250]),
        (0, 16300, [-1323.3177, -1406.4120, 744.1250]),
        (0, 16350, [-1322.5220, -1409.0443, 744.1250]),
        (0, 16400, [-1321.7262, -1411.6766, 744.1250]),
        (0, 16450, [-1320.9304, -1414.3090, 744.1250]),
        (0, 16500, [-1320.1346, -1416.9413, 744.1250]),
        (0, 16550, [-1319.3533, -1419.5258, 744.1250]),
        (0, 16600, [-1318.5719, -1422.1102, 744.1250]),
        (0, 16650, [-1317.7905, -1424.6947, 744.1250]),
        (0, 16700, [-1317.0092, -1427.2792, 744.1250]),
        (0, 16750, [-1316.2134, -1429.9115, 744.1250]),
        (0, 16800, [-1315.4176, -1432.5438, 744.1250]),
        (0, 16850, [-1314.6362, -1435.1283, 744.1250]),
        (0, 16900, [-1313.8549, -1437.7128, 744.1250]),
        (0, 16950, [-1313.0735, -1440.2972, 744.1250]),
        (0, 17000, [-1312.2921, -1442.8817, 744.1250]),
        (0, 17050, [-1311.4962, -1445.5140, 744.1250]),
        (0, 17100, [-1310.7003, -1448.1464, 744.1250]),
        (0, 17150, [-1309.9044, -1450.7787, 744.1250]),
        (0, 17200, [-1309.1086, -1453.4110, 744.1250]),
        (0, 17250, [-1308.3129, -1456.0433, 744.1250]),
        (0, 17300, [-1307.5171, -1458.6757, 744.1250]),
        (0, 17350, [-1306.7212, -1461.3080, 744.1250]),
        (0, 17400, [-1305.9253, -1463.9403, 744.1250]),
        (0, 17450, [-1305.1294, -1466.5726, 744.1250]),
        (0, 17500, [-1304.3336, -1469.2050, 744.1250]),
        (0, 17550, [-1303.6390, -1471.5023, 744.1250]),
        (0, 17600, [-1302.9619, -1473.7422, 744.1250]),
        (0, 17650, [-1302.3455, -1475.7810, 744.1250]),
        (0, 17700, [-1301.7898, -1477.6189, 744.1250]),
        (0, 17750, [-1301.3384, -1479.1122, 744.1250]),
        (0, 17800, [-1300.9911, -1480.2609, 744.1250]),
        (0, 17850, [-1300.6873, -1481.2660, 744.1250]),
        (0, 17900, [-1300.3834, -1482.2711, 744.1250]),
        (0, 17950, [-1300.0969, -1483.2188, 744.1250]),
        (0, 18000, [-1299.8104, -1484.1664, 744.1250]),
        (0, 18050, [-1299.5499, -1485.0278, 744.1250]),
        (0, 18100, [-1299.2894, -1485.8893, 744.1250]),
        (0, 18150, [-1299.0464, -1486.6934, 744.1250]),
        (0, 18200, [-1298.8033, -1487.4974, 744.1250]),
        (0, 18250, [-1298.5776, -1488.2440, 744.1250]),
        (0, 18300, [-1298.3519, -1488.9906, 744.1250]),
        (0, 18350, [-1298.1349, -1489.7085, 744.1250]),
        (0, 18400, [-1297.9178, -1490.4264, 744.1250]),
        (0, 18450, [-1297.7181, -1491.0869, 744.1250]),
        (0, 18500, [-1297.5184, -1491.7474, 744.1250]),
        (0, 18550, [-1297.3361, -1492.3505, 744.1250]),
        (0, 18600, [-1297.1537, -1492.9535, 744.1250]),
        (0, 18650, [-1296.9801, -1493.5278, 744.1250]),
        (0, 18700, [-1296.8065, -1494.1022, 744.1250]),
    ];
    let off = walks_off_the_trace(&vfs, "maps/academy3.bsp", &["npc_luke1"], &traced);
    assert!(
        off.is_empty(),
        "{} frames off: {:?}",
        off.len(),
        &off[..off.len().min(4)]
    );
}
