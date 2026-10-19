use swjkja_bsp::{DrawVert, Entity, Model};
use swjkja_world::Geometry;
use swjkja_world::brush_entity::{BrushEntity, brush_entities, touch_doors, use_doors};
use swjkja_world::script::Use;
use swjkja_world::static_model::Rules;

const SP: Rules = Rules::SinglePlayer { skill: 0 };
const MP: Rules = Rules::Multiplayer { gametype: 0 };

fn entity(pairs: &[(&str, &str)]) -> Entity {
    Entity {
        pairs: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

/// The world, then model 1: a 64 x 32 x 128 box around the origin, which the
/// collision model spreads a unit each way (66 x 34 x 130).
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
    // The spread bounds, an epsilon larger (SV_LinkEntity).
    assert_eq!(e[0].entity_box, [[66.0, -18.0, -2.0], [134.0, 18.0, 130.0]]);
    assert!(!e[0].broadcast && !e[0].shader_anim);
}

#[test]
fn a_door_that_starts_open_stands_moved_by_its_size_less_the_lip() {
    // Along +y (yaw 90): 34 units of size, less the default lip of 8.
    let e = spawn(&[
        ("classname", "func_door"),
        ("model", "*1"),
        ("angle", "90"),
        ("spawnflags", "1"),
    ]);
    assert!(close(e[0].origin, [0.0, 26.0, 0.0]));
    // Straight up (yaw -1), with a lip of 28: 130 - 28.
    let e = spawn(&[
        ("classname", "func_door"),
        ("model", "*1"),
        ("angle", "-1"),
        ("lip", "28"),
        ("spawnflags", "1"),
    ]);
    assert_eq!(e[0].origin, [0.0, 0.0, 102.0]);
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
    assert_eq!(e[0].origin, [0.0, 0.0, 500.0 - 122.0]);
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
    // RadiusFromBounds: the farthest corner of the spread bounds, (33, 17, 129).
    let r = (33.0f32 * 33.0 + 17.0 * 17.0 + 129.0 * 129.0).sqrt() + 1.0;
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

#[test]
fn a_secondary_model_stands_at_the_entity_turned_by_its_model_angles() {
    let e = spawn(&[
        ("classname", "func_static"),
        ("model", "*1"),
        ("model2", "models/map_objects/korriban/beam_statue.md3"),
        ("origin", "10 20 30"),
        ("angle", "90"),
        ("modelAngles", "0 90 0"),
    ]);
    let m = e[0].model2.as_ref().unwrap();
    assert_eq!(m.model, "models/map_objects/korriban/beam_statue.md3");
    assert_eq!(m.origin, [10.0, 20.0, 30.0]);
    // The entity's yaw of 90 and the model's 90 more.
    assert!(close(m.axes[0], [-1.0, 0.0, 0.0]));
    assert!(close(m.axes[1], [0.0, -1.0, 0.0]));
    assert_eq!(m.entity_box, Some(e[0].entity_box));
    assert!(m.normalized && m.ghoul2.is_none());
    // Without model angles, the entity's own (a door's: none).
    let e = spawn(&[
        ("classname", "func_door"),
        ("model", "*1"),
        ("model2", "models/map_objects/rocky_ruins/portcullis.md3"),
        ("angle", "90"),
    ]);
    let m = e[0].model2.as_ref().unwrap();
    assert_eq!(m.axes, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
}

#[test]
fn a_usable_started_off_draws_neither_brushes_nor_model() {
    let e = spawn(&[
        ("classname", "func_usable"),
        ("model", "*1"),
        ("model2", "models/map_objects/factory/bomb_new.md3"),
        ("spawnflags", "1"),
    ]);
    assert!(e.is_empty());
}

#[test]
fn a_ghoul2_secondary_model_is_drawn_in_single_player_only() {
    let pairs = [
        ("classname", "func_usable"),
        ("model", "*1"),
        ("model2", "models/players/a/model.glm"),
    ];
    let e = spawn(&pairs);
    let g = e[0].model2.as_ref().unwrap().ghoul2.as_ref().unwrap();
    assert_eq!(g.radius, 120.0);
    assert!(g.skin.is_none() && g.pose.is_none());
    let e = brush_entities(&[entity(&pairs)], &models(), MP);
    assert!(e[0].model2.is_none());
}

#[test]
fn multiplayer_scales_a_static_secondary_model_and_has_no_model_angles() {
    let e = brush_entities(
        &[entity(&[
            ("classname", "func_static"),
            ("model", "*1"),
            ("model2", "models/map_objects/bespin/twinpodcc.md3"),
            ("model2scale", "50"),
            ("modelAngles", "0 90 0"),
        ])],
        &models(),
        MP,
    );
    let m = e[0].model2.as_ref().unwrap();
    assert_eq!(m.scale, [0.5; 3]);
    assert_eq!(m.axes, [[0.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 0.5]]);
    assert!(!m.normalized);
    // Only a func_static or func_rotating takes "model2scale"; a negative one is none.
    for (class, scale) in [("func_usable", "50"), ("func_static", "-5")] {
        let e = brush_entities(
            &[entity(&[
                ("classname", class),
                ("model", "*1"),
                ("model2", "models/map_objects/imperial/dish.md3"),
                ("model2scale", scale),
            ])],
            &models(),
            MP,
        );
        let m = e[0].model2.as_ref().unwrap();
        assert_eq!(m.scale, [1.0; 3]);
        assert!(m.normalized);
    }
}

#[test]
fn a_rotating_one_that_starts_on_turns_by_its_speed_from_zero() {
    let rotating = |flags: &str, speed: &str| {
        spawn(&[
            ("classname", "func_rotating"),
            ("model", "*1"),
            ("origin", "10 20 30"),
            ("spawnflags", flags),
            ("speed", speed),
        ])
    };
    // t3_hevil's fans: start on (1), yaw at "speed" degrees a second.
    let e = rotating("1", "300");
    assert_eq!(e[0].spin, Some([0.0, 300.0, 0.0]));
    assert_eq!(e[0].angles_at(0), Some([0.0; 3]));
    assert!(close(e[0].angles_at(1500).unwrap(), [0.0, 450.0, 0.0]));
    assert_eq!(e[0].origin, [10.0, 20.0, 30.0]);
    // Turning, it links a cube of its radius.
    let r = (33.0f32 * 33.0 + 17.0 * 17.0 + 129.0 * 129.0).sqrt() + 1.0;
    assert!(close(e[0].entity_box[1], [10.0 + r, 20.0 + r, 30.0 + r]));
    // Roll (4) and pitch (8); speed 0 is 100.
    assert_eq!(rotating("5", "0")[0].spin, Some([0.0, 0.0, 100.0]));
    assert_eq!(rotating("9", "350")[0].spin, Some([350.0, 0.0, 0.0]));
    // Single player: not started on, it stands still at zero.
    let e = rotating("0", "300");
    assert_eq!((e[0].spin, e[0].angles_at(1500)), (None, None));
    assert_eq!(e[0].entity_box[1], [44.0, 38.0, 160.0]);
}

#[test]
fn multiplayer_turns_every_rotating_one_by_its_spin_angles() {
    let mp = |pairs: &[(&str, &str)]| brush_entities(&[entity(pairs)], &models(), MP);
    let e = mp(&[("classname", "func_rotating"), ("model", "*1")]);
    assert_eq!(e[0].spin, Some([0.0, 100.0, 0.0]));
    let e = mp(&[
        ("classname", "func_rotating"),
        ("model", "*1"),
        ("spinangles", "10 0 -20"),
        ("speed", "300"),
    ]);
    assert_eq!(e[0].spin, Some([10.0, 0.0, -20.0]));
}

/// A body (an NPC's box) of 32 x 32 x 64 standing at `at`.
fn body(at: [f32; 3]) -> [[f32; 3]; 2] {
    [
        [at[0] - 16.0, at[1] - 16.0, at[2] - 24.0],
        [at[0] + 16.0, at[1] + 16.0, at[2] + 40.0],
    ]
}

fn door(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut pairs = vec![("classname", "func_door"), ("model", "*1"), ("angle", "0")];
    pairs.extend_from_slice(extra);
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn spawn_doors(doors: &[Vec<(String, String)>]) -> Vec<BrushEntity> {
    let entities: Vec<Entity> = doors.iter().map(|p| Entity { pairs: p.clone() }).collect();
    brush_entities(&entities, &models(), SP)
}

#[test]
fn a_door_opens_by_its_size_less_the_lip_at_its_speed() {
    // Along +x: 66 units of size less the lip of 8, at the default 400 a second.
    let e = spawn_doors(&[door(&[])]);
    let d = e[0].door.as_ref().unwrap();
    assert_eq!(d.pos2, [58.0, 0.0, 0.0]);
    assert_eq!(
        (d.duration, e[0].linear, d.delay, d.touched),
        (145, false, 0, true)
    );
    // 58 units at 30 a second: 1933.33 ms, whole ms.
    let e = spawn_doors(&[door(&[("speed", "30"), ("linear", "1"), ("delay", "2")])]);
    let d = e[0].door.as_ref().unwrap();
    assert_eq!((d.duration, e[0].linear, d.delay), (1933, true, 2000));
    // One that starts open opens back to where it was placed.
    let e = spawn_doors(&[door(&[("spawnflags", "1")])]);
    assert_eq!(e[0].origin, [58.0, 0.0, 0.0]);
    assert_eq!(e[0].door.as_ref().unwrap().pos2, [0.0; 3]);
    // Multiplayer doors are not modelled.
    let entity = Entity { pairs: door(&[]) };
    assert_eq!(brush_entities(&[entity], &models(), MP)[0].door, None);
}

#[test]
fn only_doors_without_a_name_health_or_blocking_flag_open_by_touch() {
    let touched = |extra: &[(&str, &str)]| {
        spawn_doors(&[door(extra)])[0]
            .door
            .as_ref()
            .unwrap()
            .touched
    };
    assert!(touched(&[]));
    assert!(touched(&[("spawnflags", "1")]));
    assert!(touched(&[("spawnflags", "4")]));
    assert!(!touched(&[("targetname", "d")]));
    assert!(!touched(&[("health", "10")]));
    for flags in ["2", "8", "16", "32", "64", "128"] {
        assert!(!touched(&[("spawnflags", flags)]), "{flags}");
    }
}

#[test]
fn a_body_in_its_trigger_opens_a_door_from_its_first_think() {
    // The model's spread box is 66 x 34 x 130 (linked an epsilon larger); its
    // thinnest axis, y, reaches 120 further: the trigger is y -138..138.
    let mut e = spawn_doors(&[door(&[])]);
    touch_doors(&mut e, &[body([0.0, 155.0, 30.0])], 1550);
    assert_eq!(e[0].slides, []);
    assert_eq!(e[0].origin_at(5000), [0.0; 3]);
    // A body's edge on the trigger's touches it.
    touch_doors(&mut e, &[body([0.0, 154.0, 30.0])], 1550);
    let s = &e[0].slides[0];
    assert_eq!(
        (s.start, s.duration, s.from, s.to),
        (1600, 145, [0.0; 3], [58.0, 0.0, 0.0])
    );
    // It is linked where it is.
    assert_eq!(
        e[0].entity_box_at(1600),
        [[-34.0, -18.0, -2.0], [34.0, 18.0, 130.0]]
    );
    assert_eq!(
        e[0].entity_box_at(1745),
        [[24.0, -18.0, -2.0], [92.0, 18.0, 130.0]]
    );
    // It slows to a stop: sin(90 degrees x elapsed / duration) of the way.
    assert_eq!(e[0].origin_at(1600), [0.0; 3]);
    let at_58 = e[0].origin_at(1658)[0];
    let expected = 58.0 * (std::f32::consts::FRAC_PI_2 * 58.0 / 145.0).sin();
    assert!((at_58 - expected).abs() < 1e-3, "{at_58}");
    assert_eq!(e[0].origin_at(1745), [58.0, 0.0, 0.0]);
    assert_eq!(e[0].origin_at(12345), [58.0, 0.0, 0.0]);
    // "linear": at one speed, after its delay.
    let mut e = spawn_doors(&[door(&[("linear", "1"), ("delay", "1")])]);
    touch_doors(&mut e, &[body([0.0; 3])], 1550);
    assert_eq!(e[0].slides[0].start, 2600);
    assert!((e[0].origin_at(2635)[0] - 14.0).abs() < 1e-4);
    // Not touchable: it stays.
    let mut e = spawn_doors(&[door(&[("targetname", "d")])]);
    touch_doors(&mut e, &[body([0.0; 3])], 1550);
    assert_eq!(e[0].slides, []);
}

#[test]
fn a_team_opens_together_by_its_first_doors_trigger() {
    // Two doors 200 apart along y; the trigger is around both, and the first
    // door's flags decide.
    let mut pairs = [
        door(&[("team", "t")]),
        door(&[("team", "t"), ("origin", "0 200 0")]),
    ];
    let mut e = spawn_doors(&pairs);
    touch_doors(&mut e, &[body([0.0, 100.0, 30.0])], 1550);
    assert!(!e[0].slides.is_empty() && !e[1].slides.is_empty());
    assert_eq!(e[1].slides[0].to, [58.0, 200.0, 0.0]);
    // Across the team's box, the thinnest axis is x (68 < 236): it reaches 120
    // further along x, not y.
    let mut e = spawn_doors(&pairs);
    touch_doors(&mut e, &[body([150.0, 100.0, 30.0])], 1550);
    assert!(!e[0].slides.is_empty());
    let mut e = spawn_doors(&pairs);
    touch_doors(&mut e, &[body([0.0, 360.0, 30.0])], 1550);
    assert!(e[0].slides.is_empty() && e[1].slides.is_empty());
    // A named leader opens by being used only, its team too.
    pairs[0].push(("targetname".into(), "d".into()));
    let mut e = spawn_doors(&pairs);
    touch_doors(&mut e, &[body([0.0, 200.0, 30.0])], 1550);
    assert!(e[0].slides.is_empty() && e[1].slides.is_empty());
}

fn uses(list: &[(&str, i32)]) -> Vec<Use> {
    list.iter()
        .map(|&(name, at)| Use {
            name: name.to_owned(),
            at,
        })
        .collect()
}

/// Each slide's start, whether it opens, and its time.
fn moves(e: &BrushEntity) -> Vec<(i32, bool, i32)> {
    e.slides
        .iter()
        .map(|s| (s.start, s.to != e.origin, s.duration))
        .collect()
}

#[test]
fn a_used_door_opens_waits_two_seconds_and_closes() {
    let door_d = door(&[("targetname", "d")]);
    let d = spawn_doors(std::slice::from_ref(&door_d))[0].door.clone().unwrap();
    assert_eq!(
        (d.wait, d.toggle, d.targetname.as_deref()),
        (2000, false, Some("d"))
    );
    // Used in the frame at 3000: it starts 50 ms later and takes 145; it reaches
    // the end on the frame at 3200, closes on the frame at 5200 and is shut at 5345.
    let mut e = spawn_doors(std::slice::from_ref(&door_d));
    use_doors(&mut e, &uses(&[("D", 3000), ("other", 3000)]));
    assert_eq!(moves(&e[0]), [(3050, true, 145), (5200, false, 145)]);
    assert_eq!(e[0].origin_at(5199), [58.0, 0.0, 0.0]);
    let at_5258 = e[0].origin_at(5258)[0];
    let expected = 58.0 - 58.0 * (std::f32::consts::FRAC_PI_2 * 58.0 / 145.0).sin();
    assert!((at_5258 - expected).abs() < 1e-3, "{at_5258}");
    assert_eq!(e[0].origin_at(5345), [0.0; 3]);
    // A use while it stands open puts its closing off; one while it moves is lost;
    // one once it is shut opens it again.
    let mut e = spawn_doors(std::slice::from_ref(&door_d));
    use_doors(
        &mut e,
        &uses(&[
            ("d", 3000),
            ("d", 3100),
            ("d", 4000),
            ("d", 6150),
            ("d", 6350),
        ]),
    );
    assert_eq!(
        moves(&e[0]),
        [
            (3050, true, 145),
            (6000, false, 145),
            (6400, true, 145),
            (8550, false, 145)
        ]
    );
    // Nothing used: it stays.
    let mut e = spawn_doors(&[door_d]);
    use_doors(&mut e, &uses(&[("e", 3000)]));
    assert_eq!(e[0].slides, []);
}

#[test]
fn a_used_doors_wait_delay_and_toggle() {
    let used = |extra: &[(&str, &str)], list: &[(&str, i32)]| {
        let mut pairs = vec![("targetname", "d")];
        pairs.extend_from_slice(extra);
        let mut e = spawn_doors(&[door(&pairs)]);
        use_doors(&mut e, &uses(list));
        moves(&e[0])
    };
    // "wait" 5: closes on the frame 5 s after it reaches the end.
    assert_eq!(
        used(&[("wait", "5")], &[("d", 3000)]),
        [(3050, true, 145), (8200, false, 145)]
    );
    // "wait" -1: open for good, used no more.
    assert_eq!(
        used(&[("wait", "-1")], &[("d", 3000), ("d", 9000)]),
        [(3050, true, 145)]
    );
    // "delay" 1: it goes on its think a second later.
    assert_eq!(
        used(&[("delay", "1")], &[("d", 3000)]),
        [(4050, true, 145), (6200, false, 145)]
    );
    // A toggled door stays open until used, then closes 100 ms later.
    assert_eq!(
        used(&[("spawnflags", "8")], &[("d", 3000)]),
        [(3050, true, 145)]
    );
    assert_eq!(
        used(&[("spawnflags", "8")], &[("d", 3000), ("d", 7000)]),
        [(3050, true, 145), (7100, false, 145)]
    );
    // Locked (a use unlocks it), opened by a key, or inactive: a use opens none.
    for flags in ["16", "32", "128"] {
        assert_eq!(
            used(&[("spawnflags", flags)], &[("d", 3000)]),
            [],
            "{flags}"
        );
    }
}

#[test]
fn a_use_of_any_door_of_a_team_moves_the_team_on_its_first_doors_time() {
    // The second door is slower (60 a second: 966 ms, in whole ms); the first reaches its end on
    // the frame at 3200 and the team closes on the frame at 5200.
    let pairs = [
        door(&[("team", "t")]),
        door(&[("team", "t"), ("targetname", "b"), ("speed", "60")]),
    ];
    let mut e = spawn_doors(&pairs);
    use_doors(&mut e, &uses(&[("b", 3000)]));
    assert_eq!(moves(&e[0]), [(3050, true, 145), (5200, false, 145)]);
    assert_eq!(moves(&e[1]), [(3050, true, 966), (5200, false, 966)]);
    // The slow door closes from its open end, wherever it had got to.
    assert_eq!(e[1].slides[1].from, [58.0, 0.0, 0.0]);
}
