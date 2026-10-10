//! The static models of mp/ffa5 and which ones a view draws. Runs only when
//! SWJKJA_GAMEDATA points at the retail `GameData` folder; needs no GPU.

use std::path::PathBuf;

use swjkja_render::Camera;
use swjkja_render::fog::FogVolume;
use swjkja_render::models::{StaticModels, sp_fog_num};
use swjkja_render::vis::{Frustum, SurfaceCull, WorldVis};
use swjkja_world::static_model::Rules;

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
    let mut models = StaticModels::load(
        &vfs,
        &bsp,
        &fogs,
        swjkja_world::static_model::Rules::for_map("maps/mp/ffa5.bsp"),
    );
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
/// if no retail model's choice changes with the view.
#[test]
fn no_single_player_model_fog_depends_on_the_view() {
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
        for p in swjkja_world::static_model::static_models(&bsp.entities, rules) {
            let model = md3s
                .entry(p.model.to_ascii_lowercase())
                .or_insert_with(|| swjkja_model::md3::Md3Model::load(&vfs, &p.model).ok());
            let Some(model) = model else { continue };
            placed += 1;
            for md3 in &model.lods {
                let outside = sp_fog_num(&fogs, p.origin, md3, 0);
                fogged += usize::from(outside != 0);
                for view_fog in 1..=fogs.len() {
                    assert_eq!(
                        sp_fog_num(&fogs, p.origin, md3, view_fog),
                        outside,
                        "{name}: {} at {:?}, view in fog {view_fog}",
                        p.model,
                        p.origin
                    );
                }
            }
        }
    }
    eprintln!("{placed} single-player static models, {fogged} fogged levels");
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
    let mut models = StaticModels::load(&vfs, &bsp, &[], Rules::for_map("maps/t2_dpred.bsp"));
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
