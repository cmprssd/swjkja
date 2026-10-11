//! Box traces against the map's brushes and the solid entities, as the original's
//! collision code runs them (`code/qcommon/cm_trace.cpp`: `CM_Trace`,
//! `CM_TraceThroughBrush`): a box swept from one point to another stops where it
//! would first enter a brush whose contents match the mask, held
//! [`SURFACE_CLIP_EPSILON`] off the brush's face.
//!
//! Every brush is tested against the sweep's bounds (the original finds the world's
//! through the leaves the sweep crosses, which tests the same brushes). Patches,
//! NPCs, and the rotation and movement of brush entities are left out.

use swjkja_bsp::Bsp;

use crate::Vec3;
use crate::static_model::{Rules, spawns_at, vector_prefix};
use crate::style::atoi;

pub const CONTENTS_SOLID: i32 = 0x1;
pub const CONTENTS_MONSTERCLIP: i32 = 0x20;
pub const CONTENTS_BOTCLIP: i32 = 0x40;
pub const CONTENTS_BODY: i32 = 0x100;
pub const CONTENTS_TERRAIN: i32 = 0x1000;
pub const CONTENTS_OPAQUE: i32 = 0x8000;
/// What blocks an NPC (`MASK_NPCSOLID`).
pub const MASK_NPCSOLID: i32 =
    CONTENTS_SOLID | CONTENTS_MONSTERCLIP | CONTENTS_BODY | CONTENTS_TERRAIN;
/// `MIN_WORLD_COORD`: how far down a drop to the floor looks.
pub const MIN_WORLD_COORD: f32 = -64.0 * 1024.0;
/// How far a trace stops short of the face it hits.
pub const SURFACE_CLIP_EPSILON: f32 = 0.125;

/// One brush: its contents, its planes (normal and distance), and its bounds.
#[derive(Debug, Clone)]
struct Brush {
    contents: i32,
    planes: Vec<(Vec3, f32)>,
    bounds: [Vec3; 2],
}

/// The brushes and boxes traces stop at. The default has none: nothing is hit.
#[derive(Debug, Clone, Default)]
pub struct Collision {
    brushes: Vec<Brush>,
}

/// Where a trace stopped (`trace_t`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trace {
    /// How much of the way from start to end the box went, 1 for all of it.
    pub fraction: f32,
    pub end: Vec3,
    /// The box started inside a brush.
    pub start_solid: bool,
    /// The box never left it.
    pub all_solid: bool,
}

impl Collision {
    /// The brushes of `bsp`'s world model (`CMod_LoadBrushes`), contents from their
    /// shader.
    pub fn new(bsp: &Bsp) -> Self {
        let mut c = Self::default();
        c.add_model(bsp, 0, [0.0; 3]);
        c
    }

    /// The world and the brush entities solid as the map spawns under `rules`: in
    /// single player, the `func_` entities that spawn, at their origin, each brush
    /// with its own contents (`SV_SetBrushModel`), except a `func_usable` or
    /// `func_wall` started off (spawnflag 1). Their rotation is left out, and so is
    /// the open position of a door that starts open.
    pub fn at_spawn(bsp: &Bsp, rules: Rules) -> Self {
        let mut c = Self::new(bsp);
        let Rules::SinglePlayer { skill } = rules else {
            return c;
        };
        for e in bsp.entities.iter().filter(|e| spawns_at(e, skill)) {
            let Some(model) = e.get("model").and_then(|m| m.strip_prefix('*')) else {
                continue;
            };
            let class = e.get("classname").unwrap_or("").to_ascii_lowercase();
            let off = e.get("spawnflags").map_or(0, atoi) & 1 != 0;
            if !class.starts_with("func_")
                || off && matches!(class.as_str(), "func_usable" | "func_wall")
            {
                continue;
            }
            let origin = e.get("origin").map_or([0.0; 3], vector_prefix);
            c.add_model(bsp, atoi(model).max(0) as usize, origin);
        }
        c
    }

    /// Brush model `index` of `bsp`, moved by `offset`; nothing for one it lacks.
    fn add_model(&mut self, bsp: &Bsp, index: usize, offset: Vec3) {
        let Some(model) = bsp.models.get(index) else {
            return;
        };
        let range = |first: i32, n: i32| first.max(0) as usize..(first.max(0) + n.max(0)) as usize;
        self.add_brushes(
            bsp.brushes[range(model.first_brush, model.num_brushes)]
                .iter()
                .map(|b| {
                    let planes = bsp.brush_sides[range(b.first_side, b.num_sides)]
                        .iter()
                        .map(|s| {
                            let p = bsp.planes[s.plane as usize];
                            (p.normal, p.dist + dot(p.normal, offset))
                        })
                        .collect();
                    (bsp.shaders[b.shader as usize].content_flags, planes)
                }),
        );
    }

    /// A solid entity's box, `mins`..`maxs` in world space (`CM_TempBoxModel`).
    pub fn add_box(&mut self, mins: Vec3, maxs: Vec3, contents: i32) {
        let mut planes = Vec::new();
        for i in 0..3 {
            let mut n = [0.0; 3];
            n[i] = -1.0;
            planes.push((n, -mins[i]));
            n[i] = 1.0;
            planes.push((n, maxs[i]));
        }
        self.add_brushes([(contents, planes)]);
    }

    /// Brushes given as their contents and planes (normal and distance), the first
    /// six the axial ones as the compiler writes them (-x, +x, -y, +y, -z, +z),
    /// which give the bounds (`CM_BoundBrush`). One with fewer sides is skipped.
    pub fn add_brushes(&mut self, brushes: impl IntoIterator<Item = (i32, Vec<(Vec3, f32)>)>) {
        self.brushes.extend(
            brushes
                .into_iter()
                .filter(|(_, planes)| planes.len() >= 6)
                .map(|(contents, planes)| {
                    let d = |i: usize| planes[i].1;
                    Brush {
                        contents,
                        bounds: [[-d(0), -d(2), -d(4)], [d(1), d(3), d(5)]],
                        planes,
                    }
                }),
        );
    }

    /// Sweeps the box `mins`..`maxs` from `start` to `end` through the brushes whose
    /// contents meet `mask`.
    pub fn trace(&self, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3, mask: i32) -> Trace {
        let mut trace = Trace {
            fraction: 1.0,
            end,
            start_solid: false,
            all_solid: false,
        };
        // The sweep's bounds, for skipping brushes it cannot touch.
        let lo: Vec3 = [0, 1, 2].map(|i| start[i].min(end[i]) + mins[i] - 1.0);
        let hi: Vec3 = [0, 1, 2].map(|i| start[i].max(end[i]) + maxs[i] + 1.0);
        for b in &self.brushes {
            if b.contents & mask == 0
                || (0..3).any(|i| b.bounds[0][i] > hi[i] || b.bounds[1][i] < lo[i])
            {
                continue;
            }
            through_brush(&mut trace, b, start, end, mins, maxs);
            if trace.all_solid {
                break;
            }
        }
        if trace.fraction < 1.0 {
            trace.end = [0, 1, 2].map(|i| start[i] + trace.fraction * (end[i] - start[i]));
        }
        trace
    }

    /// Where a box at `origin` comes to rest dropped onto the floor below
    /// (`gi.trace` down to [`MIN_WORLD_COORD`]): `None` when it starts in solid or
    /// nothing is below.
    pub fn drop_to_floor(&self, origin: Vec3, mins: Vec3, maxs: Vec3, mask: i32) -> Option<Vec3> {
        let bottom = [origin[0], origin[1], MIN_WORLD_COORD];
        let tr = self.trace(origin, bottom, mins, maxs, mask);
        (!tr.all_solid && !tr.start_solid && tr.fraction < 1.0).then_some(tr.end)
    }
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `CM_TraceThroughBrush` with `CM_PlaneCollision`, for an axis-aligned box.
fn through_brush(trace: &mut Trace, b: &Brush, start: Vec3, end: Vec3, mins: Vec3, maxs: Vec3) {
    let (mut enter, mut leave) = (-1.0f32, 1.0f32);
    let (mut start_out, mut get_out) = (false, false);
    for &(normal, plane_dist) in &b.planes {
        // The plane pushed out to the box's corner that meets it first.
        let corner = [0, 1, 2].map(|i| if normal[i] < 0.0 { maxs[i] } else { mins[i] });
        let dist = plane_dist - dot(corner, normal);
        let d1 = dot(start, normal) - dist;
        let d2 = dot(end, normal) - dist;
        if d2 > 0.0 {
            get_out = true;
        }
        if d1 > 0.0 {
            start_out = true;
        }
        // Wholly in front of this face: outside the brush.
        if d1 > 0.0 && (d2 >= SURFACE_CLIP_EPSILON || d2 >= d1) {
            return;
        }
        if d1 <= 0.0 && d2 <= 0.0 {
            continue;
        }
        if d1 > d2 {
            let f = d1 - SURFACE_CLIP_EPSILON;
            if f < 0.0 {
                enter = enter.max(0.0);
            } else if f > enter * (d1 - d2) {
                enter = f / (d1 - d2);
            }
        } else {
            let f = d1 + SURFACE_CLIP_EPSILON;
            if f < d1 - d2 {
                leave = leave.min(1.0);
            } else if f > leave * (d1 - d2) {
                leave = f / (d1 - d2);
            }
        }
    }
    if !start_out {
        trace.start_solid = true;
        if !get_out {
            trace.all_solid = true;
            trace.fraction = 0.0;
        }
        return;
    }
    if enter < leave && enter > -1.0 && enter < trace.fraction {
        trace.fraction = enter.max(0.0);
    }
}
