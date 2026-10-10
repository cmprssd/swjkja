use swjkja_bsp::Entity;
use swjkja_world::static_model::{Rules, angles_to_axis, breakables, static_models};

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
    assert_eq!(breakables(&entities, SP).len(), 3);
    assert_eq!(
        breakables(&entities, Rules::SinglePlayer { skill: 1 }).len(),
        4
    );
    // Multiplayer maps place none, and its game would not spawn these.
    assert!(breakables(&entities, FFA).is_empty());
    // Static models are not breakables, nor the other way round.
    assert!(static_models(&entities, SP).is_empty());
}

#[test]
fn a_breakable_is_linked_by_its_box_an_epsilon_larger() {
    let models = breakables(
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
    let models = breakables(
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
