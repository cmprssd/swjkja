//! Builds the world of every retail map. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use swjkja_bsp::{Bsp, SurfaceKind};
use swjkja_model::animation;
use swjkja_world::brush_entity::{Slide, slid};
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
        let level = script::Level::of(&bsp, 0, &|s| read_script(&vfs, s));
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
    // 46 minutes and 1 second of lines.
    assert_eq!((total / 1000.0).round(), 2761.0);
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
    let mut level = script::Level::of(&bsp, 0, &|s| read_script(&vfs, s));
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
        let path = g
            .find_path(&world, at, mins, maxs, start, goal, 0, 90.0)
            .unwrap();
        let points: Vec<usize> = path.iter().map(|p| p.node).collect();
        assert_eq!(points, [goal, by], "{at:?}");
        // bay_tre_13 stands on the floor at 24: SP_waypoint lifts it clear
        // (`G_CheckInSolid`'s fix).
        assert_eq!(path[1].point, [6272.0, -2624.0, 24.125]);
    }
}

/// Where the original walks `names` on `map` (traced with `d_npcfreeze 3`, each
/// frame as (who, level time, origin) to the trace's four decimals), here `early`
/// ms earlier: the frames they are not there.
fn walks_off_the_trace(
    vfs: &swjkja_fs::Vfs,
    map: &str,
    names: &[&str],
    traced: &[(usize, i32, [f32; 3])],
    early: i32,
) -> Vec<(usize, i32, Option<[f32; 3]>)> {
    let walkers = walkers(vfs, map, names, &|_| None);
    let mut off = Vec::new();
    for &(who, level, traced) in traced {
        let (start, timeline) = &walkers[who];
        let at = timeline
            .moves
            .iter()
            .find(|(l, _)| *l == level - early)
            .map(|(_, o)| [0, 1, 2].map(|k| start[k] + o[k]));
        if !at.is_some_and(|at| (0..3).all(|k| (at[k] - traced[k]).abs() < 0.001)) {
            off.push((who, level, at));
        }
    }
    off
}

/// Where `names` on `map` begin and what they do, their skeletons having the
/// animations `holds` gives.
fn walkers(
    vfs: &swjkja_fs::Vfs,
    map: &str,
    names: &[&str],
    holds: script::Holds,
) -> Vec<([f32; 3], script::Timeline)> {
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
    let mut level = script::Level::of(&bsp, 0, &scripts);
    level.collision = world;
    level.nav = Some(nav::Graph::build(&bsp, skill, &Collision::new(&bsp)));
    let poses: Vec<_> = placements
        .iter()
        .filter_map(|p| p.ghoul2.as_ref()?.pose.as_ref())
        .collect();
    let npcs: Vec<script::Npc> = poses
        .iter()
        .map(|pose| script::Npc {
            stance: pose,
            script: pose.script.as_deref().map(Vec::as_slice),
            holds,
        })
        .collect();
    let voice = |name: &str| {
        let bytes = swjkja_world::sound::files(name)
            .iter()
            .find_map(|f| vfs.read(f).ok())?;
        swjkja_world::sound::length_ms(&bytes).map(|ms| ms.ceil() as i32)
    };
    let t = script::timelines(&npcs, &level, &voice);
    names
        .iter()
        .map(|&name| {
            let i = poses
                .iter()
                .position(|p| p.name.as_deref() == Some(name))
                .unwrap();
            (poses[i].origin, t.npcs[i].clone())
        })
        .collect()
}

/// t1_sour's intro walkers as the original walks them, every traced frame (both
/// from rest, their first think at 2100): the player's stand-in, thinking first,
/// takes the nav graph toward bay_tre_13 and steers left of Kyle standing in its
/// way; Kyle heads straight for bay_tre_13. From the next think on, a clear trace
/// sends both straight to introNav1, until at 3400 the stand-in, closing on Kyle,
/// steers left of him again. The intro's runner (entity 336) updates after them
/// (144 and 146), so they think a frame after it sets their navgoals.
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
    let off = walks_off_the_trace(&vfs, "maps/t1_sour.bsp", &names, &traced, 0);
    assert!(
        off.is_empty(),
        "{} frames off, the first {:?}",
        off.len(),
        off.first()
    );
}

/// t1_sour's intro walkers' legs and torsos at the last traced frame (3550, on the
/// client's clock 3500), as the original's trace logs their bones: each walk plays
/// as fast as its last frame moved it over the walk's stride (50 a second,
/// `g_noFootSlide`). The player's stand-in, its steps slowing and quickening,
/// carries on from where it was each time, back to 2754, at 1.1684 (58.42 units
/// a second); Kyle has gone faster than 75 a second from his first step, at the
/// most, 1.5, since he began at 2050.
#[test]
fn t1_sours_intro_walkers_stride_as_in_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let map = "maps/t1_sour.bsp";
    let text = |skeleton: &str| {
        let file = animation::config_file(&vfs, skeleton)?;
        Some(String::from_utf8_lossy(&vfs.read(&file).ok()?).into_owned())
    };
    let humanoid = text(animation::HUMANOID).unwrap();
    let cinematic = text(&animation::cinematic_skeleton(map));
    let holds = |name: &str| {
        let cinematic = cinematic.as_deref().and_then(|c| animation::find(c, name));
        cinematic.or_else(|| animation::find(&humanoid, name))
    };
    let names = ["npc_fakePlayerIntro", "npc_fakeKyleIntro"];
    let walkers = walkers(&vfs, map, &names, &holds);
    for ((_, t), (begun, speed)) in walkers.iter().zip([(2754, 1.1684), (2050, 1.5)]) {
        for part in [0, 1] {
            let c = t.at(part, 3500);
            assert_eq!(c.anim, "BOTH_WALK1");
            assert_eq!(c.clock.begun, begun, "{part}");
            assert!(
                (c.clock.speed.unwrap() - speed).abs() < 0.0001,
                "{:?}",
                c.clock
            );
        }
    }
    // The stand-in's legs at 3500 face the velocity the trace gives her frame at
    // 3550 (15.5426, -56.3122: yaw -74.570), 27.114 left of her view's -101.684.
    let t = &walkers[0].1;
    let yaw = |turns: &[Slide]| slid(turns, 3500).unwrap()[1];
    let turned = yaw(&t.legs_turns(0.0)) - yaw(&t.turns(0.0));
    assert!((turned - 27.114).abs() < 0.01, "{turned}");
}

/// academy3's Luke walks to lukeNav1 by the nav graph's first think as the
/// original does, at the trace's own frames (from 14,850): his voice line before
/// plays out as the original's does (its Xing frame counted; DECISIONS.md,
/// 2026-10-30).
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
    let off = walks_off_the_trace(&vfs, "maps/academy3.bsp", &["npc_luke1"], &traced, 0);
    assert!(
        off.is_empty(),
        "{} frames off: {:?}",
        off.len(),
        &off[..off.len().min(4)]
    );
}

/// yavin2's intro walks Kyle and Rosh to kyleNav1 and roshNav1, 415 units off and
/// down a flight of stairs, as the original does, every traced frame: by the nav
/// graph while the goal is out of reach, each think keeping the path the last one
/// found (`NAV::UpdatePath`: Kyle passes the point beside him, then heads for the
/// stairs), then down them as `Pmove` takes them (walking off each step and
/// falling, landing on the ramp's clip and walking down it, then up the garden's
/// slope). Here a frame later than in the original: Kyle's navgoal is set a frame
/// later (the voice lines before it end then), and Rosh follows his signal.
#[test]
#[allow(clippy::excessive_precision)]
fn yavin2s_intro_walkers_walk_down_the_stairs_as_in_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let names = ["npc_kyleintro1", "npc_roshintro1"];
    // The traced positions as the trace logs them, from each one's first move.
    let traced = [
        (0, 15900, [2358.0579, -5567.4819, 88.1250]),
        (0, 15950, [2355.6230, -5566.8325, 88.1250]),
        (0, 16000, [2352.6953, -5566.0518, 88.1250]),
        (0, 16050, [2349.4585, -5565.1885, 88.1250]),
        (0, 16100, [2346.2217, -5564.3252, 88.1250]),
        (0, 16150, [2342.9849, -5563.4619, 88.1250]),
        (0, 16200, [2339.7480, -5562.5986, 88.1250]),
        (0, 16250, [2336.5112, -5561.7354, 88.1250]),
        (0, 16300, [2333.3228, -5560.8853, 88.1250]),
        (0, 16350, [2330.1343, -5560.0352, 88.1250]),
        (0, 16400, [2325.8345, -5558.8887, 88.1250]),
        (0, 16450, [2321.5347, -5557.7422, 88.1250]),
        (0, 16500, [2317.2349, -5556.5957, 88.1250]),
        (0, 16550, [2312.9351, -5555.4492, 88.1250]),
        (0, 16600, [2309.7410, -5552.0220, 88.1253]),
        (0, 16650, [2307.3259, -5547.7812, 88.1256]),
        (0, 16700, [2305.4927, -5543.4448, 88.1257]),
        (0, 16750, [2304.0798, -5539.0605, 88.1259]),
        (0, 16800, [2302.9680, -5534.6377, 88.1261]),
        (0, 16850, [2302.0620, -5530.1929, 88.1263]),
        (1, 16900, [2408.3215, -5598.6890, 88.1250]),
        (0, 16900, [2301.2991, -5525.7310, 88.1265]),
        (1, 16950, [2408.6431, -5597.3779, 88.1250]),
        (0, 16950, [2300.6323, -5521.2588, 88.1267]),
        (1, 17000, [2406.1060, -5596.4546, 88.1250]),
        (0, 17000, [2300.0371, -5516.8281, 88.1269]),
        (1, 17050, [2402.4412, -5595.1206, 88.1250]),
        (0, 17050, [2299.4868, -5512.3936, 88.1271]),
        (1, 17100, [2398.2124, -5593.5815, 88.1250]),
        (0, 17100, [2298.9624, -5507.9053, 88.1273]),
        (1, 17150, [2393.9836, -5592.0425, 88.1250]),
        (0, 17150, [2298.4583, -5503.4141, 88.1275]),
        (1, 17200, [2389.8020, -5590.5205, 88.1250]),
        (0, 17200, [2297.9680, -5498.9204, 88.1277]),
        (1, 17250, [2385.6204, -5588.9985, 88.1250]),
        (0, 17250, [2297.4871, -5494.4258, 88.1279]),
        (1, 17300, [2381.3916, -5587.4595, 88.1250]),
        (0, 17300, [2297.0125, -5489.9297, 88.1282]),
        (1, 17350, [2377.1628, -5585.9204, 88.1250]),
        (0, 17350, [2296.5422, -5485.4331, 88.1284]),
        (1, 17400, [2373.4062, -5582.5723, 88.1254]),
        (0, 17400, [2296.0750, -5480.9351, 88.1286]),
        (1, 17450, [2370.2664, -5578.9595, 88.1256]),
        (0, 17450, [2295.6096, -5476.4365, 88.1288]),
        (1, 17500, [2367.5908, -5575.1484, 88.1258]),
        (0, 17500, [2295.1506, -5471.9863, 88.1291]),
        (1, 17550, [2365.2478, -5571.2080, 88.1260]),
        (0, 17550, [2294.5544, -5466.2935, 87.1293]),
        (1, 17600, [2363.1396, -5567.1729, 88.1262]),
        (0, 17600, [2293.9045, -5460.0771, 84.1295]),
        (1, 17650, [2361.1917, -5563.0762, 88.1264]),
        (0, 17650, [2293.2546, -5453.8608, 79.1297]),
        (1, 17700, [2359.3545, -5558.9351, 88.1266]),
        (0, 17700, [2292.6047, -5447.6445, 72.1299]),
        (1, 17750, [2357.5923, -5554.7656, 88.1269]),
        (0, 17750, [2291.9548, -5441.4282, 65.1022]),
        (1, 17800, [2355.8813, -5550.5752, 88.1271]),
        (0, 17800, [2291.5000, -5435.1851, 60.9401]),
        (1, 17850, [2354.2051, -5546.3716, 88.1273]),
        (0, 17850, [2291.1816, -5430.8149, 58.0267]),
        (1, 17900, [2352.5522, -5542.1577, 88.1276]),
        (0, 17900, [2290.8142, -5426.8179, 55.3630]),
        (1, 17950, [2350.9153, -5537.9380, 88.1278]),
        (0, 17950, [2290.3828, -5422.8882, 52.7444]),
        (1, 18000, [2349.2891, -5533.7124, 88.1281]),
        (0, 18000, [2289.9016, -5418.9604, 50.1272]),
        (1, 18050, [2347.6702, -5529.4844, 88.1283]),
        (0, 18050, [2289.3889, -5415.0356, 47.5118]),
        (1, 18100, [2345.3860, -5525.3584, 88.1289]),
        (0, 18100, [2288.8464, -5411.0796, 44.8758]),
        (1, 18150, [2342.8250, -5521.4644, 88.1293]),
        (0, 18150, [2288.2874, -5407.1187, 42.2367]),
        (1, 18200, [2340.4629, -5517.7212, 88.1298]),
        (0, 18200, [2287.7202, -5403.1572, 39.5972]),
        (1, 18250, [2338.1709, -5513.9580, 88.1303]),
        (0, 18250, [2287.0259, -5397.9424, 35.9577]),
        (1, 18300, [2336.0811, -5509.9009, 88.1306]),
        (0, 18300, [2286.2278, -5391.6909, 30.3182]),
        (1, 18350, [2334.1643, -5505.7759, 88.1308]),
        (0, 18350, [2285.4297, -5385.4395, 30.1250]),
        (1, 18400, [2332.3857, -5501.6504, 88.1311]),
        (0, 18400, [2284.8618, -5380.9692, 30.1250]),
        (1, 18450, [2330.6907, -5497.4956, 88.1314]),
        (0, 18450, [2284.3362, -5376.4971, 30.1251]),
        (1, 18500, [2329.0356, -5493.2715, 88.1317]),
        (0, 18500, [2283.8401, -5372.0239, 30.1251]),
        (1, 18550, [2327.4182, -5489.0322, 88.1320]),
        (0, 18550, [2283.3638, -5367.5488, 30.1251]),
        (1, 18600, [2325.8267, -5484.7812, 88.1323]),
        (0, 18600, [2282.9062, -5363.1226, 30.1251]),
        (1, 18650, [2324.2529, -5480.5239, 88.1326]),
        (0, 18650, [2282.4580, -5358.6953, 30.1251]),
        (1, 18700, [2322.6907, -5476.2598, 88.1329]),
        (0, 18700, [2282.0115, -5354.2173, 30.1251]),
        (1, 18750, [2321.1367, -5471.9922, 88.1332]),
        (0, 18750, [2281.5693, -5349.7393, 30.1251]),
        (1, 18800, [2319.1633, -5466.5581, 87.1335]),
        (0, 18800, [2281.1301, -5345.2607, 30.1251]),
        (1, 18850, [2317.0483, -5460.7300, 84.1338]),
        (0, 18850, [2280.6931, -5340.7822, 30.1251]),
        (1, 18900, [2314.9333, -5454.9019, 79.1341]),
        (0, 18900, [2278.5830, -5337.5933, 30.1251]),
        (1, 18950, [2312.8184, -5449.0737, 72.1344]),
        (0, 18950, [2275.7500, -5335.4819, 30.1251]),
        (1, 19000, [2310.7034, -5443.2456, 66.3136]),
        (0, 19000, [2273.9307, -5331.5732, 30.1251]),
        (1, 19050, [2309.2229, -5437.1904, 62.2769]),
        (0, 19050, [2272.6133, -5327.1548, 30.1251]),
        (1, 19100, [2306.3596, -5432.5391, 59.1781]),
        (0, 19100, [2271.6475, -5322.7505, 30.1251]),
        (1, 19150, [2302.6899, -5428.9395, 56.7802]),
        (0, 19150, [2270.9229, -5318.3330, 30.1251]),
        (1, 19200, [2299.8132, -5426.1143, 54.8968]),
        (0, 19200, [2270.3701, -5313.8535, 30.1251]),
        (1, 19250, [2296.9436, -5423.2842, 53.0102]),
        (0, 19250, [2269.9324, -5309.3691, 30.1251]),
        (1, 19300, [2294.1511, -5419.8564, 50.7278]),
        (0, 19300, [2269.5774, -5304.8799, 30.1251]),
        (1, 19350, [2291.5000, -5416.2061, 48.2963]),
        (0, 19350, [2269.2778, -5300.3887, 30.1251]),
        (1, 19400, [2288.7351, -5413.0859, 46.2163]),
        (0, 19400, [2269.0183, -5295.8955, 30.1251]),
        (1, 19450, [2285.7141, -5410.2285, 44.3118]),
        (0, 19450, [2268.7856, -5291.4014, 30.1251]),
        (1, 19500, [2282.7053, -5406.1138, 41.4072]),
        (0, 19500, [2268.5723, -5286.9062, 30.1251]),
        (1, 19550, [2279.7085, -5400.7412, 36.5026]),
        (0, 19550, [2268.3718, -5282.4106, 30.1251]),
        (1, 19600, [2276.7117, -5395.3687, 30.1250]),
        (0, 19600, [2268.1809, -5277.9146, 30.2993]),
        (1, 19650, [2274.5715, -5391.5205, 30.1250]),
        (0, 19650, [2267.9961, -5273.4204, 30.4906]),
        (1, 19700, [2272.5681, -5387.5913, 30.1250]),
        (0, 19700, [2267.8162, -5268.9263, 30.6819]),
        (1, 19750, [2270.6599, -5383.6216, 30.1250]),
        (0, 19750, [2267.6392, -5264.4321, 30.8732]),
        (1, 19800, [2268.7661, -5379.6499, 30.1250]),
        (0, 19800, [2267.4646, -5259.9380, 31.0645]),
        (1, 19850, [2266.8806, -5375.6743, 30.1250]),
        (0, 19850, [2267.2915, -5255.4434, 31.2558]),
        (1, 19900, [2265.2642, -5371.4287, 30.1250]),
        (0, 19900, [2267.1196, -5250.9487, 31.4471]),
        (1, 19950, [2263.8484, -5367.1367, 30.1250]),
        (0, 19950, [2266.9485, -5246.4541, 31.6384]),
        (1, 20000, [2262.5776, -5362.8096, 30.1250]),
        (0, 20000, [2266.7781, -5241.9595, 31.8298]),
        (1, 20050, [2261.4045, -5358.4604, 30.1250]),
        (0, 20050, [2266.6082, -5237.4648, 32.0211]),
        (1, 20100, [2260.3125, -5354.1440, 30.1250]),
        (0, 20100, [2266.4385, -5232.9702, 32.2124]),
        (1, 20150, [2259.2681, -5349.8174, 30.1250]),
        (0, 20150, [2266.2690, -5228.4756, 32.4037]),
        (1, 20200, [2258.2473, -5345.4341, 30.1250]),
        (0, 20200, [2266.0999, -5223.9810, 32.5951]),
        (1, 20250, [2257.2490, -5341.0459, 30.1250]),
        (0, 20250, [2265.9307, -5219.4863, 32.7864]),
        (1, 20300, [2256.2673, -5336.6543, 30.1250]),
        (0, 20300, [2264.8906, -5214.8882, 32.9822]),
        (1, 20350, [2255.2966, -5332.2603, 30.1250]),
        (0, 20350, [2263.4631, -5210.5117, 33.1685]),
        (1, 20400, [2254.3337, -5327.8643, 30.1250]),
        (0, 20400, [2264.4324, -5205.6812, 33.3743]),
        (1, 20450, [2253.3762, -5323.4673, 30.1250]),
        (0, 20450, [2266.3069, -5201.1538, 33.5671]),
        (1, 20500, [2252.4229, -5319.0693, 30.1250]),
        (0, 20500, [2267.6038, -5197.9561, 33.7031]),
        (1, 20550, [2251.4722, -5314.6709, 30.1250]),
        (0, 20550, [2268.2434, -5194.8062, 33.8372]),
        (1, 20600, [2250.5234, -5310.2720, 30.1250]),
        (0, 20600, [2268.4448, -5192.2446, 33.9463]),
        (1, 20650, [2249.5759, -5305.8730, 30.1250]),
        (0, 20650, [2268.3003, -5189.7383, 34.0530]),
        (1, 20700, [2248.6294, -5301.4736, 30.1250]),
        (0, 20700, [2268.0386, -5187.7017, 34.1397]),
        (1, 20750, [2247.6836, -5297.0742, 30.1250]),
        (0, 20750, [2267.7292, -5185.9478, 34.2144]),
        (1, 20800, [2246.7383, -5292.6748, 30.1250]),
        (0, 20800, [2267.4729, -5184.6733, 34.2687]),
        (1, 20850, [2245.7932, -5288.2749, 30.1250]),
        (0, 20850, [2267.2651, -5183.6758, 34.3112]),
        (1, 20900, [2244.8589, -5283.9243, 30.1250]),
        (1, 20950, [2243.9248, -5279.5732, 30.2287]),
        (1, 21000, [2242.9792, -5275.1758, 30.4159]),
        (1, 21050, [2242.0337, -5270.7783, 30.6031]),
        (1, 21100, [2241.0989, -5266.4297, 30.7882]),
        (1, 21150, [2240.1643, -5262.0811, 30.9733]),
        (1, 21200, [2239.3977, -5258.5137, 31.1251]),
        (1, 21250, [2238.6313, -5254.9473, 31.2770]),
        (1, 21300, [2238.0120, -5252.0645, 31.3997]),
        (1, 21350, [2237.3931, -5249.1821, 31.5224]),
        (1, 21400, [2236.8896, -5246.8369, 31.6222]),
        (1, 21450, [2236.3994, -5244.5518, 31.7195]),
        (1, 21500, [2235.9907, -5242.6465, 31.8006]),
        (1, 21550, [2235.6516, -5241.0649, 31.8680]),
        (1, 21600, [2235.4260, -5240.0122, 31.9128]),
        (1, 21650, [2235.2249, -5239.0742, 31.9528]),
    ];
    let off = walks_off_the_trace(&vfs, "maps/yavin2.bsp", &names, &traced, -50);
    assert!(
        off.is_empty(),
        "{} frames off: {:?}",
        off.len(),
        &off[..off.len().min(4)]
    );
}

/// academy4's intro puts Luke at putlukehere (`SET_ORIGIN`, `SET_ANGLES`: a unit
/// above the floor, which he drops to, facing 135), then walks him to lukenav1, 120
/// units off, as the original does, every traced frame: on the walked capture's
/// clock (the other capture of the scene has him 100 ms later, the original's voice
/// lines playing in real time).
#[test]
#[allow(clippy::excessive_precision)]
fn academy4s_luke_walks_from_where_the_intro_puts_him() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let names = ["npc_luke1"];
    // The traced positions as the trace logs them, from where he stands to walk
    // (a4-walked-npcs.txt).
    let traced = [
        (0, 64750, [-1426.6248, -1106.4729, 672.1250]),
        (0, 64800, [-1427.3063, -1104.8069, 672.1250]),
        (0, 64850, [-1428.0446, -1103.0021, 672.1250]),
        (0, 64900, [-1428.8397, -1101.0585, 672.1250]),
        (0, 64950, [-1429.6803, -1099.0038, 672.1250]),
        (0, 65000, [-1430.5663, -1096.8380, 672.1250]),
        (0, 65050, [-1431.5090, -1094.5334, 672.1250]),
        (0, 65100, [-1432.5087, -1092.0900, 672.1250]),
        (0, 65150, [-1433.5499, -1089.5447, 672.1250]),
        (0, 65200, [-1434.5912, -1086.9994, 672.1250]),
        (0, 65250, [-1435.6324, -1084.4541, 672.1250]),
        (0, 65300, [-1436.6737, -1081.9088, 672.1250]),
        (0, 65350, [-1437.7150, -1079.3635, 672.1250]),
        (0, 65400, [-1438.7562, -1076.8182, 672.1250]),
        (0, 65450, [-1439.7975, -1074.2729, 672.1250]),
        (0, 65500, [-1440.8387, -1071.7277, 672.1250]),
        (0, 65550, [-1441.8800, -1069.1824, 672.1250]),
        (0, 65600, [-1442.9213, -1066.6371, 672.1250]),
        (0, 65650, [-1443.9625, -1064.0918, 672.1250]),
        (0, 65700, [-1445.0038, -1061.5465, 672.1250]),
        (0, 65750, [-1445.9692, -1059.1864, 672.1250]),
        (0, 65800, [-1446.9347, -1056.8263, 672.1250]),
        (0, 65850, [-1447.7866, -1054.7438, 672.1250]),
        (0, 65900, [-1448.5818, -1052.8002, 672.1250]),
        (0, 65950, [-1449.2633, -1051.1342, 672.1250]),
        (0, 66000, [-1449.8313, -1049.7458, 672.1250]),
        (0, 66050, [-1450.2402, -1048.7462, 672.1250]),
        (0, 66100, [-1450.6492, -1047.7466, 672.1250]),
        (0, 66150, [-1451.0354, -1046.8025, 672.1250]),
        (0, 66200, [-1451.4216, -1045.8584, 672.1250]),
        (0, 66250, [-1451.7852, -1044.9698, 672.1250]),
        (0, 66300, [-1452.1487, -1044.0813, 672.1250]),
        (0, 66350, [-1452.4781, -1043.2761, 672.1250]),
        (0, 66400, [-1452.8076, -1042.4709, 672.1250]),
        (0, 66450, [-1453.1143, -1041.7212, 672.1250]),
        (0, 66500, [-1453.4209, -1040.9714, 672.1250]),
        (0, 66550, [-1453.7048, -1040.2772, 672.1250]),
        (0, 66600, [-1453.9888, -1039.5830, 672.1250]),
        (0, 66650, [-1454.2614, -1038.9166, 672.1250]),
        (0, 66700, [-1454.5339, -1038.2502, 672.1250]),
        (0, 66750, [-1454.7838, -1037.6394, 672.1250]),
        (0, 66800, [-1455.0337, -1037.0286, 672.1250]),
        (0, 66850, [-1455.2722, -1036.4454, 672.1250]),
        (0, 66900, [-1455.5107, -1035.8623, 672.1250]),
    ];
    // Where he stands before his first step, as the trace logs him at 64700,
    let w = walkers(&vfs, "maps/academy4.bsp", &names, &|_| None);
    let (start, t) = &w[0];
    let (_, o) = t.moves.iter().rfind(|(l, _)| *l < 64750).unwrap();
    assert_eq!(
        [0, 1, 2].map(|k| start[k] + o[k]),
        [-1426.0, -1108.0, 672.125]
    );
    // facing putlukehere's 135 as SET_ANGLES leaves his view.
    let (_, yaw) = t.yaw.iter().rfind(|(l, _)| *l < 64750).unwrap();
    assert_eq!(*yaw, 135.0);
    let off = walks_off_the_trace(&vfs, "maps/academy4.bsp", &names, &traced, 0);
    assert!(
        off.is_empty(),
        "{} frames off: {:?}",
        off.len(),
        &off[..off.len().min(4)]
    );
}

/// The entity numbers the NPCs that begin each map take, as the original's traces
/// log them (`swjkja npc <number>`), and those of the intros' runners there:
/// t1_sour's comes after its walkers (144, 146), academy3's before Luke (3).
#[test]
fn npcs_and_runners_are_numbered_as_in_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let traced: [(&str, &[i32]); 8] = [
        (
            "academy1",
            &[
                2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 25,
            ],
        ),
        (
            "academy2",
            &[2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 21],
        ),
        ("academy3", &[3, 4, 7]),
        (
            "academy4",
            &[1, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20],
        ),
        (
            "academy6",
            &[1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 29],
        ),
        ("kor1", &[142, 477, 511, 570]),
        ("kor2", &[92, 94, 96, 98, 164, 166, 172, 242, 243]),
        (
            "t1_sour",
            &[
                13, 129, 144, 146, 161, 166, 171, 183, 193, 241, 321, 338, 340, 344, 346,
            ],
        ),
    ];
    for (map, npcs) in traced {
        let bsp = Bsp::parse(&vfs.read(&format!("maps/{map}.bsp")).unwrap()).unwrap();
        let numbers = swjkja_world::entity_number::entity_numbers(&bsp.entities, 0);
        let mut got: Vec<i32> = numbers.npc.iter().flatten().copied().collect();
        got.sort_unstable();
        assert_eq!(got, npcs, "{map}");
    }
    let runner = |map: &str, name: &str| {
        let bsp = Bsp::parse(&vfs.read(&format!("maps/{map}.bsp")).unwrap()).unwrap();
        let level = script::Level::of(&bsp, 0, &|_| Some(Vec::new()));
        let r = level
            .runners
            .iter()
            .find(|r| r.targetname.as_deref() == Some(name));
        r.unwrap().number
    };
    assert_eq!(runner("t1_sour", "set"), 336);
    assert_eq!(runner("academy3", "t399"), 2);
}
