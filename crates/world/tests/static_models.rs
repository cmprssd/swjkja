use swjkja_bsp::Entity;
use swjkja_world::collision::{CONTENTS_SOLID, Collision, MASK_NPCSOLID};
use swjkja_world::static_model::entity_models as entity_models_in;
use swjkja_world::static_model::{Rules, StaticModel, angles_to_axis, static_models};

/// The entity models, with no world to drop onto.
fn entity_models(entities: &[Entity], rules: Rules) -> Vec<StaticModel> {
    swjkja_world::static_model::entity_models(entities, rules, b"", &mut Collision::default())
}

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
        // Dropped to the floor: with none below, it stays where it is.
        breakable(&[("gravity", "1")]),
        breakable(&[("gravity", "0")]),
        entity(&[("classname", "misc_model_breakable"), ("origin", "0 0 0")]),
    ];
    assert_eq!(entity_models(&entities, SP).len(), 4);
    assert_eq!(
        entity_models(&entities, Rules::SinglePlayer { skill: 1 }).len(),
        5
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
    let models = entity_models(&[rack("misc_model_gun_rack", &[("spawnflags", "7")])], SP);
    let [gun_rack, _, _, _] = &models[..] else {
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

/// Whether `a` is within `spread` of `b` on each axis.
fn within(a: [f32; 3], b: [f32; 3], spread: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= spread[i] + 1e-4)
}

/// How far `m` stands to the right of a rack at 100 200 50 facing +y (along +x),
/// if it is 4 to `max` units aside.
fn aside(m: &StaticModel, max: f32) -> Option<f32> {
    let d = m.origin[0] - 100.0;
    (4.0..=max).contains(&d.abs()).then_some(d)
}

#[test]
fn an_ammo_rack_fills_its_shelf_with_the_first_ammo() {
    let models = entity_models(&[rack("misc_model_ammo_rack", &[])], SP);
    let [_, goods @ ..] = &models[..] else {
        panic!("{models:?}")
    };
    // No ammo checked: blaster packs, 8 units apart along the rack's right (+x at
    // yaw 90), up to half a unit forward, 7 units up, turned 90 degrees from the
    // rack and up to 4 more.
    assert_eq!(goods.len(), 3);
    for (m, x) in goods.iter().zip([92.0, 100.0, 108.0]) {
        assert_eq!(m.model, "models/items/energy_cell.md3");
        assert!(
            within(m.origin, [x, 200.0, 57.0], [0.0, 0.5, 0.0]),
            "{:?}",
            m.origin
        );
        assert!(m.axes[0][0] < -4f32.to_radians().cos(), "{:?}", m.axes);
        let [x, y, z] = m.origin;
        assert_eq!(
            m.entity_box,
            Some([[x - 9.0, y - 9.0, z - 1.0], [x + 9.0, y + 9.0, z + 17.0]])
        );
    }
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
        .collect::<Vec<_>>()
    };
    // Metal bolts, health, no fill: one pack, and the medpack 4 to 8 units to a
    // side, higher.
    let goods = models("82");
    let [bolts, medpac] = &goods[..] else {
        panic!("{goods:?}")
    };
    assert_eq!(bolts.model, "models/items/metallic_bolts.md3");
    assert!(within(bolts.origin, [92.0, 200.0, 57.0], [0.0, 0.5, 0.0]));
    assert_eq!(medpac.model, "models/items/medpac.md3");
    assert!(aside(medpac, 8.0).is_some(), "{:?}", medpac.origin);
    assert_eq!(medpac.origin[2], 74.0);
    // With a weapon (a repeater) 4 to 10 units to a side, the medpack goes to the
    // other one.
    let goods = models("90");
    let [repeater, medpac] = &goods[1..] else {
        panic!("{goods:?}")
    };
    assert_eq!(
        repeater.model,
        "models/weapons2/heavy_repeater/heavy_repeater_w.glm"
    );
    assert_eq!((repeater.origin[2], medpac.origin[2]), (77.0, 74.0));
    let (gun, med) = (aside(repeater, 10.0).unwrap(), aside(medpac, 8.0).unwrap());
    assert!(gun * med < 0.0, "{gun} and {med}");
    // A power cell's rack takes no weapon.
    assert!(aside(&models("120")[1], 8.0).is_some());
    // Every ammo type, in the original's order: one each.
    let all: Vec<_> = models("103").into_iter().map(|m| m.model).collect();
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
fn a_gun_rack_stands_its_weapons_up_turned_around() {
    let blaster = "models/weapons2/blaster_r/blaster_w.glm";
    let repeater = "models/weapons2/heavy_repeater/heavy_repeater_w.glm";
    let rocket = "models/weapons2/merr_sonn/merr_sonn_w.glm";
    let goods = |flags: &str| {
        entity_models(&[rack("misc_model_gun_rack", &[("spawnflags", flags)])], SP)
            .into_iter()
            .skip(1)
            .collect::<Vec<_>>()
    };
    // Nothing checked: three blasters, 9 units apart along the rack's right (+x at
    // yaw 90), up to 2 units aside and forward, 23 units up.
    let blasters = goods("0");
    assert_eq!(blasters.len(), 3);
    for (m, x) in blasters.iter().zip([91.0, 100.0, 109.0]) {
        assert_eq!(m.model, blaster);
        assert!(
            within(m.origin, [x, 200.0, 73.0], [2.0, 2.0, 0.0]),
            "{:?}",
            m.origin
        );
        // Facing yaw 270, up to 14 degrees off, and pitched 75 degrees up, up to 4
        // degrees off: forward mostly up, a little toward -y.
        let [fx, fy, fz] = m.axes[0];
        assert!(fz > 71f32.to_radians().sin() && fz < 79f32.to_radians().sin());
        assert!(
            fy < 0.0 && fx.abs() < -fy * 14f32.to_radians().tan(),
            "{:?}",
            m.axes
        );
        let [x, y, z] = m.origin;
        assert_eq!(
            m.entity_box,
            Some([
                [x - 11.0, y - 11.0, z - 3.0],
                [x + 11.0, y + 11.0, z + 13.0]
            ])
        );
    }
    // Each checked weapon at its own height, the first filling the shelf.
    let placed: Vec<_> = goods("6")
        .iter()
        .map(|m| (m.model.clone(), m.origin[2]))
        .collect();
    assert_eq!(
        placed,
        [
            (repeater.to_owned(), 74.5),
            (rocket.to_owned(), 75.5),
            (repeater.to_owned(), 74.5)
        ]
    );
}

#[test]
fn an_ammo_rack_weapon_with_no_ammo_checked_sits_at_the_rack_height() {
    let models = entity_models(&[rack("misc_model_ammo_rack", &[("spawnflags", "8")])], SP);
    let weapon = models.last().unwrap();
    assert_eq!(weapon.model, "models/weapons2/blaster_r/blaster_w.glm");
    assert!(aside(weapon, 10.0).is_some(), "{:?}", weapon.origin);
    assert!(within(
        weapon.origin,
        [100.0, 200.0, 50.0],
        [10.0, 2.0, 0.0]
    ));
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

#[test]
fn a_solid_breakable_is_in_the_way_of_what_spawns_after_it() {
    let crate_at = |flags| {
        entity(&[
            ("classname", "misc_model_breakable"),
            ("model", "m.md3"),
            ("origin", "0 0 16"),
            ("spawnflags", flags),
        ])
    };
    let mut world = Collision::default();
    entity_models_in(&[crate_at("1")], SP, b"", &mut world);
    // Its default box, -16..16 around its origin: its top is at 32.
    assert_eq!(
        world.drop_to_floor([0.0, 0.0, 100.0], [-1.0; 3], [1.0; 3], MASK_NPCSOLID),
        Some([0.0, 0.0, 33.125])
    );
    // Not solid: only shots stop at it.
    let mut world = Collision::default();
    entity_models_in(&[crate_at("0")], SP, b"", &mut world);
    assert_eq!(
        world.drop_to_floor([0.0, 0.0, 100.0], [-1.0; 3], [1.0; 3], MASK_NPCSOLID),
        None
    );
}

#[test]
fn a_gravity_breakable_drops_onto_what_is_below() {
    let mut world = Collision::default();
    world.add_box([-64.0, -64.0, -16.0], [64.0, 64.0, 0.0], CONTENTS_SOLID);
    let models = entity_models_in(
        &[entity(&[
            ("classname", "misc_model_breakable"),
            ("model", "m.md3"),
            ("origin", "8 8 100"),
            ("mins", "-8 -8 0"),
            ("maxs", "8 8 16"),
            ("gravity", "1"),
        ])],
        SP,
        b"",
        &mut world,
    );
    assert_eq!(models[0].origin, [8.0, 8.0, 0.125]);
}
