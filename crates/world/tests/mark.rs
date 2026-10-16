use swjkja_bsp::DrawVert;
use swjkja_world::collision::{CONTENTS_SOLID, Collision};
use swjkja_world::mark::{MarkSurfaces, impact_mark, shadow_floor};
use swjkja_world::{Geometry, Surface};

fn vert(xyz: [f32; 3]) -> DrawVert {
    DrawVert {
        xyz,
        st: [0.0; 2],
        lightmap: [[0.0; 2]; 4],
        normal: [0.0, 0.0, 1.0],
        color: [[255; 4]; 4],
    }
}

fn surface(geometry: Geometry) -> Surface {
    Surface {
        shader: 0,
        fog: 0,
        lightmaps: [0; 4],
        lightmap_styles: [0; 4],
        vertex_styles: [0; 4],
        default_shader: false,
        geometry,
    }
}

/// A 200-unit square at height 0 facing `normal`'s way, as two triangles.
fn floor_face(normal: [f32; 3]) -> Surface {
    let corners = [
        [-100.0, -100.0],
        [100.0, -100.0],
        [100.0, 100.0],
        [-100.0, 100.0],
    ];
    surface(Geometry::Face {
        verts: corners.iter().map(|c| vert([c[0], c[1], 0.0])).collect(),
        indexes: vec![0, 1, 2, 0, 2, 3],
        normal,
        dist: 0.0,
    })
}

fn soup(indexes: Vec<u32>) -> Surface {
    let corners = [[-100.0, -100.0], [100.0, -100.0], [100.0, 100.0]];
    surface(Geometry::Triangles {
        verts: corners.iter().map(|c| vert([c[0], c[1], 0.0])).collect(),
        indexes,
        bounds: [[-100.0, -100.0, 0.0], [100.0, 100.0, 0.0]],
    })
}

fn area(points: &[[f32; 3]]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        .abs()
        / 2.0
}

const UP: [f32; 3] = [0.0, 0.0, 1.0];

#[test]
fn a_mark_covers_its_square_of_a_floor() {
    let floor = floor_face(UP);
    let marks = MarkSurfaces::new([(&floor, (0, CONTENTS_SOLID))]);
    let pieces = impact_mark(&marks, [0.0; 3], UP, 0.0, 0.5, 16.0);
    // One piece per triangle of the face, the square cut along its diagonal.
    assert_eq!(pieces.len(), 2);
    let xyz: Vec<Vec<[f32; 3]>> = pieces
        .iter()
        .map(|p| p.iter().map(|v| v.xyz).collect())
        .collect();
    assert_eq!(xyz.iter().map(|p| area(p)).sum::<f32>(), 32.0 * 32.0);
    for v in pieces.iter().flatten() {
        assert_eq!(v.alpha, 127);
        assert_eq!(v.xyz[2], 0.0);
        // The square's corners take the texture's corners: s along y, t along x.
        assert_eq!(v.st, [0.5 + v.xyz[1] / 32.0, 0.5 + v.xyz[0] / 32.0]);
        assert!(v.xyz[0].abs() <= 16.0 && v.xyz[1].abs() <= 16.0);
    }
}

#[test]
fn a_mark_turns_with_its_orientation() {
    let floor = floor_face(UP);
    let marks = MarkSurfaces::new([(&floor, (0, 0))]);
    let pieces = impact_mark(&marks, [0.0; 3], UP, 90.0, 1.0, 16.0);
    for v in pieces.iter().flatten() {
        let st = v.st.map(|x| (x * 1e4).round() / 1e4);
        let want = [0.5 - v.xyz[0] / 32.0, 0.5 + v.xyz[1] / 32.0];
        assert_eq!(st, want.map(|x| (x * 1e4).round() / 1e4), "{v:?}");
    }
}

#[test]
fn surfaces_facing_away_or_without_marks_take_none() {
    let ceiling = floor_face([0.0, 0.0, -1.0]);
    let floor = floor_face(UP);
    let none = |s: &Surface, flags| {
        let marks = MarkSurfaces::new([(s, flags)]);
        impact_mark(&marks, [0.0; 3], UP, 0.0, 1.0, 16.0).is_empty()
    };
    assert!(none(&ceiling, (0, 0)));
    assert!(none(&floor, (0x10_0000, 0))); // SURF_NOMARKS
    assert!(none(&floor, (0x8_0000, 0))); // SURF_NOIMPACT
    assert!(none(&floor, (0, 0x8))); // CONTENTS_FOG
    assert!(!none(&floor, (0, 0)));
}

#[test]
fn soup_triangles_take_marks_by_their_winding() {
    // (a - b) x (c - b) must point up, against the projection.
    let facing = soup(vec![2, 1, 0]);
    let away = soup(vec![0, 1, 2]);
    let pieces = |s: &Surface| {
        let marks = MarkSurfaces::new([(s, (0, 0))]);
        impact_mark(&marks, [50.0, -50.0, 0.0], UP, 0.0, 1.0, 16.0).len()
    };
    assert_eq!(pieces(&facing), 1);
    assert_eq!(pieces(&away), 0);
}

#[test]
fn a_mark_far_from_a_surface_takes_none() {
    let floor = floor_face(UP);
    let marks = MarkSurfaces::new([(&floor, (0, 0))]);
    // 32 units over the floor: past the square's 20-unit reach behind it.
    assert!(impact_mark(&marks, [0.0, 0.0, 32.0], UP, 0.0, 1.0, 16.0).is_empty());
}

#[test]
fn a_shadow_fades_with_height_and_ends_out_of_reach() {
    let mut world = Collision::default();
    world.add_box([-512.0, -512.0, -16.0], [512.0, 512.0, 0.0], CONTENTS_SOLID);
    // The box stops 0.125 over the floor: (24 - 0.125) / 128 of the way.
    let (end, normal, alpha) = shadow_floor(&world, [0.0, 0.0, 24.0]).unwrap();
    assert_eq!(end, [0.0, 0.0, 0.125]);
    assert_eq!(normal, UP);
    assert_eq!(alpha, 1.0 - 23.875 / 128.0);
    assert_eq!(shadow_floor(&world, [0.0, 0.0, 129.0]), None);
    assert_eq!(shadow_floor(&world, [0.0, 0.0, -8.0]), None);
}
