use swjkja_bsp::DrawVert;
use swjkja_world::patch::subdivide;
use swjkja_world::stitch::stitch_all;
use swjkja_world::{Grid, color_shift};

fn vert(x: f32, y: f32, z: f32) -> DrawVert {
    DrawVert {
        xyz: [x, y, z],
        st: [x / 64.0, y / 64.0],
        lightmap: [[0.0; 2]; 4],
        normal: [0.0, 0.0, 1.0],
        color: [[255; 4]; 4],
    }
}

/// A 3x3 patch over x, y in [0, 128] with height `z(column, row)`.
fn patch(y0: f32, z: impl Fn(usize, usize) -> f32) -> Vec<DrawVert> {
    let mut v = Vec::new();
    for row in 0..3 {
        for col in 0..3 {
            v.push(vert(col as f32 * 64.0, y0 + row as f32 * 64.0, z(col, row)));
        }
    }
    v
}

fn row_z(g: &Grid, row: usize) -> Vec<f32> {
    (0..g.width).map(|c| g.vert(c, row).xyz[2]).collect()
}

#[test]
fn color_shift_keeps_the_hue_when_it_overflows() {
    assert_eq!(color_shift([200, 100, 50, 7], 0), [200, 100, 50, 7]);
    assert_eq!(color_shift([100, 50, 25, 7], 1), [200, 100, 50, 7]);
    // 400, 200, 100 is scaled down by 255/400 rather than clipped at 255.
    assert_eq!(color_shift([200, 100, 50, 7], 1), [255, 127, 63, 7]);
}

#[test]
fn a_flat_patch_collapses_to_its_corners() {
    let g = subdivide(3, 3, &patch(0.0, |_, _| 0.0), 4.0);
    assert_eq!((g.width, g.height), (2, 2));
    assert_eq!(g.vert(1, 1).xyz, [128.0, 128.0, 0.0]);
    assert!(g.verts.iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
    assert_eq!(g.width_lod_error, [0.0, 0.0]);
}

#[test]
fn a_curved_patch_subdivides_until_within_tolerance() {
    // Middle column raised by 32: the curve's midpoint is 16 units off the chord, so
    // one split (to 5 columns) leaves halves 4 units off, which r_subdivisions 4 allows.
    let points = patch(0.0, |c, _| if c == 1 { 32.0 } else { 0.0 });
    let g = subdivide(3, 3, &points, 4.0);
    assert_eq!((g.width, g.height), (5, 2));
    // On the quadratic curve: z(t) = 2t(1-t) * 32 at t = 0, 1/4, 1/2, 3/4, 1.
    assert_eq!(row_z(&g, 0), [0.0, 12.0, 16.0, 12.0, 0.0]);
    // Errors are 1 / distance from the chord. The first split's was 16 (1/16 at the
    // middle). Each half is then measured on its control points (0,0,0), (32,0,16),
    // (64,0,16): their (a + 2b + c) / 4 point is 3.8806 from the chord (worked out by
    // hand), so 1/3.8806 on either side.
    let e = &g.width_lod_error;
    assert_eq!((e[0], e[2], e[4]), (0.0, 0.0625, 0.0));
    assert!((e[1] - 1.0 / 3.8806).abs() < 1e-4 && e[3] == e[1], "{e:?}");
    // A looser tolerance keeps the 3 columns.
    assert_eq!(subdivide(3, 3, &points, 16.0).width, 3);
}

#[test]
fn a_tall_patch_is_turned_so_its_long_side_is_the_width() {
    // Curved along the rows instead: the grid ends up 5 wide, 2 high.
    let g = subdivide(
        3,
        3,
        &patch(0.0, |_, r| if r == 1 { 32.0 } else { 0.0 }),
        4.0,
    );
    assert_eq!((g.width, g.height), (5, 2));
}

#[test]
fn stitching_inserts_the_vertices_a_coarser_neighbour_lacks() {
    let curve = |c: usize, _| if c == 1 { 32.0 } else { 0.0 };
    // The fine patch spans y 0..128; the coarse one y 128..256, sharing the y = 128 edge.
    let mut fine = subdivide(3, 3, &patch(0.0, curve), 4.0);
    let mut coarse = subdivide(3, 3, &patch(128.0, curve), 16.0);
    assert_eq!((fine.width, coarse.width), (5, 3));
    for g in [&mut fine, &mut coarse] {
        g.lod_origin = [64.0, 128.0, 0.0];
        g.lod_radius = 200.0;
    }
    let mut grids = vec![Some(fine), None, Some(coarse)];
    assert_eq!(stitch_all(&mut grids), 2);
    let coarse = grids[2].as_ref().unwrap();
    assert_eq!(coarse.width, 5);
    // The shared edge now has the fine patch's vertices.
    assert_eq!(row_z(coarse, 0), [0.0, 12.0, 16.0, 12.0, 0.0]);
    assert_eq!(grids[0].as_ref().unwrap().width, 5);
}

#[test]
fn patches_of_different_groups_are_not_stitched() {
    let curve = |c: usize, _| if c == 1 { 32.0 } else { 0.0 };
    let fine = subdivide(3, 3, &patch(0.0, curve), 4.0);
    let mut coarse = subdivide(3, 3, &patch(128.0, curve), 16.0);
    coarse.lod_radius += 1.0;
    let mut grids = vec![Some(fine), Some(coarse)];
    assert_eq!(stitch_all(&mut grids), 0);
}
