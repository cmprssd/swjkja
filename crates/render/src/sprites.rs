//! Surface sprites (`tr_surfacesprites.cpp`): a `surfaceSprites` stage scatters
//! sprites (grass, mostly) over its surfaces' upward-facing triangles each frame,
//! placed by a fixed chart of random numbers seeded from each triangle's corners,
//! faded out with distance from the view and swaying with time.
//!
//! Vertical and flattened sprites only; oriented and effect ones are not drawn. The
//! wind is the original's without a weather system or wind cvars (no wind, only the
//! idle sway).

use swjkja_shader::{Facing, SpriteKind, SurfaceSprites};

/// `randomchart`.
#[rustfmt::skip]
const RANDOM_CHART: [f32; 256] = [
    0.6554, 0.6909, 0.4806, 0.6218, 0.5717, 0.3896, 0.0677, 0.7356,
    0.8333, 0.1105, 0.4445, 0.8161, 0.4689, 0.0433, 0.7152, 0.0336,
    0.0186, 0.9140, 0.1626, 0.6553, 0.8340, 0.7094, 0.2020, 0.8087,
    0.9119, 0.8009, 0.1339, 0.8492, 0.9173, 0.5003, 0.6012, 0.6117,
    0.5525, 0.5787, 0.1586, 0.3293, 0.9273, 0.7791, 0.8589, 0.4985,
    0.0883, 0.8545, 0.2634, 0.4727, 0.3624, 0.1631, 0.7825, 0.0662,
    0.6704, 0.3510, 0.7525, 0.9486, 0.4685, 0.1535, 0.1545, 0.1121,
    0.4724, 0.8483, 0.3833, 0.1917, 0.8207, 0.3885, 0.9702, 0.9200,
    0.8348, 0.7501, 0.6675, 0.4994, 0.0301, 0.5225, 0.8011, 0.1696,
    0.5351, 0.2752, 0.2962, 0.7550, 0.5762, 0.7303, 0.2835, 0.4717,
    0.1818, 0.2739, 0.6914, 0.7748, 0.7640, 0.8355, 0.7314, 0.5288,
    0.7340, 0.6692, 0.6813, 0.2810, 0.8057, 0.0648, 0.8749, 0.9199,
    0.1462, 0.5237, 0.3014, 0.4994, 0.0278, 0.4268, 0.7238, 0.5107,
    0.1378, 0.7303, 0.7200, 0.3819, 0.2034, 0.7157, 0.5552, 0.4887,
    0.0871, 0.3293, 0.2892, 0.4545, 0.0088, 0.1404, 0.0275, 0.0238,
    0.0515, 0.4494, 0.7206, 0.2893, 0.6060, 0.5785, 0.4182, 0.5528,
    0.9118, 0.8742, 0.3859, 0.6030, 0.3495, 0.4550, 0.9875, 0.6900,
    0.6416, 0.2337, 0.7431, 0.9788, 0.6181, 0.2464, 0.4661, 0.7621,
    0.7020, 0.8203, 0.8869, 0.2145, 0.7724, 0.6093, 0.6692, 0.9686,
    0.5609, 0.0310, 0.2248, 0.2950, 0.2365, 0.1347, 0.2342, 0.1668,
    0.3378, 0.4330, 0.2775, 0.9901, 0.7053, 0.7266, 0.4840, 0.2820,
    0.5733, 0.4555, 0.6049, 0.0770, 0.4760, 0.6060, 0.4159, 0.3427,
    0.1234, 0.7062, 0.8569, 0.1878, 0.9057, 0.9399, 0.8139, 0.1407,
    0.1794, 0.9123, 0.9493, 0.2827, 0.9934, 0.0952, 0.4879, 0.5160,
    0.4118, 0.4873, 0.3642, 0.7470, 0.0866, 0.5172, 0.6365, 0.2676,
    0.2407, 0.7223, 0.5761, 0.1143, 0.7137, 0.2342, 0.3353, 0.6880,
    0.2296, 0.6023, 0.6027, 0.4138, 0.5408, 0.9859, 0.1503, 0.7238,
    0.6054, 0.2477, 0.6804, 0.1432, 0.4540, 0.9776, 0.8762, 0.7607,
    0.9025, 0.9807, 0.0652, 0.8661, 0.7663, 0.2586, 0.3994, 0.0335,
    0.7328, 0.0166, 0.9589, 0.4348, 0.5493, 0.7269, 0.6867, 0.6614,
    0.6800, 0.7804, 0.5591, 0.8381, 0.0910, 0.7573, 0.8985, 0.3083,
    0.3188, 0.8481, 0.2356, 0.6736, 0.4770, 0.4560, 0.6266, 0.4677,];

/// `FADE_RANGE`: the fade's ideal width, in units.
const FADE_RANGE: f32 = 250.0;

/// A vertex of a surface that scatters sprites: where it is, its normal's height,
/// and its light (the blue of its colour, which the sprites take as grey).
#[derive(Debug, Clone, Copy)]
pub struct SpriteVertex {
    pub xyz: [f32; 3],
    pub normal_z: f32,
    pub light: u8,
}

/// The view sprites are scattered for: its origin, its left axis, the game time in
/// milliseconds, and how much further than the shader says its sprites reach
/// ([`range_scale`]).
#[derive(Debug, Clone, Copy)]
pub struct SpriteView {
    pub origin: [f32; 3],
    pub left: [f32; 3],
    pub time_ms: i32,
    pub range_scale: f32,
}

/// `R_SurfaceSpriteFrameUpdate`'s `rangescalefactor` for a view `fov_x` degrees
/// across: sprites seen through a narrower field of view than the standard one
/// look larger, so they reach as much further. The standard is the first field
/// of view sprites are drawn with, between 50 and 135 degrees (`standard`, kept
/// across frames; until then the scale is 1).
pub fn range_scale(standard: &mut Option<f32>, fov_x: f32) -> f32 {
    let tan_half = |fov: f32| (f64::from(fov) * 0.5).to_radians().tan();
    match *standard {
        None => {
            if fov_x > 50.0 && fov_x < 135.0 {
                *standard = Some(fov_x);
            }
            1.0
        }
        Some(s) if s == fov_x => 1.0,
        // standardscalex is a float.
        Some(s) => (f64::from(tan_half(s) as f32) / tan_half(fov_x.min(135.0))) as f32,
    }
}

/// One sprite: its corners (bottom right, top right, top left, bottom left, with
/// texture coordinates (1, 1), (1, 0), (0, 0), (0, 1)) and its colour. The top
/// left leans away from the view, so the quad is not flat: the original draws it
/// as a `GL_QUADS` quad, which the reference GPU's driver splits along the top
/// right to bottom left diagonal ([`QUAD_TRIANGLES`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    pub corners: [[f32; 3]; 4],
    pub color: [u8; 4],
}

/// A sprite's two triangles, by corner: split along the top right to bottom left
/// diagonal, as the reference capture shows (the other diagonal, with the lean,
/// moved dpred_troopers' near grass visibly).
pub const QUAD_TRIANGLES: [u32; 6] = [0, 1, 3, 1, 2, 3];

/// `Q_rsqrt`: the inverse square root, approximated as the original does.
fn q_rsqrt(number: f32) -> f32 {
    let x2 = number * 0.5;
    let y = f32::from_bits(0x5f37_59df - (number.to_bits() >> 1));
    y * (1.5 - (x2 * y * y))
}

/// C's float-to-byte conversion as MSVC does it: to an integer, then its low byte.
fn byte(f: f32) -> u8 {
    f as i32 as u8
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    a.map(|x| x * s)
}

/// `RB_DrawVerticalSurfaceSprites` for one surface's triangles (`indexes` into
/// `verts`), appending its sprites to `out`. `additive` is a stage blended
/// `GL_ONE GL_ONE`, whose sprites fade by darkening.
pub fn scatter(
    ss: &SurfaceSprites,
    additive: bool,
    verts: &[SpriteVertex],
    indexes: &[u32],
    view: &SpriteView,
    out: &mut Vec<Sprite>,
) {
    if !matches!(ss.kind, SpriteKind::Vertical | SpriteKind::Flattened) {
        return;
    }
    // R_SurfaceSpriteFrameUpdate: a horizontal forward vector, and four right
    // vectors turned 10 and 30 degrees either way.
    let left = view.left;
    let forward = [left[1], -left[0], 0.0];
    let nudge = |l: f32, f: f32| add(scale(left, l), scale(forward, f));
    let rights = [
        nudge(0.985, 0.174),
        nudge(0.866, -0.5),
        nudge(0.866, 0.5),
        nudge(0.985, -0.174),
    ];

    let cut = ss.fade_max * view.range_scale;
    let fade = ss.fade_dist * view.range_scale;
    let inv_fade_diff = 1.0 / (cut * cut - fade * fade);
    let fade_range = (FADE_RANGE / (cut - fade)).min(1.0);
    let alpha_of = |v: &SpriteVertex| {
        let d = [0, 1, 2].map(|k| view.origin[k] - v.xyz[k]);
        let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        1.0 - (d2 - fade * fade) * inv_fade_diff
    };
    let hangdown = ss.facing != Facing::Normal;

    for tri in indexes.as_chunks::<3>().0 {
        let [p1, p2, p3] = [0, 1, 2].map(|k| &verts[tri[k] as usize]);
        if !faces(ss, [p1, p2, p3]) {
            continue;
        }
        let [a1, a2, a3] = [p1, p2, p3].map(alpha_of);
        if a1 <= 0.0 && a2 <= 0.0 && a3 <= 0.0 {
            continue;
        }
        let Some(step) = step(ss, [p1, p2, p3]) else {
            continue;
        };
        let (v1, v2, v3) = (p1.xyz, p2.xyz, p3.xyz);

        let mut random = byte(v1[0] + v1[1] + v2[0] + v2[1] + v3[0] + v3[1]);
        // At least 3, and odd.
        let interval = byte(v1[0] + v2[1] + v3[2]) | 3;
        let draw = |r: &mut u8| {
            let x = RANDOM_CHART[*r as usize];
            *r = r.wrapping_add(interval);
            x
        };
        let mut right_index = 0;
        let mut posi = 0.0f32;
        while posi < 1.0 {
            let mut posj = 0.0f32;
            // The bounds are doubles in the original (`1.0 - posi`).
            while f64::from(posj) < 1.0 - f64::from(posi) {
                let fa = posi + draw(&mut random) * step;
                let fb = posj + draw(&mut random) * step;
                posj += step;
                right_index = (right_index + 1) & 3;
                if fa > 1.0 || f64::from(fb) > 1.0 - f64::from(fa) {
                    continue;
                }
                let fc = (1.0 - f64::from(fa) - f64::from(fb)) as f32;
                // The interpolated fade (1 where it starts), against a fade start
                // drawn for each sprite.
                let alpha_pos = a1 * fa + a2 * fb + a3 * fc;
                let fade_start = fade_range + (1.0 - fade_range) * draw(&mut random);
                let mut alpha = 1.0 - ((fade_start - alpha_pos) / fade_range);
                if alpha <= 0.0 {
                    continue;
                }
                alpha = alpha.min(1.0);
                let at = add(add(scale(v1, fa), scale(v2, fb)), scale(v3, fc));
                let mut light =
                    f32::from(p1.light) * fa + f32::from(p2.light) * fb + f32::from(p3.light) * fc;
                if additive {
                    light = (128.0 + light * 0.5) * alpha;
                    alpha = 1.0;
                }
                let r = RANDOM_CHART[random as usize];
                let mut width = ss.width * (1.0 + ss.variance[0] * r);
                let height = ss.height * (1.0 + ss.variance[1] * r);
                let mut more = random.wrapping_add(1);
                if RANDOM_CHART[more as usize] > 0.5 {
                    width = -width;
                }
                more = more.wrapping_add(1);
                if ss.fade_scale != 0.0 && alpha_pos < 1.0 {
                    width *= 1.0 + ss.fade_scale * (1.0 - alpha_pos);
                }
                let mut skew = [0.0; 2];
                if ss.vert_skew != 0.0 {
                    for s in &mut skew {
                        let x = RANDOM_CHART[more as usize];
                        more = more.wrapping_add(1);
                        *s = height * ((ss.vert_skew * 2.0 * x) - ss.vert_skew);
                    }
                }
                let right = if ss.kind == SpriteKind::Flattened {
                    let a = f64::from(at[0]).to_radians();
                    [
                        (a.sin() * f64::from(width)) as f32,
                        (a.cos() * f64::from(height)) as f32,
                        0.0,
                    ]
                } else {
                    scale(rights[right_index], width * 0.5)
                };
                out.push(sprite(
                    at,
                    width,
                    height,
                    [byte(light), byte(alpha * 255.0)],
                    ss.wind_idle,
                    hangdown,
                    skew,
                    right,
                    forward,
                    view.time_ms,
                ));
            }
            posi += step;
        }
    }
}

/// Whether a triangle's corners all face the way the sprites stand: up, or down
/// for any other facing.
fn faces(ss: &SurfaceSprites, corners: [&SpriteVertex; 3]) -> bool {
    corners.iter().all(|v| {
        if ss.facing == Facing::Normal {
            v.normal_z >= 0.5
        } else {
            v.normal_z <= -0.5
        }
    })
}

/// The step between a triangle's sprites, as a fraction of each edge, from its
/// area seen from above; `None` for a sliver.
fn step(ss: &SurfaceSprites, [p1, p2, p3]: [&SpriteVertex; 3]) -> Option<f32> {
    let (v1, v2, v3) = (p1.xyz, p2.xyz, p3.xyz);
    let e12 = [v2[0] - v1[0], v2[1] - v1[1]];
    let e13 = [v3[0] - v1[0], v3[1] - v1[1]];
    let area = (e13[0] * e12[1] - e13[1] * e12[0]).abs();
    (area > 1.0).then(|| ss.density * q_rsqrt(area))
}

/// The most sprites [`scatter`] can make of these triangles, from any view: every
/// place it tries.
pub fn capacity(ss: &SurfaceSprites, verts: &[SpriteVertex], indexes: &[u32]) -> usize {
    let mut n = 0;
    for tri in indexes.as_chunks::<3>().0 {
        let corners = [0, 1, 2].map(|k| &verts[tri[k] as usize]);
        if !faces(ss, corners) {
            continue;
        }
        let Some(step) = step(ss, corners) else {
            continue;
        };
        let mut posi = 0.0f32;
        while posi < 1.0 {
            let mut posj = 0.0f32;
            while f64::from(posj) < 1.0 - f64::from(posi) {
                n += 1;
                posj += step;
            }
            posi += step;
        }
    }
    n
}

/// `RB_VerticalSurfaceSprite` without wind: a quad from `at` up (or down) `height`,
/// its top swaying in a circle with time.
#[allow(clippy::too_many_arguments)]
fn sprite(
    at: [f32; 3],
    width: f32,
    height: f32,
    [light, alpha]: [u8; 2],
    wind_idle: f32,
    hangdown: bool,
    skew: [f32; 2],
    right: [f32; 3],
    forward: [f32; 3],
    time_ms: i32,
) -> Sprite {
    let angle = (f64::from(at[0] + at[1]) * 0.02 + f64::from(time_ms) * 0.0015) as f32;
    let sway = if wind_idle > 0.0 {
        let s = height * wind_idle * 0.075;
        [angle.cos() * s, angle.sin() * s]
    } else {
        [0.0; 2]
    };
    let top = [
        at[0] + skew[0] + sway[0],
        at[1] + skew[1] + sway[1],
        if hangdown {
            at[2] - height
        } else {
            at[2] + height
        },
    ];
    let lean = width * 0.2;
    Sprite {
        corners: [
            add(at, right),
            add(top, right),
            [
                top[0] - right[0] + forward[0] * lean,
                top[1] - right[1] + forward[1] * lean,
                top[2] - right[2],
            ],
            [at[0] - right[0], at[1] - right[1], at[2] - right[2]],
        ],
        color: [light, light, light, alpha],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// dpred's grass (`textures/imperial/dp_ground`).
    const GRASS: SurfaceSprites = SurfaceSprites {
        kind: SpriteKind::Flattened,
        width: 48.0,
        height: 24.0,
        density: 45.0,
        fade_dist: 400.0,
        fade_max: 3000.0,
        fade_scale: 2.5,
        wind: 0.8,
        wind_idle: 0.8,
        variance: [1.0, 2.0],
        facing: Facing::Normal,
        vert_skew: 0.0,
    };

    /// dpred_troopers' view, looking down -x.
    const VIEW: SpriteView = SpriteView {
        origin: [-900.0, -4300.0, 90.0],
        left: [0.0, -1.0, 0.0],
        time_ms: 12345,
        range_scale: 1.0,
    };

    fn triangle(normal_z: f32) -> Vec<SpriteVertex> {
        let v = |xyz, light| SpriteVertex {
            xyz,
            normal_z,
            light,
        };
        vec![
            v([-900.0, -4400.0, 0.0], 200),
            v([-700.0, -4400.0, 0.0], 180),
            v([-900.0, -4200.0, 4.0], 160),
        ]
    }

    #[test]
    fn q_rsqrt_is_the_one_step_approximation() {
        assert_eq!(q_rsqrt(40000.0), 0.004_991_806);
        assert_eq!(q_rsqrt(4.0), 0.499_153_58);
    }

    #[test]
    fn a_triangle_scatters_the_sprites_its_corners_seed() {
        let mut out = Vec::new();
        scatter(&GRASS, false, &triangle(1.0), &[0, 1, 2], &VIEW, &mut out);
        // From a separate port of RB_DrawVerticalSurfaceSprites: 9 sprites, the
        // first three standing at these points with these lights.
        assert_eq!(out.len(), 9);
        // Of 15 places tried (a step of 0.2246: 5 + 4 + 3 + 2 + 1).
        assert_eq!(capacity(&GRASS, &triangle(1.0), &[0, 1, 2]), 15);
        let expected = [
            ([-887.9777, -4212.4175, 3.751648], 161),
            ([-836.5731, -4295.581, 2.088388], 172),
            ([-769.1614, -4363.3564, 0.732873], 179),
        ];
        for (s, (at, light)) in out.iter().zip(expected) {
            let base = [0, 1, 2].map(|k| (s.corners[0][k] + s.corners[3][k]) / 2.0);
            for k in 0..3 {
                assert!((base[k] - at[k]).abs() < 1e-3, "{base:?} != {at:?}");
            }
            assert_eq!(s.color, [light, light, light, 255]);
        }
    }

    #[test]
    fn a_narrower_view_than_the_first_reaches_further() {
        // A 90-degree camera drew sprites first: an 80-degree view reaches
        // tan 45 / tan 40 further.
        let mut standard = None;
        assert_eq!(range_scale(&mut standard, 90.0), 1.0);
        assert_eq!(standard, Some(90.0));
        assert_eq!(range_scale(&mut standard, 80.0), 1.191_753_6);
        assert_eq!(range_scale(&mut standard, 90.0), 1.0);
        // Beyond 135 degrees, as at 135.
        assert_eq!(range_scale(&mut standard, 170.0), 0.414_213_57);
        // A first view outside 50-135 sets no standard.
        let mut standard = None;
        assert_eq!(range_scale(&mut standard, 30.0), 1.0);
        assert_eq!(standard, None);
        // Sprites past the shader's 3000 units show in range.
        let mut out = Vec::new();
        let far = SpriteView {
            origin: [-900.0 + 3200.0, -4300.0, 90.0],
            ..VIEW
        };
        scatter(&GRASS, false, &triangle(1.0), &[0, 1, 2], &far, &mut out);
        assert!(out.is_empty());
        let further = SpriteView {
            range_scale: 1.191_753_6,
            ..far
        };
        scatter(
            &GRASS,
            false,
            &triangle(1.0),
            &[0, 1, 2],
            &further,
            &mut out,
        );
        assert!(!out.is_empty());
    }

    #[test]
    fn sprites_need_upward_faces_within_range() {
        let mut out = Vec::new();
        scatter(&GRASS, false, &triangle(0.4), &[0, 1, 2], &VIEW, &mut out);
        assert!(out.is_empty());
        let far = SpriteView {
            origin: [5000.0, -4300.0, 90.0],
            ..VIEW
        };
        scatter(&GRASS, false, &triangle(1.0), &[0, 1, 2], &far, &mut out);
        assert!(out.is_empty());
        // Hanging sprites need downward faces.
        let hanging = SurfaceSprites {
            facing: Facing::Down,
            ..GRASS
        };
        scatter(
            &hanging,
            false,
            &triangle(-1.0),
            &[0, 1, 2],
            &VIEW,
            &mut out,
        );
        assert_eq!(out.len(), 9);
        assert!(out[0].corners[1][2] < out[0].corners[0][2]);
    }
}
