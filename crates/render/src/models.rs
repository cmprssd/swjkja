//! The map's static models (`misc_model_static`), drawn the way multiplayer draws
//! them: cgame's `CG_DrawMiscStaticModels` culls each one by a sphere, the PVS and
//! the map's `distanceCull`, then the renderer's `R_AddMD3Surfaces` picks a level of
//! detail (`R_ComputeLOD`), culls its box (`R_CullModel`, which skips the sphere for
//! these scaled entities) and picks a fog (`R_ComputeFogNum`).
//!
//! The models never move, so each placement's surfaces are baked into world space at
//! load; their normals carry the axes' scale, which is what the original's lighting
//! sees (`R_SetupEntityLighting` turns the light into the model's scaled axes).

use std::collections::HashMap;
use std::ops::Range;

use swjkja_bsp::{Bsp, DrawVert};
use swjkja_fs::Vfs;
use swjkja_model::md3::{Md3, Md3Model};
use swjkja_world::Geometry;
use swjkja_world::static_model::StaticModel;

use crate::camera::Camera;
use crate::fog::FogVolume;
use crate::vis::{Frustum, WorldVis};

pub type Vec3 = [f32; 3];

/// `r_lodscale`, the original's default.
const LOD_SCALE: f32 = 5.0;
/// The worldspawn's "distanceCull" when it has none (`DEFAULT_DISTANCE_CULL`).
const DEFAULT_DISTANCE_CULL: f32 = 6000.0;

/// One surface of one level of one placed model, in world space.
pub struct Piece {
    /// The surface's shader (skin 0), or "" for none (the default shader).
    pub shader: String,
    /// Fog number (0 none, else the map's fog + 1), as for world surfaces.
    pub fog: usize,
    /// A triangle soup whose vertices hold the entity number in `lightmap[3][1]`.
    pub geometry: Geometry,
}

pub struct ModelLod {
    /// Frame 0's bounds in the model's own space.
    pub bounds: [Vec3; 2],
    /// Indexes into [`StaticModels::pieces`].
    pub pieces: Range<usize>,
}

pub struct Instance {
    pub origin: Vec3,
    /// Scaled axes.
    pub axes: [Vec3; 3],
    /// The point cgame culls by: the origin, 1 + zoffset units up.
    pub cull_origin: Vec3,
    pub cull_cluster: i32,
    /// cgame's culling radius: the scaled bounds' (`R_ModelBounds`, level 0).
    pub radius: f32,
    /// `R_ComputeLOD`'s radius: level 0's frame 0 bounds, unscaled.
    pub lod_radius: f32,
    /// Most detailed first.
    pub lods: Vec<ModelLod>,
}

pub struct StaticModels {
    /// Entity `n + 1` is instance `n` (entity 0 is the world).
    pub instances: Vec<Instance>,
    pub pieces: Vec<Piece>,
    /// Every shader the models register, in the order the original registers them
    /// (`R_LoadMD3`, levels from least detailed to most).
    pub shaders: Vec<String>,
    distance_cull: f32,
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(v: Vec3) -> f32 {
    dot(v, v).sqrt()
}

/// `RadiusFromBounds`.
pub fn radius_from_bounds([mins, maxs]: [Vec3; 2]) -> f32 {
    length([0, 1, 2].map(|i| mins[i].abs().max(maxs[i].abs())))
}

/// `origin + Σ v[k] * axes[k]`.
fn transform(origin: Vec3, axes: &[Vec3; 3], v: Vec3) -> Vec3 {
    [0, 1, 2].map(|i| origin[i] + v[0] * axes[0][i] + v[1] * axes[1][i] + v[2] * axes[2][i])
}

/// `R_ComputeFogNum`: the first fog whose bounds the frame's sphere (the origin plus
/// the frame's unrotated, unscaled local origin) overlaps.
pub fn fog_num(fogs: &[FogVolume], origin: Vec3, md3: &Md3) -> usize {
    let frame = &md3.frames[0];
    let center = [0, 1, 2].map(|i| origin[i] + frame.local_origin[i]);
    fogs.iter()
        .position(|f| {
            (0..3).all(|j| {
                center[j] - frame.radius < f.bounds[1][j]
                    && center[j] + frame.radius > f.bounds[0][j]
            })
        })
        .map_or(0, |i| i + 1)
}

/// `R_ComputeLOD` with `r_lodbias` 0: the level for a model of `lod_radius` at
/// `origin`, from its projected size.
pub fn lod(count: usize, lod_radius: f32, origin: Vec3, camera: &Camera) -> usize {
    if count < 2 {
        return 0;
    }
    // ProjectRadius: the radius over the distance along the view axis, in units of
    // the half screen height.
    let forward = camera.axes()[0];
    let dist = dot(forward, origin) - dot(forward, camera.origin);
    let projected = if dist <= 0.0 {
        0.0
    } else {
        (lod_radius.abs() / (dist * camera.tan_half_fov().1)).min(1.0)
    };
    // Projected to nothing (the model crosses the view plane): the most detail.
    let flod = if projected != 0.0 {
        1.0 - projected * LOD_SCALE
    } else {
        0.0
    };
    ((flod * count as f32) as i32).clamp(0, count as i32 - 1) as usize
}

impl StaticModels {
    /// Loads the models the map places and bakes each placement (call
    /// [`StaticModels::locate`] before drawing). A model that
    /// does not load stops the original's map load; here its placements are skipped
    /// with a warning.
    pub fn load(vfs: &Vfs, bsp: &Bsp, fogs: &[FogVolume]) -> Self {
        let placements = swjkja_world::static_model::static_models(&bsp.entities, 0);
        let mut models: HashMap<String, Option<Md3Model>> = HashMap::new();
        let mut shaders = Vec::new();
        let mut instances = Vec::new();
        let mut pieces = Vec::new();
        for p in &placements {
            let model = models
                .entry(p.model.to_ascii_lowercase())
                .or_insert_with(|| match Md3Model::load(vfs, &p.model) {
                    Ok(m) => {
                        for md3 in m.lods.iter().rev() {
                            for s in &md3.surfaces {
                                shaders.extend(s.shaders.iter().cloned());
                            }
                        }
                        Some(m)
                    }
                    Err(e) => {
                        eprintln!("warning: misc_model_static {}: {e}", p.model);
                        None
                    }
                });
            let Some(model) = model else { continue };
            let entity = instances.len() + 1;
            let lods = model
                .lods
                .iter()
                .map(|md3| {
                    let first = pieces.len();
                    let fog = fog_num(fogs, p.origin, md3);
                    for s in &md3.surfaces {
                        pieces.push(bake(s, p, entity, fog));
                    }
                    ModelLod {
                        bounds: md3.frames[0].bounds,
                        pieces: first..pieces.len(),
                    }
                })
                .collect();
            let bounds = model.lods[0].frames[0].bounds;
            let scaled = bounds.map(|b| [0, 1, 2].map(|i| b[i] * p.scale[i]));
            let cull_origin = [p.origin[0], p.origin[1], p.origin[2] + 1.0 + p.zoffset];
            instances.push(Instance {
                origin: p.origin,
                axes: p.axes,
                cull_origin,
                cull_cluster: -1,
                radius: radius_from_bounds(scaled),
                lod_radius: radius_from_bounds(bounds),
                lods,
            });
        }
        let distance_cull = bsp
            .entities
            .first()
            .and_then(|e| e.get("distanceCull"))
            .and_then(|v| v.split_ascii_whitespace().next()?.parse().ok())
            .unwrap_or(DEFAULT_DISTANCE_CULL);
        Self {
            instances,
            pieces,
            shaders,
            distance_cull,
        }
    }

    /// Finds the cluster each model is culled by.
    pub fn locate(&mut self, vis: &WorldVis) {
        for m in &mut self.instances {
            m.cull_cluster = vis.cluster(m.cull_origin);
        }
    }

    /// The level of detail instance `n` draws in this view, or `None` if culled.
    pub fn visible(
        &self,
        n: usize,
        camera: &Camera,
        frustum: &Frustum,
        view_cluster: i32,
        vis: &WorldVis,
    ) -> Option<usize> {
        let m = &self.instances[n];
        // CG_DrawMiscStaticModels.
        if m.radius != 0.0 && frustum.culls_sphere(m.cull_origin, m.radius) {
            return None;
        }
        if !vis.can_see(view_cluster, m.cull_cluster) {
            return None;
        }
        let to_view = [0, 1, 2].map(|i| m.origin[i] - camera.origin[i]);
        if length(to_view) - m.radius > self.distance_cull {
            return None;
        }
        // R_AddMD3Surfaces: the level, then its box.
        let level = lod(m.lods.len(), m.lod_radius, m.origin, camera);
        let [mins, maxs] = m.lods[level].bounds;
        let corners: [Vec3; 8] = std::array::from_fn(|i| {
            let v = [
                if i & 1 == 0 { mins[0] } else { maxs[0] },
                if i & 2 == 0 { mins[1] } else { maxs[1] },
                if i & 4 == 0 { mins[2] } else { maxs[2] },
            ];
            transform(m.origin, &m.axes, v)
        });
        (!frustum.culls_points(&corners)).then_some(level)
    }
}

/// One surface's frame 0 in world space (`RB_SurfaceMesh`), with white vertex
/// colours and the entity number for the shader's entity table.
fn bake(s: &swjkja_model::md3::Surface, p: &StaticModel, entity: usize, fog: usize) -> Piece {
    let (xyz, normals) = s.frame(0);
    let verts: Vec<DrawVert> = xyz
        .iter()
        .zip(normals)
        .zip(&s.st)
        .map(|((&v, &n), &st)| DrawVert {
            xyz: transform(p.origin, &p.axes, v),
            st,
            lightmap: [[0.0; 2], [0.0; 2], [0.0; 2], [0.0, entity as f32]],
            normal: transform([0.0; 3], &p.axes, n),
            color: [[255; 4]; 4],
        })
        .collect();
    let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
    for v in &verts {
        bounds[0] = [0, 1, 2].map(|k| bounds[0][k].min(v.xyz[k]));
        bounds[1] = [0, 1, 2].map(|k| bounds[1][k].max(v.xyz[k]));
    }
    Piece {
        shader: s.shaders.first().cloned().unwrap_or_default(),
        fog,
        geometry: Geometry::Triangles {
            verts,
            indexes: s.indexes.clone(),
            bounds,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radius_reaches_the_farthest_corner() {
        assert_eq!(radius_from_bounds([[-3.0, 0.0, 0.0], [1.0, 4.0, 0.0]]), 5.0);
    }

    #[test]
    fn level_of_detail_follows_projected_size() {
        let camera = Camera::new([0.0; 3], [0.0; 3], 640, 480);
        // 80 degrees across at 4:3: tan(half vertical fov) = 0.629.
        let ty = camera.tan_half_fov().1;
        let at = |d: f32| lod(3, 10.0, [d, 0.0, 0.0], &camera);
        // Projected radius 1/5 or more: flod <= 0, the most detail.
        assert_eq!(at(10.0 / (0.2 * ty)), 0);
        // Projected radius 0.1: flod 0.5, times 3 levels: level 1.
        assert_eq!(at(10.0 / (0.1 * ty)), 1);
        // Far away: the least detail.
        assert_eq!(at(1.0e6), 2);
        // Behind the viewer: projected to nothing, the most detail.
        assert_eq!(at(-100.0), 0);
        // One level: always it.
        assert_eq!(lod(1, 10.0, [1.0e6, 0.0, 0.0], &camera), 0);
    }
}
