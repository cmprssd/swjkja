use swjkja_bsp::Entity;
use swjkja_world::sky_portal;

fn entity(pairs: &[(&str, &str)]) -> Entity {
    Entity {
        pairs: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

#[test]
fn a_map_without_a_sky_portal_has_none() {
    let world = entity(&[("classname", "worldspawn")]);
    assert_eq!(sky_portal(&[world]), None);
}

#[test]
fn the_last_sky_portal_spawned_is_the_one_drawn() {
    let ents = [
        entity(&[("classname", "worldspawn")]),
        entity(&[
            ("classname", "misc_skyportal"),
            ("origin", "-192 2688 -928"),
        ]),
        entity(&[("classname", "info_null"), ("origin", "1 2 3")]),
        entity(&[("classname", "Misc_SkyPortal"), ("origin", "4352 1024 640")]),
    ];
    assert_eq!(sky_portal(&ents), Some([4352.0, 1024.0, 640.0]));
}

#[test]
fn a_sky_portal_without_an_origin_looks_from_the_map_origin() {
    let ents = [entity(&[("classname", "misc_skyportal")])];
    assert_eq!(sky_portal(&ents), Some([0.0; 3]));
}
