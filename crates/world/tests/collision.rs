use swjkja_world::Vec3;
use swjkja_world::collision::{
    CONTENTS_MONSTERCLIP, CONTENTS_SOLID, Collision, MASK_NPCSOLID, SURFACE_CLIP_EPSILON,
};
use swjkja_world::npc::settle;

/// An axis-aligned brush from `mins` to `maxs`, its sides in the compiler's order.
fn block(contents: i32, mins: Vec3, maxs: Vec3) -> (i32, Vec<(Vec3, f32)>) {
    let mut planes = Vec::new();
    for i in 0..3 {
        let mut n = [0.0; 3];
        n[i] = -1.0;
        planes.push((n, -mins[i]));
        n[i] = 1.0;
        planes.push((n, maxs[i]));
    }
    (contents, planes)
}

/// A floor whose top is at height 0, and an NPC's box.
fn floor() -> Collision {
    let mut world = Collision::default();
    world.add_box([-512.0, -512.0, -64.0], [512.0, 512.0, 0.0], CONTENTS_SOLID);
    world
}
const MINS: Vec3 = [-15.0, -15.0, -24.0];
const MAXS: Vec3 = [15.0, 15.0, 40.0];

#[test]
fn a_box_stops_short_of_the_floor_it_falls_onto() {
    let tr = floor().trace(
        [0.0, 0.0, 100.0],
        [0.0, 0.0, -100.0],
        MINS,
        MAXS,
        MASK_NPCSOLID,
    );
    assert_eq!(tr.end, [0.0, 0.0, 24.0 + SURFACE_CLIP_EPSILON]);
    assert!(!tr.start_solid);
    assert_eq!(
        floor().drop_to_floor([10.0, 20.0, 32.0], MINS, MAXS, MASK_NPCSOLID),
        Some([10.0, 20.0, 24.125])
    );
}

#[test]
fn a_trace_that_misses_goes_all_the_way() {
    // Beside the floor, and through brushes the mask does not name.
    let tr = floor().trace(
        [600.0, 0.0, 100.0],
        [600.0, 0.0, -100.0],
        MINS,
        MAXS,
        MASK_NPCSOLID,
    );
    assert_eq!((tr.fraction, tr.end), (1.0, [600.0, 0.0, -100.0]));
    let tr = floor().trace(
        [0.0, 0.0, 100.0],
        [0.0, 0.0, -100.0],
        MINS,
        MAXS,
        CONTENTS_MONSTERCLIP,
    );
    assert_eq!(tr.fraction, 1.0);
    assert_eq!(
        floor().drop_to_floor([600.0, 0.0, 32.0], MINS, MAXS, MASK_NPCSOLID),
        None
    );
}

#[test]
fn a_box_inside_a_brush_starts_solid() {
    let tr = floor().trace(
        [0.0, 0.0, -32.0],
        [0.0, 0.0, -40.0],
        [-1.0; 3],
        [1.0; 3],
        MASK_NPCSOLID,
    );
    assert!(tr.start_solid && tr.all_solid);
    assert_eq!(tr.fraction, 0.0);
    assert_eq!(
        floor().drop_to_floor([0.0, 0.0, -32.0], MINS, MAXS, MASK_NPCSOLID),
        None
    );
}

#[test]
fn a_box_lands_on_a_slope_where_its_corner_meets_it() {
    // A 45-degree ramp rising towards +x: z <= x inside the floor's bounds.
    let mut ramp = block(
        CONTENTS_SOLID,
        [-512.0, -512.0, -512.0],
        [512.0, 512.0, 512.0],
    );
    let s = std::f32::consts::FRAC_1_SQRT_2;
    ramp.1.push(([-s, 0.0, s], 0.0));
    let mut world = Collision::default();
    world.add_brushes([ramp]);
    // The box's corner at x = 15, the ramp's highest under it, rests on the ramp
    // at z = 15, plus the epsilon measured along the ramp's normal.
    let end = world
        .drop_to_floor([0.0, 0.0, 100.0], MINS, MAXS, MASK_NPCSOLID)
        .unwrap();
    assert!(
        (end[2] - (24.0 + 15.0 + SURFACE_CLIP_EPSILON / s)).abs() < 1e-3,
        "{end:?}"
    );
}

#[test]
fn an_npc_falls_to_the_floor_and_rises_out_of_it() {
    // Spawned 8 units above the floor, or with its feet 8 units into it.
    assert_eq!(
        settle(&floor(), [0.0, 0.0, 32.0], MINS, MAXS),
        [0.0, 0.0, 24.125]
    );
    let risen = settle(&floor(), [0.0, 0.0, 16.0], MINS, MAXS);
    assert!((risen[2] - 24.125).abs() < 1e-3, "{risen:?}");
    // Stuck in solid, or over nothing: where it spawned.
    assert_eq!(
        settle(&floor(), [0.0, 0.0, -32.0], MINS, MAXS),
        [0.0, 0.0, -32.0]
    );
    assert_eq!(
        settle(&floor(), [600.0, 0.0, 32.0], MINS, MAXS),
        [600.0, 0.0, 32.0]
    );
}

#[test]
fn an_npc_lands_on_a_solid_box_on_the_floor() {
    let mut world = floor();
    world.add_box([-32.0, -32.0, 0.0], [32.0, 32.0, 48.0], CONTENTS_SOLID);
    assert_eq!(
        settle(&world, [0.0, 0.0, 100.0], MINS, MAXS),
        [0.0, 0.0, 72.125]
    );
}
