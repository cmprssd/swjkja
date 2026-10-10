use swjkja_bsp::Entity;
use swjkja_world::static_model::{Rules, angles_to_axis, entity_models, static_models};

fn entity(pairs: &[(&str, &str)]) -> Entity {
    Entity {
        pairs: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
}

const FFA: Rules = Rules::Multiplayer { gametype: 0 };
const SP: Rules = Rules::SinglePlayer { skill: 0 };

#[test]
fn spawns_with_origin_angles_scale_and_zoffset() {
    let models = static_models(
        &[
            entity(&[("classname", "worldspawn")]),
            entity(&[
                ("classname", "misc_model_static"),
                ("model", "models/a.md3"),
                ("origin", "10 20 30"),
                ("angle", "90"),
                ("modelscale", "2"),
                ("zoffset", "-8"),
            ]),
        ],
        FFA,
    );
    let [m] = &models[..] else {
        panic!("{models:?}")
    };
    assert_eq!(m.model, "models/a.md3");
    assert_eq!(m.origin, [10.0, 20.0, 30.0]);
    assert_eq!(m.zoffset, -8.0);
    assert_eq!(m.scale, [2.0; 3]);
    // Yaw 90: forward is +y, left is -x; each axis doubled.
    assert!(close(m.axes[0], [0.0, 2.0, 0.0]), "{:?}", m.axes);
    assert!(close(m.axes[1], [-2.0, 0.0, 0.0]), "{:?}", m.axes);
    assert!(close(m.axes[2], [0.0, 0.0, 2.0]), "{:?}", m.axes);
}

#[test]
fn angles_win_over_angle_and_scale_vec_over_scale() {
    let models = static_models(
        &[entity(&[
            ("classname", "misc_model_static"),
            ("model", "m.md3"),
            ("angles", "0 180 0"),
            ("angle", "90"),
            ("modelscale_vec", "1 2 3"),
            ("modelscale", "5"),
        ])],
        FFA,
    );
    assert_eq!(models[0].scale, [1.0, 2.0, 3.0]);
    assert!(close(models[0].axes[0], [-1.0, 0.0, 0.0]));
    assert!(close(models[0].axes[1], [0.0, -2.0, 0.0]));
    assert!(close(models[0].axes[2], [0.0, 0.0, 3.0]));
}

#[test]
fn malformed_scale_vec_reads_as_zero() {
    let models = static_models(
        &[entity(&[
            ("classname", "misc_model_static"),
            ("model", "m.md3"),
            ("modelscale_vec", "1 2"),
        ])],
        FFA,
    );
    assert_eq!(models[0].scale, [0.0; 3]);
}

#[test]
fn game_type_filters_and_missing_models() {
    let model = |extra: &[(&str, &str)]| {
        let mut pairs = vec![("classname", "misc_model_static"), ("model", "m.md3")];
        pairs.extend_from_slice(extra);
        entity(&pairs)
    };
    let entities = [
        model(&[]),
        model(&[("notfree", "1")]),
        model(&[("notteam", "1")]),
        model(&[("gametype", "ctf siege")]),
        model(&[("gametype", "ffa duel")]),
        entity(&[("classname", "misc_model_static")]),
        entity(&[("classname", "misc_model_breakable"), ("model", "m.md3")]),
    ];
    // Free-for-all: the plain one, the notteam one and the ffa one.
    assert_eq!(static_models(&entities, FFA).len(), 3);
    // CTF (8): the plain one, the notfree one and the ctf one.
    assert_eq!(
        static_models(&entities, Rules::Multiplayer { gametype: 8 }).len(),
        3
    );
}

#[test]
fn axes_follow_angle_vectors() {
    // Pitch 90 looks straight down: forward is -z, up is +x.
    let [f, l, u] = angles_to_axis([90.0, 0.0, 0.0]);
    assert!(close(f, [0.0, 0.0, -1.0]), "{f:?}");
    assert!(close(l, [0.0, 1.0, 0.0]), "{l:?}");
    assert!(close(u, [1.0, 0.0, 0.0]), "{u:?}");
}

#[test]
fn maps_outside_mp_follow_single_player_rules() {
    assert_eq!(Rules::for_map("maps/mp/ffa5.bsp"), FFA);
    assert_eq!(Rules::for_map("MAPS\\MP\\ffa5.bsp"), FFA);
    assert_eq!(Rules::for_map("maps/t1_fatal.bsp"), SP);
    assert_eq!(Rules::for_map("maps/academy1.bsp"), SP);
}

#[test]
fn single_player_scales_only_axes_with_a_scale_other_than_0_and_1() {
    let models = static_models(
        &[
            entity(&[
                ("classname", "misc_model_static"),
                ("model", "m.md3"),
                ("modelscale_vec", "0 1 3"),
            ]),
            entity(&[("classname", "misc_model_static"), ("model", "m.md3")]),
        ],
        SP,
    );
    let [scaled, plain] = &models[..] else {
        panic!("{models:?}")
    };
    assert_eq!(scaled.scale, [0.0, 1.0, 3.0]);
    assert!(close(scaled.axes[0], [1.0, 0.0, 0.0]), "{:?}", scaled.axes);
    assert!(close(scaled.axes[1], [0.0, 1.0, 0.0]), "{:?}", scaled.axes);
    assert!(close(scaled.axes[2], [0.0, 0.0, 3.0]), "{:?}", scaled.axes);
    assert!(!scaled.normalized);
    assert_eq!(plain.scale, [1.0; 3]);
    assert!(plain.normalized);
}

#[test]
fn single_player_modelscale_wins_and_the_last_angle_key_counts() {
    let models = static_models(
        &[entity(&[
            ("classname", "misc_model_static"),
            ("model", "m.md3"),
            ("angles", "0 180 0"),
            ("angle", "90"),
            ("modelscale_vec", "1 2 3"),
            ("modelscale", "2"),
            ("zoffset", "4"),
        ])],
        SP,
    );
    let m = &models[0];
    assert_eq!(m.scale, [2.0; 3]);
    assert_eq!(m.zoffset, 4.0);
    // Yaw 90 from "angle", which came last.
    assert!(close(m.axes[0], [0.0, 2.0, 0.0]), "{:?}", m.axes);
}

#[test]
fn single_player_filters_by_skill_notsingle_and_model() {
    let model = |extra: &[(&str, &str)]| {
        let mut pairs = vec![("classname", "misc_model_static"), ("model", "m.md3")];
        pairs.extend_from_slice(extra);
        entity(&pairs)
    };
    let entities = [
        model(&[]),
        model(&[("notsingle", "1")]),
        model(&[("notfree", "1")]),
        model(&[("spawnflags", "256")]),
        model(&[("spawnflags", "512")]),
        model(&[("spawnflags", "1536")]),
        entity(&[("classname", "misc_model_static"), ("model", "m.glm")]),
    ];
    let count = |skill| static_models(&entities, Rules::SinglePlayer { skill }).len();
    // Easy leaves out 256; the multiplayer keys do not count.
    assert_eq!(count(0), 4);
    // Medium leaves out 512 and 1536.
    assert_eq!(count(1), 3);
    // Hard leaves out 1536.
    assert_eq!(count(2), 4);
}

fn breakable(extra: &[(&str, &str)]) -> Entity {
    let mut pairs = vec![
        ("classname", "misc_model_breakable"),
        ("model", "m.md3"),
        ("origin", "10 20 30"),
    ];
    pairs.extend_from_slice(extra);
    entity(&pairs)
}

#[test]
fn breakables_spawn_in_single_player_unless_started_off() {
    let entities = [
        breakable(&[]),
        breakable(&[("spawnflags", "4096")]),
        breakable(&[("spawnflags", "4162")]),
        // Left out at easy, like any single-player entity.
        breakable(&[("spawnflags", "256")]),
        // Animated: drawn at its first frame.
        breakable(&[("spawnflags", "2")]),
        breakable(&[("notsingle", "1")]),
        // Dropped to the floor in the original: left out until there is collision.
        breakable(&[("gravity", "1")]),
        breakable(&[("gravity", "0")]),
        entity(&[("classname", "misc_model_breakable"), ("origin", "0 0 0")]),
    ];
    assert_eq!(entity_models(&entities, SP).len(), 3);
    assert_eq!(
        entity_models(&entities, Rules::SinglePlayer { skill: 1 }).len(),
        4
    );
    // Multiplayer maps place none, and its game would not spawn these.
    assert!(entity_models(&entities, FFA).is_empty());
    // Static models are not breakables, nor the other way round.
    assert!(static_models(&entities, SP).is_empty());
}

#[test]
fn a_breakable_is_linked_by_its_box_an_epsilon_larger() {
    let models = entity_models(
        &[
            breakable(&[("angle", "90")]),
            breakable(&[("mins", "-8 -8 0"), ("maxs", "8 8 40")]),
            breakable(&[("mins", "0 0 0"), ("maxs", "8 8 8")]),
        ],
        SP,
    );
    let [default, sized, zero_mins] = &models[..] else {
        panic!("{models:?}")
    };
    // No mins and maxs: 16 units each way.
    assert_eq!(
        default.entity_box,
        Some([[-7.0, 3.0, 13.0], [27.0, 37.0, 47.0]])
    );
    assert!(default.normalized);
    assert_eq!(default.zoffset, 0.0);
    assert!(
        close(default.axes[0], [0.0, 1.0, 0.0]),
        "{:?}",
        default.axes
    );
    assert_eq!(
        sized.entity_box,
        Some([[1.0, 11.0, 29.0], [19.0, 29.0, 71.0]])
    );
    // All-zero mins read as unset.
    assert_eq!(
        zero_mins.entity_box,
        Some([[-7.0, 3.0, 13.0], [19.0, 29.0, 39.0]])
    );
}

#[test]
fn a_scaled_breakable_scales_its_box_and_rises_by_its_sunken_bottom() {
    let models = entity_models(
        &[
            breakable(&[("modelscale", "2")]),
            breakable(&[("modelscale_vec", "1 1 0.5"), ("modelscale", "3")]),
            breakable(&[("model", "models/map_objects/ships/TIE_bomber.md3")]),
        ],
        SP,
    );
    let [doubled, squashed, bomber] = &models[..] else {
        panic!("{models:?}")
    };
    // The box's bottom sinks from -16 to -32, so the origin rises 16.
    assert_eq!(doubled.origin, [10.0, 20.0, 46.0]);
    assert_eq!(
        doubled.entity_box,
        Some([[-23.0, -13.0, 13.0], [43.0, 53.0, 79.0]])
    );
    assert!(
        close(doubled.axes[2], [0.0, 0.0, 2.0]),
        "{:?}",
        doubled.axes
    );
    assert!(!doubled.normalized);
    // "modelscale_vec" wins; x and y keep unit axes, z is halved and rises -8.
    assert_eq!(squashed.scale, [1.0, 1.0, 0.5]);
    assert_eq!(squashed.origin, [10.0, 20.0, 22.0]);
    assert!(
        close(squashed.axes[0], [1.0, 0.0, 0.0]),
        "{:?}",
        squashed.axes
    );
    assert!(
        close(squashed.axes[2], [0.0, 0.0, 0.5]),
        "{:?}",
        squashed.axes
    );
    // The tie bomber's box is fixed at 80 units each way.
    assert_eq!(
        bomber.entity_box,
        Some([[-71.0, -61.0, -51.0], [91.0, 101.0, 111.0]])
    );
}

fn rack(class: &str, extra: &[(&str, &str)]) -> Entity {
    let mut pairs = vec![
        ("classname", class),
        ("model", "models/map_objects/imperial/weaponsrung.md3"),
        ("origin", "100 200 50"),
        ("angle", "90"),
    ];
    pairs.extend_from_slice(extra);
    entity(&pairs)
}

#[test]
fn racks_draw_their_own_model_linked_by_a_point() {
    // The gun rack's weapons are Ghoul2 models, not drawn yet.
    let models = entity_models(&[rack("misc_model_gun_rack", &[("spawnflags", "7")])], SP);
    let [gun_rack] = &models[..] else {
        panic!("{models:?}")
    };
    assert_eq!(gun_rack.model, "models/map_objects/kejim/weaponsrack.md3");
    assert_eq!(gun_rack.origin, [100.0, 200.0, 50.0]);
    assert!(
        close(gun_rack.axes[0], [0.0, 1.0, 0.0]),
        "{:?}",
        gun_rack.axes
    );
    assert!(gun_rack.normalized);
    assert_eq!(
        gun_rack.entity_box,
        Some([[99.0, 199.0, 49.0], [101.0, 201.0, 51.0]])
    );
    let models = entity_models(&[rack("misc_model_ammo_rack", &[])], SP);
    assert_eq!(models[0].model, "models/map_objects/kejim/weaponsrung.md3");
    assert_eq!(models[0].entity_box, gun_rack.entity_box);
}

#[test]
fn an_ammo_rack_fills_its_shelf_with_the_first_ammo() {
    let models = entity_models(&[rack("misc_model_ammo_rack", &[])], SP);
    let [_, goods @ ..] = &models[..] else {
        panic!("{models:?}")
    };
    // No ammo checked: blaster packs, 8 units apart along the rack's right (+x at
    // yaw 90), 7 units up, turned 90 degrees from the rack.
    let origins: Vec<_> = goods.iter().map(|m| m.origin).collect();
    assert_eq!(
        origins,
        [
            [92.0, 200.0, 57.0],
            [100.0, 200.0, 57.0],
            [108.0, 200.0, 57.0]
        ]
    );
    for m in goods {
        assert_eq!(m.model, "models/items/energy_cell.md3");
        assert!(close(m.axes[0], [-1.0, 0.0, 0.0]), "{:?}", m.axes);
    }
    assert_eq!(
        goods[0].entity_box,
        Some([[83.0, 191.0, 56.0], [101.0, 209.0, 74.0]])
    );
}

#[test]
fn an_ammo_rack_places_checked_ammo_then_a_medpack_beside_the_weapon() {
    let models = |flags: &str| {
        entity_models(
            &[rack("misc_model_ammo_rack", &[("spawnflags", flags)])],
            SP,
        )
        .into_iter()
        .skip(1)
        .map(|m| (m.model, m.origin))
        .collect::<Vec<_>>()
    };
    let bolts = || "models/items/metallic_bolts.md3".to_owned();
    let medpac = || "models/items/medpac.md3".to_owned();
    // Metal bolts, health, no fill: one pack, and the medpack on the right, higher.
    assert_eq!(
        models("82"),
        [
            (bolts(), [92.0, 200.0, 57.0]),
            (medpac(), [106.0, 200.0, 24.0 + 50.0])
        ]
    );
    // With a weapon (a repeater, not drawn) on the right, the medpack goes left.
    assert_eq!(models("90")[1], (medpac(), [94.0, 200.0, 74.0]));
    // A power cell's rack takes no weapon, so the medpack stays right.
    assert_eq!(models("120")[1].1, [106.0, 200.0, 74.0]);
    // Every ammo type, in the original's order: one each.
    let all: Vec<_> = models("103").into_iter().map(|(m, _)| m).collect();
    assert_eq!(
        all,
        [
            "models/items/energy_cell.md3",
            "models/items/metallic_bolts.md3",
            "models/items/power_cell.md3",
            "models/items/rockets.md3",
        ]
    );
}

#[test]
fn rack_goods_come_after_every_map_entity() {
    let models = entity_models(
        &[
            rack("misc_model_ammo_rack", &[("spawnflags", "65")]),
            breakable(&[]),
        ],
        SP,
    );
    let names: Vec<_> = models.iter().map(|m| m.model.as_str()).collect();
    assert_eq!(
        names,
        [
            "models/map_objects/kejim/weaponsrung.md3",
            "m.md3",
            "models/items/energy_cell.md3"
        ]
    );
}

#[test]
fn beacons_and_bombs_wait_for_a_targetname_to_be_used() {
    let at = |class| {
        entity(&[
            ("classname", class),
            ("origin", "0 0 0"),
            ("model", "x.md3"),
        ])
    };
    let named = |class| {
        entity(&[
            ("classname", class),
            ("origin", "0 0 0"),
            ("targetname", "bigbomb"),
        ])
    };
    let models = entity_models(
        &[
            at("misc_model_beacon"),
            at("misc_model_bomb_planted"),
            named("misc_model_beacon"),
            named("misc_model_bomb_planted"),
        ],
        SP,
    );
    let [beacon, bomb] = &models[..] else {
        panic!("{models:?}")
    };
    assert_eq!(beacon.model, "models/map_objects/wedge/beacon.md3");
    assert_eq!(
        beacon.entity_box,
        Some([[-17.0, -17.0, -1.0], [17.0, 17.0, 25.0]])
    );
    assert_eq!(bomb.model, "models/map_objects/factory/bomb_new_deact.md3");
    assert_eq!(
        bomb.entity_box,
        Some([[-17.0, -17.0, -1.0], [17.0, 17.0, 71.0]])
    );
}
