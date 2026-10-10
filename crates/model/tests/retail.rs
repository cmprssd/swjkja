//! Parses every MD3 in the retail data, and registers every model the maps place.
//! Runs only when SWJKJA_GAMEDATA points at the retail `GameData` folder
//! (`.local-ci` sets it for the Windows run).

use std::collections::BTreeSet;
use std::path::PathBuf;

use swjkja_bsp::Bsp;
use swjkja_fs::Vfs;
use swjkja_model::md3::{MAX_LODS, Md3, Md3Model, lod_file};

fn retail() -> Option<Vfs> {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return None;
    };
    Some(Vfs::mount_game(&[gamedata], &["base"]).unwrap())
}

#[test]
fn every_retail_md3_parses() {
    let Some(vfs) = retail() else { return };
    let mut names: Vec<&str> = vfs.files().filter(|f| f.ends_with(".md3")).collect();
    names.sort_unstable();
    let (mut surfaces, mut triangles) = (0, 0);
    for name in &names {
        let md3 = Md3::parse(&vfs.read(name).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        surfaces += md3.surfaces.len();
        triangles += md3
            .surfaces
            .iter()
            .map(|s| s.indexes.len() / 3)
            .sum::<usize>();
    }
    eprintln!(
        "{} md3s, {surfaces} surfaces, {triangles} triangles",
        names.len()
    );
    // Counted independently with Python's zipfile: 595 unique .md3 names.
    assert_eq!(names.len(), 595);
}

#[test]
fn every_model_the_maps_place_registers() {
    let Some(vfs) = retail() else { return };
    let mut models = BTreeSet::new();
    for map in vfs.files().filter(|f| f.ends_with(".bsp")) {
        let bsp = Bsp::parse(&vfs.read(map).unwrap()).unwrap();
        for e in &bsp.entities {
            // The racks' spawn functions draw fixed models whatever the key says
            // (`models/map_objects/kejim/weaponsrack.md3` and `weaponsrung.md3`), and
            // 21 of their keys name models the retail data lacks.
            let class = e.get("classname").unwrap_or_default();
            if matches!(class, "misc_model_gun_rack" | "misc_model_ammo_rack") {
                continue;
            }
            if let Some(m) = e
                .get("model")
                .filter(|m| m.to_ascii_lowercase().ends_with(".md3"))
            {
                models.insert(m.to_ascii_lowercase());
            }
        }
    }
    models.insert("models/map_objects/kejim/weaponsrack.md3".into());
    models.insert("models/map_objects/kejim/weaponsrung.md3".into());
    let mut lods = [0usize; MAX_LODS + 1];
    for name in &models {
        let model = Md3Model::load(&vfs, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        // Levels are never missing in between (the original would leave a slot empty).
        let files = (0..MAX_LODS)
            .filter(|&l| vfs.contains(&lod_file(name, l)))
            .count();
        assert_eq!(model.lods.len(), files, "{name}");
        lods[model.lods.len()] += 1;
    }
    eprintln!("{} models placed; by level count: {lods:?}", models.len());
    // Counted independently with Python's zipfile: 380 models, and only
    // hoth/bacta_tank.md3 has a second level (bacta_tank_1.md3).
    assert_eq!(models.len(), 380);
    assert_eq!(lods, [0, 379, 1, 0]);
}
