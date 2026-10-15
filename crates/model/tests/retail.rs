//! Parses every MD3 in the retail data, registers every model the maps place, and
//! loads every Ghoul2 mesh with its skeleton.
//! Runs only when SWJKJA_GAMEDATA points at the retail `GameData` folder
//! (`.local-ci` sets it for the Windows run).

use std::collections::BTreeSet;
use std::path::PathBuf;

use swjkja_bsp::Bsp;
use swjkja_fs::Vfs;
use swjkja_model::animation;
use swjkja_model::gla::{DEFAULT_NAME, Gla};
use swjkja_model::glm::{Ghoul2Model, skin};
use swjkja_model::md3::{MAX_LODS, Md3, Md3Model, lod_file};
use swjkja_model::skin::Skin;

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

#[test]
fn every_retail_ghoul2_model_loads_with_its_skeleton() {
    let Some(vfs) = retail() else { return };
    let mut skeletons: Vec<&str> = vfs.files().filter(|f| f.ends_with(".gla")).collect();
    skeletons.sort_unstable();
    for name in &skeletons {
        let gla = Gla::parse(&vfs.read(name).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        // Every frame poses (an index past the pool would panic).
        for frame in [0, gla.frame_count - 1] {
            assert_eq!(gla.pose(frame).len(), gla.bones.len(), "{name}");
        }
    }
    let mut meshes: Vec<&str> = vfs.files().filter(|f| f.ends_with(".glm")).collect();
    meshes.sort_unstable();
    let (mut default, mut triangles) = (0, 0);
    for name in &meshes {
        let model = Ghoul2Model::load(&vfs, name).unwrap_or_else(|e| panic!("{e}"));
        default += usize::from(model.glm.anim_name == DEFAULT_NAME);
        let pose = model.gla.pose(0);
        for lod in &model.glm.lods {
            for s in lod {
                let (xyz, normals) = skin(s, &pose);
                assert!(
                    xyz.iter().chain(&normals).flatten().all(|c| c.is_finite()),
                    "{name}"
                );
                triangles += s.indexes.len() / 3;
            }
        }
    }
    eprintln!(
        "{} glas, {} glms ({default} on the default skeleton), {triangles} triangles",
        skeletons.len(),
        meshes.len()
    );
    // Counted independently with Python's zipfile: 66 unique .gla and 127 unique
    // .glm names, 41 meshes naming "*default".
    assert_eq!((skeletons.len(), meshes.len(), default), (66, 127, 41));
}

#[test]
fn every_player_model_skin_loads_and_its_skeleton_stands() {
    let Some(vfs) = retail() else { return };
    let mut meshes: Vec<&str> = vfs
        .files()
        .filter(|f| f.starts_with("models/players/") && f.ends_with("/model.glm"))
        .collect();
    meshes.sort_unstable();
    let (mut skins, mut standing) = (0, 0);
    for name in &meshes {
        let model = Ghoul2Model::load(&vfs, name).unwrap();
        let folder = name.strip_suffix("model.glm").unwrap();
        for file in vfs.files().filter(|f| {
            f.strip_prefix(folder)
                .is_some_and(|f| !f.contains('/') && f.ends_with(".skin"))
        }) {
            let skin = Skin::load(&vfs, file).unwrap_or_else(|| panic!("{file}: no surfaces"));
            assert!(!model.glm.skinned_flags(&skin).is_empty());
            skins += 1;
        }
        if model.glm.anim_name == DEFAULT_NAME {
            continue;
        }
        let cfg = animation::config_file(&vfs, &model.glm.anim_name)
            .unwrap_or_else(|| panic!("{name}: no animation file"));
        let text = String::from_utf8_lossy(&vfs.read(&cfg).unwrap()).into_owned();
        if let Some(frame) = animation::find(&text, animation::STAND).map(|a| a.start()) {
            assert!(frame < model.gla.frame_count, "{name}: {frame}");
            standing += 1;
        }
    }
    eprintln!(
        "{} player models, {skins} skins, {standing} with a standing frame",
        meshes.len()
    );
    // Counted independently with Python's zipfile: 77 player models and 305 skins
    // beside them.
    assert_eq!((meshes.len(), skins), (77, 305));
}

#[test]
fn every_cinematic_skeleton_has_the_humanoids_bones() {
    let Some(vfs) = retail() else { return };
    let load = |name: &str| Gla::parse(&vfs.read(&format!("{name}.gla")).unwrap()).unwrap();
    let humanoid = load(animation::HUMANOID);
    let mut maps: Vec<&str> = vfs
        .files()
        .filter(|f| f.ends_with(".bsp") && !f.starts_with("maps/mp/"))
        .collect();
    maps.sort_unstable();
    let mut found = Vec::new();
    for map in maps {
        let name = animation::cinematic_skeleton(map);
        if !vfs.contains(&format!("{name}.gla")) {
            continue;
        }
        let gla = load(&name);
        // The meshes index the bones of whichever skeleton plays: the same bones,
        // in the same order, under the same parents, at the same base pose.
        assert_eq!(gla.bones, humanoid.bones, "{name}");
        let file = animation::config_file(&vfs, &name).unwrap();
        let text = String::from_utf8(vfs.read(&file).unwrap()).unwrap();
        for line in text.lines() {
            let Some(anim) = line.split_ascii_whitespace().next() else {
                continue;
            };
            if let Some(a) = animation::find(&text, anim) {
                assert!(a.first + a.count <= gla.frame_count, "{file}: {anim}");
            }
        }
        found.push(map.trim_start_matches("maps/").trim_end_matches(".bsp"));
    }
    // Counted from the scan.
    assert_eq!(found.len(), 32, "{found:?}");
}
