//! Sky geometry, as OpenJK's `rd-vanilla/tr_sky.cpp` builds it: a box around the
//! viewer, 6 sides of 9 x 9 points, textured with the sky's six images; and, for
//! skies with a cloud height, the same box's upper 5 sides with texture coordinates
//! that bend the cloud layers into a dome (`R_InitSkyTexCoords`).
//!
//! Positions are offsets from the viewer; the renderer adds the view origin and puts
//! them at the far plane.

const SUBDIVISIONS: usize = 8;
const HALF: i32 = (SUBDIVISIONS / 2) as i32;
/// The image suffixes for the six sides, in side order.
pub const SIDE_SUFFIXES: [&str; 6] = ["rt", "lf", "bk", "ft", "up", "dn"];
/// The box size the original uses when it computes cloud coordinates: zFar 1024
/// divided by 1.75 ("div sqrt(3)").
const CLOUD_BOX: f32 = 1024.0 / 1.75;
/// The box size sky geometry is drawn at (its depth is forced to the far plane, so
/// the size only matters for texture-coordinate generators).
pub const BOX_SIZE: f32 = CLOUD_BOX;

/// A sky vertex: offset from the viewer and texture coordinates.
pub type SkyVertex = ([f32; 3], [f32; 2]);

/// `MakeSkyVec`: the point at (s, t) in [-1, 1] on side `axis`, and its texture
/// coordinates (clamped to [min, max], t flipped).
pub fn sky_vec(s: f32, t: f32, axis: usize, box_size: f32, clamp: (f32, f32)) -> SkyVertex {
    const ST_TO_VEC: [[i32; 3]; 6] = [
        [3, -1, 2],
        [-3, 1, 2],
        [1, 3, 2],
        [-1, -3, 2],
        [-2, -1, 3],
        [2, -1, -3],
    ];
    let b = [s * box_size, t * box_size, box_size];
    let xyz = ST_TO_VEC[axis].map(|k| {
        if k < 0 {
            -b[(-k - 1) as usize]
        } else {
            b[(k - 1) as usize]
        }
    });
    let tc = |v: f32| ((v + 1.0) * 0.5).clamp(clamp.0, clamp.1);
    (xyz, [tc(s), 1.0 - tc(t)])
}

fn grid_coord(i: usize) -> f32 {
    (i as i32 - HALF) as f32 / HALF as f32
}

/// One side of the outer box (`DrawSkyBox` with the whole side visible): 81 vertices
/// and the triangles of `DrawSkySide`'s strips.
pub fn box_side(axis: usize) -> (Vec<SkyVertex>, Vec<u32>) {
    let n = SUBDIVISIONS + 1;
    let mut verts = Vec::with_capacity(n * n);
    for t in 0..n {
        for s in 0..n {
            verts.push(sky_vec(
                grid_coord(s),
                grid_coord(t),
                axis,
                BOX_SIZE,
                (0.0, 1.0),
            ));
        }
    }
    let mut indexes = Vec::new();
    for t in 0..SUBDIVISIONS {
        // A strip along s: (t, s), (t + 1, s) for each s.
        let strip: Vec<u32> = (0..n)
            .flat_map(|s| [(t * n + s) as u32, ((t + 1) * n + s) as u32])
            .collect();
        for w in strip.windows(3) {
            indexes.extend_from_slice(w);
        }
    }
    (verts, indexes)
}

/// `R_InitSkyTexCoords`: the cloud texture coordinates of side `axis`, for points
/// (s, t) on its 9 x 9 grid, for a cloud layer at `cloud_height`.
pub fn cloud_tex_coords(axis: usize, cloud_height: f32) -> Vec<[f32; 2]> {
    let radius = 4096.0f32;
    let h = cloud_height;
    let mut out = Vec::new();
    for t in 0..=SUBDIVISIONS {
        for s in 0..=SUBDIVISIONS {
            let (v, _) = sky_vec(grid_coord(s), grid_coord(t), axis, CLOUD_BOX, (0.0, 1.0));
            let sq = |x: f32| x * x;
            let p = (1.0 / (2.0 * (v[0] * v[0] + v[1] * v[1] + v[2] * v[2])))
                * (-2.0 * v[2] * radius
                    + 2.0
                        * (sq(v[2]) * sq(radius)
                            + 2.0 * sq(v[0]) * radius * h
                            + sq(v[0]) * sq(h)
                            + 2.0 * sq(v[1]) * radius * h
                            + sq(v[1]) * sq(h)
                            + 2.0 * sq(v[2]) * radius * h
                            + sq(v[2]) * sq(h))
                        .sqrt());
            let mut w = [v[0] * p, v[1] * p, v[2] * p + radius];
            let len = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
            w = w.map(|x| x / len);
            out.push([w[0].clamp(-1.0, 1.0).acos(), w[1].clamp(-1.0, 1.0).acos()]);
        }
    }
    out
}

/// One cloud side (`FillCloudBox` with full clouds: sides 0-4, all rows), with the
/// triangles of `FillCloudySkySide`.
pub fn cloud_side(axis: usize, cloud_height: f32) -> (Vec<SkyVertex>, Vec<u32>) {
    let n = SUBDIVISIONS + 1;
    let tcs = cloud_tex_coords(axis, cloud_height);
    let mut verts = Vec::with_capacity(n * n);
    for t in 0..n {
        for s in 0..n {
            let (xyz, _) = sky_vec(grid_coord(s), grid_coord(t), axis, BOX_SIZE, (0.0, 1.0));
            verts.push((xyz, tcs[t * n + s]));
        }
    }
    let w = n as u32;
    let mut indexes = Vec::new();
    for t in 0..w - 1 {
        for s in 0..w - 1 {
            indexes.extend([
                s + t * w,
                s + (t + 1) * w,
                s + 1 + t * w,
                s + (t + 1) * w,
                s + 1 + (t + 1) * w,
                s + 1 + t * w,
            ]);
        }
    }
    (verts, indexes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides_face_the_right_directions() {
        let centre = |axis| sky_vec(0.0, 0.0, axis, 1.0, (0.0, 1.0)).0;
        // rt, lf, bk, ft, up, dn.
        assert_eq!(centre(0), [1.0, 0.0, 0.0]);
        assert_eq!(centre(1), [-1.0, 0.0, 0.0]);
        assert_eq!(centre(2), [0.0, 1.0, 0.0]);
        assert_eq!(centre(3), [0.0, -1.0, 0.0]);
        assert_eq!(centre(4), [0.0, 0.0, 1.0]);
        assert_eq!(centre(5), [0.0, 0.0, -1.0]);
    }

    #[test]
    fn texture_coordinates_span_the_image_with_t_flipped() {
        assert_eq!(sky_vec(-1.0, -1.0, 0, 1.0, (0.0, 1.0)).1, [0.0, 1.0]);
        assert_eq!(sky_vec(1.0, 1.0, 0, 1.0, (0.0, 1.0)).1, [1.0, 0.0]);
        let (verts, indexes) = box_side(4);
        assert_eq!((verts.len(), indexes.len()), (81, 8 * 16 * 3));
    }

    #[test]
    fn clouds_straight_up_are_at_the_dome_top() {
        // Straight up the dome point is (0, 0, 1) after normalising: acos(0) twice.
        let tcs = cloud_tex_coords(4, 512.0);
        let top = tcs[4 * 9 + 4];
        assert!(
            (top[0] - std::f32::consts::FRAC_PI_2).abs() < 1e-6
                && (top[1] - std::f32::consts::FRAC_PI_2).abs() < 1e-6
        );
        let (verts, indexes) = cloud_side(0, 512.0);
        assert_eq!((verts.len(), indexes.len()), (81, 8 * 8 * 6));
    }
}
