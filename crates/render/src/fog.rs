//! Fog volumes as the original draws them (OpenJK `rd-vanilla`): `R_LoadFogs` (the
//! fog's colour, thickness and surface plane), `R_FogFactor` and `R_CreateFogImage`
//! (the 256 x 32 fog image: S is distance, T is depth below the fog surface), and
//! `RB_CalcFogTexCoords` (each vertex's place in that image). The vertex shader
//! repeats `fog_tex_coords` and `fog_factor`; these are the reference for its tests.

use swjkja_bsp::Bsp;

pub type Vec3 = [f32; 3];

/// One of the map's fogs, as the renderer uses it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogVolume {
    /// `colorInt`: the fog colour in bytes, times tr.identityLight (1 here).
    pub color: [u8; 4],
    /// `tcScale`: 1 / (8 * depthForOpaque), with depthForOpaque at least 1.
    pub tc_scale: f32,
    /// The fog surface plane (normal pointing into the fog, and its distance, as
    /// `surface[3]`); all zero for the global fog, which has none.
    pub surface: [f32; 4],
}

impl FogVolume {
    /// `R_LoadFogs` for fog `index` (0-based, the bsp's order), with its shader's
    /// fogParms (or the original's red at 250 units when the shader has none).
    pub fn load(bsp: &Bsp, index: usize, parms: &swjkja_shader::FogParms) -> Self {
        let fog = &bsp.fogs[index];
        let color = parms.color.map(|c| (c * 255.0) as u8);
        let depth = parms.depth_for_opaque.max(1.0);
        let mut surface = [0.0; 4];
        if fog.brush >= 0 && fog.visible_side != -1 {
            let first_side = bsp.brushes[fog.brush as usize].first_side;
            let side = &bsp.brush_sides[(first_side + fog.visible_side) as usize];
            let plane = &bsp.planes[side.plane as usize];
            surface = [
                -plane.normal[0],
                -plane.normal[1],
                -plane.normal[2],
                -plane.dist,
            ];
        }
        Self {
            color: [color[0], color[1], color[2], 255],
            tc_scale: 1.0 / (depth * 8.0),
            surface,
        }
    }
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `RB_CalcFogTexCoords` for a world vertex `v`, seen from `eye` looking along
/// `forward`: s grows with eye-space depth, t says how far the vertex is inside the
/// fog (cut at the fog plane when the eye is outside it).
pub fn fog_tex_coords(fog: &FogVolume, v: Vec3, eye: Vec3, forward: Vec3) -> [f32; 2] {
    let surface = [fog.surface[0], fog.surface[1], fog.surface[2]];
    let depth_w = -fog.surface[3];
    let eye_t = dot(eye, surface) + depth_w;
    let s = (dot(v, forward) - dot(eye, forward)) * fog.tc_scale + 1.0 / 512.0;
    let mut t = dot(v, surface) + depth_w;
    if eye_t < 0.0 {
        t = if t < 1.0 {
            1.0 / 32.0
        } else {
            1.0 / 32.0 + 30.0 / 32.0 * t / (t - eye_t)
        };
    } else {
        t = if t < 0.0 { 1.0 / 32.0 } else { 31.0 / 32.0 };
    }
    if s.is_nan() { [0.0, 0.0] } else { [s, t] }
}

/// `R_FogFactor`: the fog's opacity (0-1) at fog texture coordinates (s, t), from
/// the original's table of square roots in 256 steps.
pub fn fog_factor(s: f32, t: f32) -> f32 {
    let mut s = s - 1.0 / 512.0;
    if s < 0.0 || t < 1.0 / 32.0 {
        return 0.0;
    }
    if t < 31.0 / 32.0 {
        s *= (t - 1.0 / 32.0) / (30.0 / 32.0);
    }
    s = (s * 8.0).min(1.0);
    ((s * 255.0) as i32 as f32 / 255.0).sqrt()
}

pub const FOG_S: usize = 256;
pub const FOG_T: usize = 32;

/// `R_CreateFogImage`: white, with the fog factor at each texel centre as alpha.
pub fn fog_image() -> Vec<u8> {
    let mut data = vec![255u8; FOG_S * FOG_T * 4];
    for y in 0..FOG_T {
        for x in 0..FOG_S {
            let d = fog_factor(
                (x as f32 + 0.5) / FOG_S as f32,
                (y as f32 + 0.5) / FOG_T as f32,
            );
            data[(y * FOG_S + x) * 4 + 3] = (255.0 * d) as u8;
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_factor_is_zero_outside_and_rises_as_a_square_root() {
        // At the start of the distance axis, and at the top row (outside the fog).
        assert_eq!(fog_factor(1.0 / 512.0, 0.5), 0.0);
        assert_eq!(fog_factor(0.5, 0.0), 0.0);
        // Fully inside (t = 31/32): s = 1/512 + 1/32 is 1/4 of the clamp range,
        // index 63 of 255.
        assert_eq!(
            fog_factor(1.0 / 512.0 + 1.0 / 32.0, 31.0 / 32.0),
            (63.0f32 / 255.0).sqrt()
        );
        // Past 1/8 + 1/512 the fog is opaque.
        assert_eq!(fog_factor(0.5, 31.0 / 32.0), 1.0);
    }

    #[test]
    fn fog_image_alpha_follows_the_factor() {
        let image = fog_image();
        assert_eq!(image.len(), 256 * 32 * 4);
        // Row 0 (t = 1/64) is above the fog: clear. The last row's right half is opaque.
        assert_eq!(image[3], 0);
        assert_eq!(image[(31 * 256 + 200) * 4 + 3], 255);
        assert_eq!(image[(31 * 256 + 200) * 4], 255);
    }

    #[test]
    fn tex_coords_inside_and_outside_a_fog_volume() {
        // A fog filling z < 100: the surface plane faces down into it, t = 100 - z.
        let fog = FogVolume {
            color: [0; 4],
            tc_scale: 1.0 / (8.0 * 400.0),
            surface: [0.0, 0.0, -1.0, -100.0],
        };
        let forward = [1.0, 0.0, 0.0];
        // Eye inside the fog: a point 400 units ahead is fully in (t = 31/32), at
        // 400 / 3200 + 1/512 along s.
        let st = fog_tex_coords(&fog, [400.0, 0.0, 0.0], [0.0, 0.0, 0.0], forward);
        assert_eq!(st, [400.0 / 3200.0 + 1.0 / 512.0, 31.0 / 32.0]);
        assert_eq!(fog_factor(st[0], st[1]), 1.0);
        // Eye above the fog (eyeT = -100): a point 50 units under the surface is cut
        // at the plane: t = 1/32 + 30/32 * 50 / 150.
        let st = fog_tex_coords(&fog, [0.0, 0.0, 50.0], [0.0, 0.0, 200.0], forward);
        assert_eq!(st[1], 1.0 / 32.0 + 30.0 / 32.0 * 50.0 / 150.0);
        // And a point above the fog is not fogged.
        let st = fog_tex_coords(&fog, [0.0, 0.0, 150.0], [0.0, 0.0, 200.0], forward);
        assert_eq!(st[1], 1.0 / 32.0);
        assert_eq!(fog_factor(st[0], st[1]), 0.0);
    }

    #[test]
    fn global_fog_has_no_surface_and_fogs_everything() {
        let fog = FogVolume {
            color: [0; 4],
            tc_scale: 0.001,
            surface: [0.0; 4],
        };
        assert_eq!(
            fog_tex_coords(&fog, [5.0, 0.0, -9999.0], [0.0; 3], [1.0, 0.0, 0.0])[1],
            31.0 / 32.0
        );
    }
}
