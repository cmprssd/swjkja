use swjkja_bsp::{DrawVert, Entity, Model};
use swjkja_world::Geometry;
use swjkja_world::brush_entity::{BrushEntity, brush_entities};
use swjkja_world::static_model::Rules;

const SP: Rules = Rules::SinglePlayer { skill: 0 };

fn entity(pairs: &[(&str, &str)]) -> Entity {
    Entity {
        pairs: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

/// The world, then model 1: a 64 x 32 x 128 box around the origin.
fn models() -> Vec<Model> {
    let model = |mins, maxs| Model {
        mins,
        maxs,
        first_surface: 0,
        num_surfaces: 0,
        first_brush: 0,
        num_brushes: 0,
    };
    vec![
        model([-4096.0; 3], [4096.0; 3]),
        model([-32.0, -16.0, 0.0], [32.0, 16.0, 128.0]),
    ]
}

fn spawn(pairs: &[(&str, &str)]) -> Vec<BrushEntity> {
    brush_entities(&[entity(pairs)], &models(), SP)
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
}

#[test]
fn a_closed_door_stands_at_its_origin_in_its_linked_box() {
    let e = spawn(&[
        ("classname", "func_door"),
        ("model", "*1"),
        ("origin", "100 0 0"),
        ("angle", "90"),
    ]);
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].model, 1);
    assert_eq!(e[0].origin, [100.0, 0.0, 0.0]);
    // A door's angle is its direction of travel; it is drawn unturned.
    assert_eq!(
        e[0].axes,
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    );
    assert_eq!(e[0].entity_box, [[67.0, -17.0, -1.0], [133.0, 17.0, 129.0]]);
    assert!(!e[0].broadcast && !e[0].shader_anim);
}

#[test]
fn a_door_that_starts_open_stands_moved_by_its_size_less_the_lip() {
    // Along +y (yaw 90): 32 units of size, less the default lip of 8.
    let e = spawn(&[
        ("classname", "func_door"),
        ("model", "*1"),
        ("angle", "90"),
        ("spawnflags", "1"),
    ]);
    assert!(close(e[0].origin, [0.0, 24.0, 0.0]));
    // Straight up (yaw -1), with a lip of 28: 128 - 28.
    let e = spawn(&[
        ("classname", "func_door"),
        ("model", "*1"),
        ("angle", "-1"),
        ("lip", "28"),
        ("spawnflags", "1"),
    ]);
    assert_eq!(e[0].origin, [0.0, 0.0, 100.0]);
}

#[test]
fn a_locked_door_holds_its_shader_animation() {
    let e = spawn(&[
        ("classname", "func_door"),
        ("model", "*1"),
        ("spawnflags", "16"),
    ]);
    assert!(e[0].shader_anim);
}

#[test]
fn a_plat_starts_at_the_bottom() {
    let e = spawn(&[
        ("classname", "func_plat"),
        ("model", "*1"),
        ("origin", "0 0 500"),
    ]);
    assert_eq!(e[0].origin, [0.0, 0.0, 500.0 - 120.0]);
    let e = spawn(&[
        ("classname", "func_plat"),
        ("model", "*1"),
        ("origin", "0 0 500"),
        ("height", "64"),
    ]);
    assert_eq!(e[0].origin, [0.0, 0.0, 436.0]);
}

#[test]
fn a_turned_func_static_is_drawn_turned_and_links_a_cube_of_its_radius() {
    let e = spawn(&[
        ("classname", "func_static"),
        ("model", "*1"),
        ("origin", "10 20 30"),
        ("angle", "90"),
        ("spawnflags", "2048"),
    ]);
    assert!(close(e[0].axes[0], [0.0, 1.0, 0.0]));
    assert!(close(e[0].axes[1], [-1.0, 0.0, 0.0]));
    assert!(close(e[0].place([1.0, 0.0, 0.0]), [10.0, 21.0, 30.0]));
    // RadiusFromBounds: the farthest corner, (32, 16, 128).
    let r = (32.0f32 * 32.0 + 16.0 * 16.0 + 128.0 * 128.0).sqrt() + 1.0;
    assert!(close(e[0].entity_box[0], [10.0 - r, 20.0 - r, 30.0 - r]));
    assert!(e[0].broadcast);
}

#[test]
fn walls_and_usables_started_off_and_rail_movers_are_not_drawn() {
    for class in ["func_wall", "func_usable"] {
        assert!(spawn(&[("classname", class), ("model", "*1"), ("spawnflags", "1")]).is_empty());
        assert_eq!(spawn(&[("classname", class), ("model", "*1")]).len(), 1);
    }
    for class in ["rail_mover", "trigger_multiple", "func_train"] {
        assert!(spawn(&[("classname", class), ("model", "*1")]).is_empty());
    }
    // Nor the world model, a model the map lacks, or one left out at this skill.
    assert!(spawn(&[("classname", "func_wall"), ("model", "*0")]).is_empty());
    assert!(spawn(&[("classname", "func_wall"), ("model", "*2")]).is_empty());
    assert!(
        spawn(&[
            ("classname", "func_wall"),
            ("model", "*1"),
            ("spawnflags", "256")
        ])
        .is_empty()
    );
}

#[test]
fn a_force_visible_usable_without_an_origin_is_broadcast() {
    let pairs = [
        ("classname", "func_usable"),
        ("model", "*1"),
        ("forcevisible", "1"),
    ];
    assert!(spawn(&pairs).first().unwrap().broadcast);
    let mut with_origin = pairs.to_vec();
    with_origin.push(("origin", "1 0 0"));
    assert!(!spawn(&with_origin)[0].broadcast);
}

#[test]
fn a_placed_face_moves_its_vertices_and_plane() {
    let e = spawn(&[
        ("classname", "func_static"),
        ("model", "*1"),
        ("origin", "100 0 0"),
        ("angle", "90"),
    ]);
    let vert = DrawVert {
        xyz: [1.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
        st: [0.0; 2],
        lightmap: [[0.0; 2]; 4],
        color: [[255; 4]; 4],
    };
    let face = Geometry::Face {
        verts: vec![vert],
        indexes: vec![0],
        normal: [1.0, 0.0, 0.0],
        dist: 1.0,
    };
    let Geometry::Face {
        verts,
        normal,
        dist,
        ..
    } = e[0].placed(&face)
    else {
        panic!("not a face");
    };
    assert!(close(verts[0].xyz, [100.0, 1.0, 0.0]));
    assert!(close(verts[0].normal, [0.0, 1.0, 0.0]));
    assert!(close(normal, [0.0, 1.0, 0.0]));
    // The plane y = 1, which the moved vertex lies on.
    assert!((dist - 1.0).abs() < 1e-4);
}
