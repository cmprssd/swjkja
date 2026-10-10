use swjkja_bsp::Entity;
use swjkja_world::static_model::{Rules, angles_to_axis, static_models};

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
