//! The light grid that lights models, and the light an entity gets from it, as
//! OpenJK's `rd-vanilla` computes them: `R_LoadLightGrid` and `R_LoadLightGridArray`
//! (`tr_bsp.cpp`), `R_SetupEntityLightingGrid` and `R_SetupEntityLighting`
//! (`tr_light.cpp`), without dynamic lights.

use swjkja_bsp::{Bsp, GridPoint, MAX_LIGHTMAPS};

use crate::style::MAX_LIGHT_STYLES;
use crate::{Vec3, color_shift};

/// A grid sample's style slot that is unused (`LS_LSNONE`); in the first slot, the
/// sample is inside a wall.
const STYLE_NONE: u8 = 0xff;
/// `r_ambientScale` and `r_directedScale`, the original's defaults.
const AMBIENT_SCALE: f32 = 0.6;
const DIRECTED_SCALE: f32 = 1.0;
/// `tr.identityLight * 150` when there is no grid, `tr.identityLightByte` the most
/// ambient light, and the minimum ambient every entity gets (`* 32`), or one drawn
/// with `RF_MORELIGHT` (`* 96`); identityLight is 1 without overbright bits.
const NO_GRID_LIGHT: f32 = 150.0;
const IDENTITY_LIGHT_BYTE: f32 = 255.0;
const MIN_AMBIENT: f32 = 32.0;
const MORE_LIGHT_AMBIENT: f32 = 96.0;

/// `tr.sinTable`: 1024 samples of one sine period.
fn sin_table(i: usize) -> f32 {
    (i as f64 * std::f64::consts::TAU / 1024.0).sin() as f32
}

/// An entity's light for `RB_CalcDiffuseColor`: colours in 0-255, the direction it
/// comes from in world space (unit length, or zero).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityLight {
    pub ambient: Vec3,
    pub directed: Vec3,
    pub dir: Vec3,
}

#[derive(Debug, Clone)]
pub struct LightGrid {
    origin: Vec3,
    inverse_size: Vec3,
    bounds: [i32; 3],
    points: Vec<GridPoint>,
    /// Per cell, its point. Empty when the map has no usable grid.
    array: Vec<u16>,
}

fn atof3(s: &str) -> Option<Vec3> {
    let mut it = s.split_ascii_whitespace().map(|t| t.parse::<f32>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

impl LightGrid {
    /// The map's grid; `light_shift` as in [`crate::Options`].
    pub fn load(bsp: &Bsp, light_shift: u32) -> Self {
        // R_LoadEntities: the worldspawn's "gridsize", 64 64 128 by default.
        let size = bsp
            .entities
            .first()
            .and_then(|e| e.get("gridsize"))
            .and_then(atof3)
            .unwrap_or([64.0, 64.0, 128.0]);
        let world = bsp.models.first();
        let bounds = world.map_or([[0.0; 3]; 2], |m| [m.mins, m.maxs]);
        Self::new(size, bounds, &bsp.light_grid, &bsp.light_array, light_shift)
    }

    /// `R_LoadLightGrid` and `R_LoadLightGridArray`: a grid of `size` cells over the
    /// world model's `bounds`, its `points`, and per cell the point it uses.
    pub fn new(
        size: Vec3,
        [mins, maxs]: [Vec3; 2],
        points: &[GridPoint],
        array: &[u16],
        light_shift: u32,
    ) -> Self {
        let mut origin = [0.0; 3];
        let mut bounds = [0; 3];
        for i in 0..3 {
            origin[i] = size[i] * (mins[i] / size[i]).ceil();
            let max = size[i] * (maxs[i] / size[i]).floor();
            bounds[i] = ((max - origin[i]) / size[i] + 1.0) as i32;
        }
        let shift = |c: [u8; 3]| {
            let s = color_shift([c[0], c[1], c[2], 0], light_shift);
            [s[0], s[1], s[2]]
        };
        let points = points
            .iter()
            .map(|p| {
                let mut p = *p;
                p.ambient = p.ambient.map(shift);
                p.direct = p.direct.map(shift);
                p
            })
            .collect();
        // An array of the wrong size drops the grid ("light grid array mismatch").
        let cells = bounds.iter().map(|&b| b.max(0) as usize).product::<usize>();
        let array = if array.len() == cells {
            array.to_vec()
        } else {
            Vec::new()
        };
        Self {
            origin,
            inverse_size: size.map(|s| 1.0 / s),
            bounds,
            points,
            array,
        }
    }

    /// Whether the map has a usable grid.
    pub fn exists(&self) -> bool {
        !self.array.is_empty()
    }

    /// `R_SetupEntityLightingGrid`: the grid's light at `point`, with each style
    /// coloured by `styles` (RGB 0-255, as [`crate::style::LightStyles::colors`]).
    pub fn sample(&self, point: Vec3, styles: &[[u8; 4]; MAX_LIGHT_STYLES]) -> EntityLight {
        let mut pos = [0i32; 3];
        let mut frac = [0f32; 3];
        for i in 0..3 {
            let v = (point[i] - self.origin[i]) * self.inverse_size[i];
            pos[i] = v.floor() as i32;
            frac[i] = v - pos[i] as f32;
            pos[i] = pos[i].clamp(0, (self.bounds[i] - 1).max(0));
        }
        let step = [1, self.bounds[0], self.bounds[0] * self.bounds[1]];
        let start = pos[0] * step[0] + pos[1] * step[1] + pos[2] * step[2];
        let (mut ambient, mut directed, mut direction) = ([0f32; 3], [0f32; 3], [0f32; 3]);
        let mut total = 0f32;
        for i in 0..8 {
            let mut factor = 1f32;
            let mut cell = start;
            for j in 0..3 {
                if i & (1 << j) != 0 {
                    factor *= frac[j];
                    cell += step[j];
                } else {
                    factor *= 1.0 - frac[j];
                }
            }
            // "we've gone off the array somehow".
            let Some(&index) = usize::try_from(cell).ok().and_then(|c| self.array.get(c)) else {
                continue;
            };
            let Some(data) = self.points.get(usize::from(index)) else {
                continue;
            };
            if data.styles[0] == STYLE_NONE {
                continue; // inside a wall
            }
            total += factor;
            for j in 0..MAX_LIGHTMAPS {
                let style = data.styles[j];
                if style == STYLE_NONE {
                    break;
                }
                let color = styles[usize::from(style) % MAX_LIGHT_STYLES];
                for c in 0..3 {
                    let s = f32::from(color[c]) / 255.0;
                    ambient[c] += factor * f32::from(data.ambient[j][c]) * s;
                    directed[c] += factor * f32::from(data.direct[j][c]) * s;
                }
            }
            let lat = usize::from(data.lat_long[1]) * 4;
            let lng = usize::from(data.lat_long[0]) * 4;
            let normal = [
                sin_table((lat + 256) & 1023) * sin_table(lng),
                sin_table(lat) * sin_table(lng),
                sin_table((lng + 256) & 1023),
            ];
            for c in 0..3 {
                direction[c] += factor * normal[c];
            }
        }
        if total > 0.0 && total < 0.99 {
            let scale = 1.0 / total;
            ambient = ambient.map(|x| x * scale);
            directed = directed.map(|x| x * scale);
        }
        EntityLight {
            ambient: ambient.map(|x| x * AMBIENT_SCALE),
            directed: directed.map(|x| x * DIRECTED_SCALE),
            dir: normalize(direction),
        }
    }

    /// `R_SetupEntityLighting` for an entity lit at `point`: the grid's light (or a
    /// flat 150 without a grid), plus the minimum ambient (the larger one with
    /// `more_light`, `RF_MORELIGHT`), clamped. `sun_direction` is the light's
    /// direction without a grid (`tr.sunDirection`).
    pub fn entity_light(
        &self,
        point: Vec3,
        styles: &[[u8; 4]; MAX_LIGHT_STYLES],
        sun_direction: Vec3,
        more_light: bool,
    ) -> EntityLight {
        let mut light = if self.exists() {
            self.sample(point, styles)
        } else {
            EntityLight {
                ambient: [NO_GRID_LIGHT; 3],
                directed: [NO_GRID_LIGHT; 3],
                dir: sun_direction,
            }
        };
        let min = if more_light {
            MORE_LIGHT_AMBIENT
        } else {
            MIN_AMBIENT
        };
        light.ambient = light.ambient.map(|a| (a + min).min(IDENTITY_LIGHT_BYTE));
        // The direction is rebuilt as dir * |directed| and normalized again: without
        // directed light it becomes zero.
        let d = length(light.directed);
        light.dir = normalize(light.dir.map(|x| x * d));
        light
    }
}

fn length(v: Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// `VectorNormalize`: a zero vector stays zero.
fn normalize(v: Vec3) -> Vec3 {
    let l = length(v);
    if l == 0.0 { v } else { v.map(|x| x / l) }
}
