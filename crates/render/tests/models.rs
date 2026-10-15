//! The static models of mp/ffa5 and which ones a view draws. Runs only when
//! SWJKJA_GAMEDATA points at the retail `GameData` folder; needs no GPU.

use std::path::PathBuf;

use swjkja_render::Camera;
use swjkja_render::fog::FogVolume;
use swjkja_render::models::{StaticModels, md3_fog_sphere, sp_fog_num};
use swjkja_render::vis::{Frustum, SurfaceCull, WorldVis};
use swjkja_world::static_model::{ITEM_RADIUS, Rules};

#[test]
fn ffa5_static_models_cull_like_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/mp/ffa5.bsp").unwrap()).unwrap();
    let parms = swjkja_shader::FogParms {
        color: [1.0; 3],
        depth_for_opaque: 250.0,
    };
    let fogs: Vec<FogVolume> = (0..bsp.fogs.len())
        .map(|i| FogVolume::load(&bsp, i, &parms))
        .collect();
    let mut models = StaticModels::load(&vfs, &bsp, &fogs, "maps/mp/ffa5.bsp", 0);
    let vis = WorldVis::new(&bsp, vec![SurfaceCull::Never; bsp.surfaces.len()]);
    models.locate(&vis);
    assert_eq!(models.instances.len(), 84);
    // 48 lava rings and 36 others, one surface each but the stations and pipes.
    let find = |origin: [f32; 3]| {
        models
            .instances
            .iter()
            .position(|m| m.origin == origin)
            .unwrap()
    };

    // The rings view (bench/reference/model-views.txt): eye at a spawn point on the
    // level of the rings at z 576, looking at the lava column.
    let camera = Camera::new([-2024.0, -1296.0, 572.0], [0.0, 342.0, 0.0], 1280, 720);
    let frustum = Frustum::new(&camera);
    let cluster = vis.cluster(camera.origin);
    let drawn = |origin| models.visible(find(origin), &camera, &frustum, cluster, &vis);
    // The four rings at eye level.
    for ring in [
        [-1464.0, -1464.0, 576.0],
        [-1464.0, -1480.0, 576.0],
        [-1480.0, -1464.0, 576.0],
        [-1480.0, -1480.0, 576.0],
    ] {
        assert_eq!(drawn(ring), Some(0), "{ring:?}");
    }
    // The rings 1200 units up are above the view.
    assert_eq!(drawn([-1464.0, -1464.0, 1792.0]), None);
    // The control station 74 degrees to the left, outside the 80-degree view.
    assert_eq!(drawn([-1828.0, -1004.0, 512.0]), None);
    // The other lava column, across the map: not in this view's PVS.
    assert_eq!(drawn([120.0, 2296.0, 640.0]), None);
    let total = (0..models.instances.len())
        .filter(|&n| {
            models
                .visible(n, &camera, &frustum, cluster, &vis)
                .is_some()
        })
        .count();
    eprintln!("{total} of 84 models drawn in the rings view");
}

/// Single player's fog choice depends on the view's fog when a model's box straddles
/// fogs; the renderer bakes the choice seen from outside any fog, which is right only
/// if no retail model's choice changes with the view. One does (a known difference).
#[test]
fn only_one_single_player_model_fog_depends_on_the_view() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let shaders = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let mut md3s = std::collections::HashMap::new();
    let (mut placed, mut fogged) = (0, 0);
    for name in vfs.files().filter(|f| f.ends_with(".bsp")) {
        let rules = Rules::for_map(name);
        if !matches!(rules, Rules::SinglePlayer { .. }) {
            continue;
        }
        let bsp = swjkja_bsp::Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let fogs: Vec<FogVolume> = (0..bsp.fogs.len())
            .map(|i| {
                let shader = swjkja_shader::resolve::strip_extension(bsp.fogs[i].shader());
                let parms = match shaders.parse(shader) {
                    Some(Ok(def)) if def.fog_parms.is_some() => def.fog_parms.unwrap(),
                    _ => swjkja_shader::FogParms {
                        color: [1.0, 0.0, 0.0],
                        depth_for_opaque: 250.0,
                    },
                };
                FogVolume::load(&bsp, i, &parms)
            })
            .collect();
        let mut placements = swjkja_world::static_model::entity_models(
            &bsp.entities,
            rules,
            &mut swjkja_world::collision::Collision::at_spawn(&bsp, rules),
        );
        placements.extend(swjkja_world::static_model::static_models(
            &bsp.entities,
            rules,
        ));
        for p in placements {
            // A Ghoul2 item's sphere is its radius around its origin; an MD3's,
            // each level's frame 0.
            let spheres = if let Some(g) = &p.ghoul2 {
                vec![(p.origin, g.radius)]
            } else {
                let model = md3s
                    .entry(p.model.to_ascii_lowercase())
                    .or_insert_with(|| swjkja_model::md3::Md3Model::load(&vfs, &p.model).ok());
                let Some(model) = model else { continue };
                model
                    .lods
                    .iter()
                    .map(|md3| md3_fog_sphere(p.origin, md3))
                    .collect()
            };
            placed += 1;
            for (center, radius) in spheres {
                let outside = sp_fog_num(&fogs, center, radius, 0);
                fogged += usize::from(outside != 0);
                for view_fog in 1..=fogs.len() {
                    // The one exception: taspir1's Raven's Claw (a breakable)
                    // straddles fogs 1 and 2, and takes fog 1 from a view inside it.
                    if name == "maps/taspir1.bsp"
                        && p.origin == [-1808.0, -1400.0, 624.0]
                        && view_fog == 1
                    {
                        assert_eq!((outside, sp_fog_num(&fogs, center, radius, 1)), (2, 1));
                        continue;
                    }
                    assert_eq!(
                        sp_fog_num(&fogs, center, radius, view_fog),
                        outside,
                        "{name}: {} at {:?}, view in fog {view_fog}",
                        p.model,
                        p.origin
                    );
                }
            }
        }
    }
    eprintln!("{placed} single-player models, {fogged} fogged levels");
    assert!(placed > 0);
}

/// Single player draws no static model while the view is in solid (`inPVS` needs
/// both points in an area, and a solid leaf has none). This eye point on t2_dpred is
/// inside a solid leaf under the trees; a reference capture from it shows none of
/// the trees this renderer drew before the rule.
#[test]
fn single_player_draws_no_models_from_a_view_in_solid() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t2_dpred.bsp").unwrap()).unwrap();
    let mut models = StaticModels::load(&vfs, &bsp, &[], "maps/t2_dpred.bsp", 0);
    let vis = WorldVis::new(&bsp, vec![SurfaceCull::Never; bsp.surfaces.len()]);
    models.locate(&vis);
    let camera = Camera::new([-2072.0, 1288.0, 580.0], [0.0, 225.0, 0.0], 1280, 720);
    let frustum = Frustum::new(&camera);
    let cluster = vis.cluster(camera.origin);
    assert_eq!(cluster, -1);
    let drawn = |cluster| {
        (0..models.instances.len())
            .filter(|&n| {
                models
                    .visible(n, &camera, &frustum, cluster, &vis)
                    .is_some()
            })
            .count()
    };
    assert_eq!(drawn(cluster), 0);
    // The tree nearest the eye (raised 512 units for culling) is inside the map, so
    // from a view inside the map it is drawn.
    let tree = models
        .instances
        .iter()
        .position(|m| m.origin == [-2284.0, 1076.0, 524.0])
        .unwrap();
    let tree_cluster = models.instances[tree].cull_cluster;
    assert!(tree_cluster >= 0);
    assert!(drawn(tree_cluster) > 0);
}

/// t3_stamp's game entities (crates and barrels, ammo racks and their goods) in the
/// stamp_barrels view (bench/reference/model-views.txt): the server sends those whose
/// box touches a cluster the view sees, the renderer culls them by the frustum.
#[test]
fn t3_stamp_entities_cull_like_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t3_stamp.bsp").unwrap()).unwrap();
    let mut models = StaticModels::load(&vfs, &bsp, &[], "maps/t3_stamp.bsp", 0);
    let vis = WorldVis::new(&bsp, vec![SurfaceCull::Never; bsp.surfaces.len()]);
    models.locate(&vis);
    let entities = models
        .instances
        .iter()
        .take_while(|m| m.entity_box.is_some())
        .count();
    // 193 breakables, 5 ammo racks and the 12 packs and medpacks on them, the 9
    // cranes func_statics draw ("model2"), then the 8 NPCs.
    assert_eq!(entities, 193 + 5 + 12 + 9 + 8);
    let npcs = entities - 8..entities;
    let entities = npcs.start;
    // The eye 350 units south of the barrels, looking north.
    let camera = Camera::new([352.0, 342.0, 124.0], [10.0, 90.0, 0.0], 1280, 720);
    let frustum = Frustum::new(&camera);
    let cluster = vis.cluster(camera.origin);
    let find = |origin: [f32; 3]| {
        models
            .instances
            .iter()
            .position(|m| m.origin == origin)
            .unwrap()
    };
    let sent = |n: usize| vis.sends(cluster, models.instances[n].link.as_ref().unwrap());
    let drawn = |n| models.visible(n, &camera, &frustum, cluster, &vis);
    // The barrels ahead, at their most detailed level.
    for barrels in [
        [352.0, 692.0, 64.0],
        [312.0, 720.0, 64.0],
        [280.0, 704.0, 64.0],
    ] {
        assert_eq!(drawn(find(barrels)), Some(0), "{barrels:?}");
    }
    // In the same hangar, so sent, but behind the view.
    let behind = find([-152.0, -1392.0, 64.0]);
    assert!(sent(behind));
    assert_eq!(drawn(behind), None);
    // In another room the view's clusters cannot see.
    let elsewhere = find([-1578.0, -1742.0, 152.0]);
    assert!(!sent(elsewhere));
    assert_eq!(drawn(elsewhere), None);
    // The ammo rack ahead (a medpack and a blaster pack): its goods sit 7 and 24
    // units up, 8 units to its left and 6 to its right (-x and +x at yaw 270).
    for rack in [
        [128.0, 762.0, 80.0],
        [136.0, 762.0, 87.0],
        [122.0, 762.0, 104.0],
    ] {
        assert!(drawn(find(rack)).is_some(), "{rack:?}");
    }
    let count = |f: &dyn Fn(usize) -> bool| (0..entities).filter(|&n| f(n)).count();
    // 77 breakables, two racks with their goods, and five cranes: four with no origin
    // (drawn at the world's, out of sight) and one in the next hangar, behind.
    assert_eq!(count(&sent), 77 + 6 + 5);
    assert_eq!(count(&|n| drawn(n).is_some()), 10 + 3);
    let npcs_drawn: Vec<_> = npcs
        .filter(|&n| drawn(n).is_some())
        .map(|n| models.instances[n].origin)
        .collect();
    // Ahead to the right: a cultist, and the mutant rancor (scale 2, lifted 24 units),
    // each standing 0.125 units off the floor (SURFACE_CLIP_EPSILON).
    assert_eq!(
        npcs_drawn,
        [[852.0, 984.0, 88.125], [858.0, 1509.0, 120.125]]
    );
}

/// t2_dpred's rack weapons are Ghoul2 items: 18 on its six gun racks and 7 on its
/// ammo racks. The dpred_guns view (bench/reference/model-views.txt) looks at two
/// gun racks from 76 units: their six weapons are drawn at full detail, culled only
/// by the item's sphere.
#[test]
fn t2_dpred_rack_weapons_draw_as_ghoul2_items() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t2_dpred.bsp").unwrap()).unwrap();
    let mut models = StaticModels::load(&vfs, &bsp, &[], "maps/t2_dpred.bsp", 0);
    let vis = WorldVis::new(&bsp, vec![SurfaceCull::Never; bsp.surfaces.len()]);
    models.locate(&vis);
    let weapons: Vec<usize> = (0..models.instances.len())
        .filter(|&n| models.instances[n].ghoul2_radius == Some(ITEM_RADIUS))
        .collect();
    assert_eq!(weapons.len(), 25);
    for &n in &weapons {
        // Every rack weapon has three levels of detail.
        assert_eq!(models.instances[n].lods.len(), 3);
    }
    let camera = Camera::new([95.0, -2030.0, 625.0], [15.0, 270.0, 0.0], 1280, 720);
    let frustum = Frustum::new(&camera);
    let cluster = vis.cluster(camera.origin);
    let drawn: Vec<(f32, usize)> = weapons
        .iter()
        .filter_map(|&n| {
            let level = models.visible(n, &camera, &frustum, cluster, &vis)?;
            Some((models.instances[n].origin[0], level))
        })
        .collect();
    // Gun racks at x 127 and 63 (in entity order), weapons 9 units apart along each
    // rack's right (+x: the racks face +y).
    assert_eq!(
        drawn,
        [
            (118.0, 0),
            (127.0, 0),
            (136.0, 0),
            (54.0, 0),
            (63.0, 0),
            (72.0, 0)
        ]
    );
}

/// t2_dpred's stormtroopers near the start (-1100 -4300) spawn with the map: drawn
/// as Ghoul2 characters with their skin, standing with their guns in hand on the
/// floor, 0.125 units off it.
#[test]
fn t2_dpred_stormtroopers_draw_skinned_and_armed() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t2_dpred.bsp").unwrap()).unwrap();
    let models = StaticModels::load(&vfs, &bsp, &[], "maps/t2_dpred.bsp", 0);
    let find = |origin: [f32; 3]| {
        models
            .instances
            .iter()
            .find(|m| (0..3).all(|i| (m.origin[i] - origin[i]).abs() < 1e-3))
            .unwrap_or_else(|| panic!("{origin:?}"))
    };
    // Its spawner is 8 units above the floor.
    let trooper = find([-2080.0, -4264.0, 24.125]);
    // A commander: scale 105, lifted 24 * 0.05 units.
    let commander = find([-738.0, -3813.0, 24.125 + 24.0 * 0.05]);
    let shaders = |m: &swjkja_render::models::Instance| -> Vec<String> {
        models.pieces[m.lods[0].pieces.clone()]
            .iter()
            .map(|p| p.shader.rsplit('/').next().unwrap().to_owned())
            .collect()
    };
    // The default skin hides the pauldron ("*off"); the officer skin shows it.
    let (t, c) = (shaders(trooper), shaders(commander));
    assert_eq!((t.len(), c.len()), (21 + 2, 20 + 2));
    // Then their guns' two surfaces: a trooper's blaster, a commander's repeater.
    assert_eq!(t[21..], ["blaster_w.tga", "blaster_w.tga"]);
    assert_eq!(c[20..], ["heavy_repeater_w.tga", "heavy_repeater_w.tga"]);
    assert!(t.contains(&"helmet.tga".into()) && !t.contains(&"shoulder.tga".into()));
    assert!(c.contains(&"shoulder.tga".into()));
    // Radius "height" (64, 68), times the scale.
    assert_eq!(trooper.ghoul2_radius, Some(64.0));
    assert!((commander.ghoul2_radius.unwrap() - 68.0 * 1.05).abs() < 1e-3);
    // Standing (legs in BOTH_STAND3): feet at the box's bottom, 24 below the
    // origin, the head (a little bent over the gun) 63 above them; the gun held
    // 20 units ahead (+x, forward in the model's space).
    let [mins, maxs] = trooper.lods[0].bounds;
    assert!(
        (mins[2] + 24.0).abs() < 0.5 && (maxs[2] - 39.0).abs() < 0.5,
        "{mins:?} {maxs:?}"
    );
    assert!((maxs[0] - 20.7).abs() < 0.5, "{mins:?} {maxs:?}");
}

/// An NPC is posed again as time passes: its bones skin its surfaces where they
/// were baked at the time it was loaded at, and elsewhere in its idle animation
/// (from 5000 ms), its gun following its hand.
#[test]
fn t2_dpred_stormtroopers_move_as_time_passes() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/t2_dpred.bsp").unwrap()).unwrap();
    let models = StaticModels::load(&vfs, &bsp, &[], "maps/t2_dpred.bsp", 3000);
    let n = models
        .instances
        .iter()
        .position(|m| (0..3).all(|i| (m.origin[i] - [-2080.0, -4264.0, 24.125][i]).abs() < 1e-3))
        .unwrap();
    // Each piece's positions skinned by the bones at `time`.
    let posed = |time| -> Vec<Vec<[f32; 3]>> {
        let bones = models.bones(n, time);
        let pieces = models.instances[n].lods[0].pieces.clone();
        pieces
            .map(|k| {
                let (surface, first) = models.posed_surface(k).unwrap();
                swjkja_model::glm::skin(surface, &bones[first..]).0
            })
            .collect()
    };
    let baked: Vec<Vec<[f32; 3]>> = models.pieces[models.instances[n].lods[0].pieces.clone()]
        .iter()
        .map(|p| match &p.geometry {
            swjkja_world::Geometry::Triangles { verts, .. } => {
                verts.iter().map(|v| v.xyz).collect()
            }
            _ => unreachable!(),
        })
        .collect();
    let near = |a: &[[f32; 3]], b: &[[f32; 3]]| {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(a, b)| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3))
    };
    // Body and gun (the last two pieces).
    let (at_load, later) = (posed(3000), posed(7000));
    assert_eq!(at_load.len(), 21 + 2);
    for (k, (posed, baked)) in at_load.iter().zip(&baked).enumerate() {
        assert!(near(posed, baked), "piece {k}");
    }
    for (k, (before, after)) in at_load.iter().zip(&later).enumerate() {
        assert!(!near(before, after), "piece {k} did not move");
    }
    // A model without a rig (a rack weapon) has no bones.
    let item = models
        .instances
        .iter()
        .position(|m| m.rig.is_none())
        .unwrap();
    assert!(models.bones(item, 7000).is_empty());
}

/// academy1's seated students look at Luke (their spawnscript's `SET_LOOK_TARGET`):
/// each settled look (pitch, yaw) as the original's capture logged it
/// (`renderInfo.lastHeadAngles`, `SCRIPTS=1` at 12345 ms), within a tenth of a
/// degree, given its point for Luke's eyes.
#[test]
fn academy1_students_look_at_luke_as_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/academy1.bsp").unwrap()).unwrap();
    let time = 12345;
    let models = StaticModels::load(&vfs, &bsp, &[], "maps/academy1.bsp", time);
    // Where each student spawned, and its look.
    let logged = [
        ([-1312.0, -976.0], [-7.136, -8.547]),
        ([-1360.0, -976.0], [-7.432, -4.471]),
        ([-1600.0, -928.0], [-9.426, 14.453]),
        ([-1488.0, -936.0], [-6.471, 6.026]),
        ([-1568.0, -992.0], [-2.834, 13.282]),
        ([-1536.0, -952.0], [-6.482, 9.965]),
        ([-1448.0, -880.0], [-1.099, 2.999]),
        ([-1336.0, -904.0], [-5.883, -5.427]),
        ([-1240.0, -984.0], [-10.486, -13.123]),
        ([-1288.0, -928.0], [-1.324, -8.036]),
        ([-1416.0, -928.0], [-10.333, 1.445]),
    ];
    let luke = models
        .instances
        .iter()
        .position(|m| m.origin[0] == -1408.0 && m.origin[1] == -1304.0)
        .unwrap();
    // The original's point for Luke's eyes: another script (`affect`, not run here)
    // has him stand in BOTH_STAND4 from 1950 ms, not his spawnscript's BOTH_CIN_4.
    let luke_eyes = [-1407.97, -1300.67, 779.98];
    let mut looking = 0;
    for n in 0..models.instances.len() {
        let Some((target, _)) = models.look(n, time) else {
            continue;
        };
        looking += 1;
        assert_eq!(target, luke);
        let look = models.look_toward(n, time, luke_eyes).unwrap();
        let o = models.instances[n].origin;
        let (_, want) = logged
            .iter()
            .find(|(at, _)| at[0] == o[0] && at[1] == o[1])
            .unwrap_or_else(|| panic!("no logged look at {o:?}"));
        assert!(
            (look[0] - want[0]).abs() < 0.1 && (look[1] - want[1]).abs() < 0.1 && look[2] == 0.0,
            "{o:?}: {look:?}, logged {want:?}"
        );
    }
    assert_eq!(looking, logged.len());
    // Before the scripts' first pass, nobody looks.
    assert!((0..models.instances.len()).all(|n| models.look(n, 1549).is_none()));
}

/// academy2's students look at Luke as the original's capture log has them, those
/// in the map's cinematic stances (`BOTH_CIN_*`, on its cinematic skeleton) too.
/// Left out: those whose scripts wait a random time and the one whose look target
/// the intro sets (`affect`, not run here).
#[test]
fn academy2_students_look_at_luke_as_the_original() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read("maps/academy2.bsp").unwrap()).unwrap();
    let time = 12345;
    let models = StaticModels::load(&vfs, &bsp, &[], "maps/academy2.bsp", time);
    let luke = models
        .instances
        .iter()
        .position(|m| m.origin[0] == -1408.0 && m.origin[1] == -1304.0)
        .unwrap();
    // Where each student spawned, and its logged look (pitch, yaw).
    let logged = [
        ([-1402.0, -992.0], [-7.004, -0.483]),
        ([-1516.0, -960.0], [-6.537, -11.508]),
        ([-1482.0, -928.0], [-5.284, -6.317]),
        ([-1336.0, -924.0], [-6.356, 2.807]),
        ([-1382.0, -912.0], [-5.169, -1.840]),
        ([-1268.0, -982.0], [-7.103, 12.887]),
        ([-1530.0, -996.0], [-6.103, -4.010]),
        ([-1308.0, -994.0], [-6.356, 13.535]),
        ([-1348.0, -960.0], [-10.245, 1.950]),
        ([-1428.0, -954.0], [-10.212, 1.593]),
        ([-1438.0, -1008.0], [-6.790, -1.983]),
        ([-1364.0, -1000.0], [-7.679, 1.538]),
    ];
    for (at, want) in logged {
        let n = models
            .instances
            .iter()
            .position(|m| m.origin[0] == at[0] && m.origin[1] == at[1])
            .unwrap();
        let (target, look) = models.look(n, time).unwrap();
        assert_eq!(target, luke);
        assert!(
            (look[0] - want[0]).abs() < 0.05 && (look[1] - want[1]).abs() < 0.05,
            "{at:?}: {look:?}, logged {want:?}"
        );
    }
}
