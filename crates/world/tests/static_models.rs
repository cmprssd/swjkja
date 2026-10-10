use swjkja_bsp::Entity;
use swjkja_world::static_model::{angles_to_axis, static_models};

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

const FFA: usize = 0;

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
    assert_eq!(static_models(&entities, 8).len(), 3);
}

#[test]
fn axes_follow_angle_vectors() {
    // Pitch 90 looks straight down: forward is -z, up is +x.
    let [f, l, u] = angles_to_axis([90.0, 0.0, 0.0]);
    assert!(close(f, [0.0, 0.0, -1.0]), "{f:?}");
    assert!(close(l, [0.0, 1.0, 0.0]), "{l:?}");
    assert!(close(u, [1.0, 0.0, 0.0]), "{u:?}");
}
