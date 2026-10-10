//! The static models of mp/ffa5 and which ones a view draws. Runs only when
//! SWJKJA_GAMEDATA points at the retail `GameData` folder; needs no GPU.

use std::path::PathBuf;

use swjkja_render::Camera;
use swjkja_render::fog::FogVolume;
use swjkja_render::models::StaticModels;
use swjkja_render::vis::{Frustum, SurfaceCull, WorldVis};

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
    let mut models = StaticModels::load(&vfs, &bsp, &fogs);
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
