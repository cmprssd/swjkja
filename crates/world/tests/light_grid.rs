use swjkja_bsp::GridPoint;
use swjkja_world::light_grid::{EntityLight, LightGrid};
use swjkja_world::style::MAX_LIGHT_STYLES;

const WHITE: [[u8; 4]; MAX_LIGHT_STYLES] = [[255; 4]; MAX_LIGHT_STYLES];

/// A point lit by style 0 only, its light coming straight down the z axis
/// (longitude 0).
fn point(ambient: u8, direct: u8) -> GridPoint {
    GridPoint {
        ambient: [[ambient; 3], [0; 3], [0; 3], [0; 3]],
        direct: [[direct; 3], [0; 3], [0; 3], [0; 3]],
        styles: [0, 0xff, 0xff, 0xff],
        lat_long: [0, 0],
    }
}

/// A 2x1x1 grid of 64-unit cells from the origin: cell 0 uses point 0, cell 1
/// point 1.
fn grid(points: &[GridPoint]) -> LightGrid {
    LightGrid::new(
        [64.0, 64.0, 128.0],
        [[0.0; 3], [64.0, 0.0, 0.0]],
        points,
        &[0, 1],
        0,
    )
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

#[test]
fn samples_blend_between_cells_and_scale_ambient() {
    let g = grid(&[point(100, 50), point(200, 150)]);
    assert!(g.exists());
    let at = |x| g.sample([x, 0.0, 0.0], &WHITE);
    // On a cell: that cell's light, ambient times r_ambientScale 0.6.
    let l = at(0.0);
    assert!(close(l.ambient, [60.0; 3]), "{l:?}");
    assert!(close(l.directed, [50.0; 3]), "{l:?}");
    assert!(close(l.dir, [0.0, 0.0, 1.0]), "{l:?}");
    // A quarter of the way: 3/4 of cell 0 and 1/4 of cell 1.
    let l = at(16.0);
    assert!(close(l.ambient, [75.0; 3]), "{l:?}");
    assert!(close(l.directed, [75.0; 3]), "{l:?}");
    // Past the last cell: clamped to it (and the cell beyond is off the array).
    assert!(close(at(500.0).ambient, [120.0; 3]));
}

#[test]
fn samples_in_walls_are_skipped_and_the_rest_renormalized() {
    let mut wall = point(255, 255);
    wall.styles[0] = 0xff;
    let g = grid(&[point(100, 50), wall]);
    // 3/4 of cell 0 alone, scaled back up to a full sample.
    let l = g.sample([16.0, 0.0, 0.0], &WHITE);
    assert!(close(l.ambient, [60.0; 3]), "{l:?}");
}

#[test]
fn styles_colour_the_light() {
    let mut p = point(100, 100);
    p.styles = [5, 0xff, 0xff, 0xff];
    let g = grid(&[p, p]);
    let mut styles = WHITE;
    styles[5] = [255, 0, 51, 255];
    let l = g.sample([0.0; 3], &styles);
    assert!(close(l.ambient, [60.0, 0.0, 12.0]), "{l:?}");
}

#[test]
fn entities_get_a_minimum_ambient_clamped_to_a_byte() {
    let g = grid(&[point(0, 0), point(250, 80)]);
    let dark = g.entity_light([0.0; 3], &WHITE, [1.0, 0.0, 0.0]);
    // No directed light: no direction either.
    assert_eq!(
        dark,
        EntityLight {
            ambient: [32.0; 3],
            directed: [0.0; 3],
            dir: [0.0; 3],
        }
    );
    let bright = g.entity_light([64.0, 0.0, 0.0], &WHITE, [1.0, 0.0, 0.0]);
    // 250 * 0.6 + 32 = 182; directed is not clamped.
    assert!(close(bright.ambient, [182.0; 3]), "{bright:?}");
    assert!(close(bright.dir, [0.0, 0.0, 1.0]), "{bright:?}");
}

#[test]
fn a_mismatched_array_drops_the_grid() {
    let g = LightGrid::new(
        [64.0, 64.0, 128.0],
        [[0.0; 3], [64.0, 0.0, 0.0]],
        &[point(10, 10)],
        &[0],
        0,
    );
    assert!(!g.exists());
    // tr.identityLight * 150, plus 32; the sun's direction.
    let l = g.entity_light([0.0; 3], &WHITE, [0.0, 1.0, 0.0]);
    assert!(close(l.ambient, [182.0; 3]), "{l:?}");
    assert!(close(l.directed, [150.0; 3]), "{l:?}");
    assert!(close(l.dir, [0.0, 1.0, 0.0]), "{l:?}");
}
