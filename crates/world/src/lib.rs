//! The map's drawable surfaces, built from a parsed `.bsp` the way OpenJK's
//! `rd-vanilla/tr_bsp.cpp` (`R_LoadSurfaces` and the `Parse*` functions) builds them:
//! planar faces, triangle soups, patches subdivided into grids and stitched, and
//! flares. This is CPU-side data; the renderer uploads it.

// This crate ports C loops over parallel arrays line by line; index loops keep each
// line comparable with the original.
#![allow(clippy::needless_range_loop)]

pub mod brush_entity;
pub mod collision;
pub mod light_grid;
pub mod npc;
pub mod patch;
pub mod rand;
pub mod script;
pub mod sound;
pub mod static_model;
pub mod stitch;
pub mod style;

use swjkja_bsp::{Bsp, DrawVert, SurfaceKind};

pub use patch::{Grid, Vec3};

/// `LIGHTMAP_BY_VERTEX`: the surface is lit by its vertex colours.
pub const LIGHTMAP_BY_VERTEX: i32 = -3;
/// Faces with more points than this get the default shader (`MAX_FACE_POINTS`).
pub const MAX_FACE_POINTS: usize = 64;

#[derive(Debug, Clone)]
pub struct Options {
    /// `r_mapOverBrightBits - overbrightBits`, at least 0: how many bits vertex
    /// colours and lightmaps are shifted up. 0 with the original's defaults.
    pub light_shift: u32,
    /// `r_subdivisions`, 4 by default.
    pub subdivisions: f32,
    /// `r_patchStitching`, on by default.
    pub patch_stitching: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            light_shift: 0,
            subdivisions: 4.0,
            patch_stitching: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Geometry {
    /// A planar face: vertices (with the plane's normal), indexes, and its plane.
    Face {
        verts: Vec<DrawVert>,
        indexes: Vec<u32>,
        normal: Vec3,
        dist: f32,
    },
    /// A triangle soup (usually a misc_model baked into the map), lit by vertex.
    Triangles {
        verts: Vec<DrawVert>,
        indexes: Vec<u32>,
        bounds: [Vec3; 2],
    },
    Grid(Grid),
    Flare {
        origin: Vec3,
        color: Vec3,
        normal: Vec3,
    },
    /// A patch with a `nodraw` shader, or an unknown surface type: not drawn.
    Skip,
}

#[derive(Debug, Clone)]
pub struct Surface {
    /// Index into the map's shader list.
    pub shader: usize,
    /// Fog volume + 1, or 0 for none.
    pub fog: usize,
    /// The lightmap pages the shader is resolved with (triangle soups use
    /// [`LIGHTMAP_BY_VERTEX`]).
    pub lightmaps: [i32; 4],
    pub lightmap_styles: [u8; 4],
    pub vertex_styles: [u8; 4],
    /// The face had more than [`MAX_FACE_POINTS`] points and must use the default
    /// shader.
    pub default_shader: bool,
    pub geometry: Geometry,
}

#[derive(Debug, Clone)]
pub struct World {
    pub surfaces: Vec<Surface>,
    /// How many vertices patch stitching inserted.
    pub stitches: usize,
}

/// `R_ColorShiftLightingBytes`: shifts a colour up by `shift` bits, scaling all
/// channels down together if one would pass 255 (keeping the hue, not saturating).
pub fn color_shift(c: [u8; 4], shift: u32) -> [u8; 4] {
    let [r, g, b] = [c[0], c[1], c[2]].map(|x| u32::from(x) << shift);
    let (r, g, b) = if (r | g | b) > 255 {
        let max = r.max(g).max(b);
        (r * 255 / max, g * 255 / max, b * 255 / max)
    } else {
        (r, g, b)
    };
    [r as u8, g as u8, b as u8, c[3]]
}

fn shifted(v: &DrawVert, shift: u32) -> DrawVert {
    let mut v = *v;
    v.color = v.color.map(|c| color_shift(c, shift));
    v
}

impl World {
    pub fn load(bsp: &Bsp, options: &Options) -> Self {
        let mut surfaces = Vec::with_capacity(bsp.surfaces.len());
        for s in &bsp.surfaces {
            let verts = &bsp.verts[s.first_vert as usize..][..s.num_verts as usize];
            let indexes = || -> Vec<u32> {
                bsp.indexes[s.first_index as usize..][..s.num_indexes as usize]
                    .iter()
                    .map(|&i| i as u32)
                    .collect()
            };
            let shader = s.shader as usize;
            let mut surface = Surface {
                shader,
                fog: (s.fog + 1) as usize,
                lightmaps: s.lightmap_num,
                lightmap_styles: s.lightmap_styles,
                vertex_styles: s.vertex_styles,
                default_shader: false,
                geometry: Geometry::Skip,
            };
            surface.geometry = match s.kind() {
                SurfaceKind::Planar => {
                    let points = verts.len().min(MAX_FACE_POINTS);
                    surface.default_shader = verts.len() > MAX_FACE_POINTS;
                    // Faces store no vertex normals: the original draws every vertex
                    // with the face's plane normal (RB_SurfaceFace), so it goes here.
                    let normal = s.lightmap_vecs[2];
                    let verts: Vec<DrawVert> = verts[..points]
                        .iter()
                        .map(|v| {
                            let mut v = shifted(v, options.light_shift);
                            v.normal = normal;
                            v
                        })
                        .collect();
                    let dist = verts.first().map_or(0.0, |v| dot(v.xyz, normal));
                    Geometry::Face {
                        verts,
                        indexes: indexes(),
                        normal,
                        dist,
                    }
                }
                SurfaceKind::TriangleSoup => {
                    surface.lightmaps = [LIGHTMAP_BY_VERTEX; 4];
                    let verts: Vec<DrawVert> = verts
                        .iter()
                        .map(|v| shifted(v, options.light_shift))
                        .collect();
                    let mut bounds = [[99999.0f32; 3], [-99999.0f32; 3]];
                    for v in &verts {
                        for k in 0..3 {
                            bounds[0][k] = bounds[0][k].min(v.xyz[k]);
                            bounds[1][k] = bounds[1][k].max(v.xyz[k]);
                        }
                    }
                    Geometry::Triangles {
                        verts,
                        indexes: indexes(),
                        bounds,
                    }
                }
                SurfaceKind::Patch => {
                    if bsp.shaders[shader].surface_flags & NODRAW != 0 {
                        Geometry::Skip
                    } else {
                        let points: Vec<DrawVert> = verts
                            .iter()
                            .map(|v| shifted(v, options.light_shift))
                            .collect();
                        let mut grid = patch::subdivide(
                            s.patch_width as usize,
                            s.patch_height as usize,
                            &points,
                            options.subdivisions,
                        );
                        // The LOD group: the bounds the compiler stored for the whole
                        // curved brush, not this patch's own.
                        let [b0, b1] = [s.lightmap_vecs[0], s.lightmap_vecs[1]];
                        grid.lod_origin = [0, 1, 2].map(|k| (b0[k] + b1[k]) * 0.5);
                        grid.lod_radius = length(sub(b0, grid.lod_origin));
                        Geometry::Grid(grid)
                    }
                }
                SurfaceKind::Flare => {
                    // `{ LIGHTMAP_BY_VERTEX }` in C: only the first entry is set.
                    surface.lightmaps = [LIGHTMAP_BY_VERTEX, 0, 0, 0];
                    Geometry::Flare {
                        origin: s.lightmap_origin,
                        color: s.lightmap_vecs[0],
                        normal: s.lightmap_vecs[2],
                    }
                }
                SurfaceKind::Bad => Geometry::Skip,
            };
            surfaces.push(surface);
        }

        // Stitch and LOD-sync the patches.
        let mut grids: Vec<Option<Grid>> = surfaces
            .iter_mut()
            .map(
                |s| match std::mem::replace(&mut s.geometry, Geometry::Skip) {
                    Geometry::Grid(g) => Some(g),
                    other => {
                        s.geometry = other;
                        None
                    }
                },
            )
            .collect();
        let stitches = if options.patch_stitching {
            stitch::stitch_all(&mut grids)
        } else {
            0
        };
        stitch::fix_shared_vertex_lod_error(&mut grids);
        for (s, g) in surfaces.iter_mut().zip(grids) {
            if let Some(g) = g {
                s.geometry = Geometry::Grid(g);
            }
        }
        Self { surfaces, stitches }
    }
}

/// `SURF_NODRAW`.
const NODRAW: i32 = 0x20_0000;

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}
