//! Loads and parses every retail shader script. Runs only when SWJKJA_GAMEDATA points
//! at the retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;

use swjkja_shader::{Facing, ParseError, ShaderText, SpriteKind, SurfaceSprites, Texture};

#[test]
fn every_retail_shader_loads_and_parses() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    // Counted independently with a Python script over the .pk3s (see the PR).
    assert_eq!(vfs.list("shaders", ".shader").len(), 68);
    let text = ShaderText::load(&vfs).unwrap();
    assert!(text.rejected_files.is_empty(), "{:?}", text.rejected_files);
    let names: Vec<&str> = text.names().collect();
    assert_eq!(names.len(), 3365);

    // The only shaders the original rejects are the editor and compiler-only ones
    // (caulk, clip, hint, nodraw, ...), which have no drawing stages.
    let rejected: Vec<_> = names
        .iter()
        .filter_map(|n| text.parse(n).unwrap().err().map(|e| (n, e)))
        .collect();
    assert_eq!(rejected.len(), 40);
    assert!(
        rejected.iter().all(|(_, e)| *e == ParseError::NoStages),
        "{rejected:?}"
    );

    // Defined in two files: the file listed last wins (test.shader over imp_mine.shader,
    // models.shader over imperial.shader), as the original's precedence gives.
    let first_texture = |name: &str| text.parse(name).unwrap().unwrap().stages[0].texture.clone();
    let map = |name: &str| Texture::Map {
        name: name.into(),
        clamp: false,
    };
    assert_eq!(
        first_texture("textures/impdetention/deathcon1"),
        map("textures/tests/energything2")
    );
    assert_eq!(
        first_texture("models/map_objects/cairn/utilitylight_face"),
        map("models/map_objects/cairn/utilitylight_face")
    );

    // t2_dpred's grass: the second stage of textures/imperial/dp_ground
    // (imperial.shader, read by hand).
    assert_eq!(
        text.parse("textures/imperial/dp_ground")
            .unwrap()
            .unwrap()
            .stages[1]
            .surface_sprites,
        Some(SurfaceSprites {
            kind: SpriteKind::Flattened,
            width: 48.0,
            height: 24.0,
            density: 45.0,
            fade_dist: 400.0,
            fade_max: 3000.0,
            fade_scale: 2.5,
            wind: 0.8,
            wind_idle: 0.8,
            variance: [1.0, 2.0],
            facing: Facing::Normal,
            vert_skew: 0.0,
        })
    );
}
