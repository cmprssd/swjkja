//! Marks laid on the world's surfaces, as the original lays an NPC's blob shadow
//! (`cg_shadows 1`): `_PlayerShadow` traces a small box down from the NPC,
//! `CG_ImpactMark` builds a square facing the floor it hits, and the renderer's
//! `R_MarkFragments` (`tr_marks.cpp`) chops the world's surfaces under the square
//! into the polygons drawn.
//!
//! The original finds the surfaces through the BSP leaves the projection's box
//! touches, at most 64 of them; this takes every world surface whose bounds meet the
//! box (the chop leaves nothing of one outside the projection either way), without
//! the cap.

use crate::collision::{Collision, MASK_PLAYERSOLID};
use crate::{Geometry, Surface, Vec3};

/// `SURF_NOIMPACT`, `SURF_NOMARKS`: surfaces that take no marks.
const SURF_NOIMPACT: i32 = 0x8_0000;
const SURF_NOMARKS: i32 = 0x10_0000;
/// `CONTENTS_FOG`.
const CONTENTS_FOG: i32 = 0x8;
/// The renderer's limit on a chopped polygon's points (`tr_marks.cpp`).
const MAX_CHOP_POINTS: usize = 64;
/// cgame's limits on one mark (`cg_marks.cpp`): its points, its fragments, and the
/// points of a fragment it keeps (`MAX_VERTS_ON_POLY`, cg_local.h).
pub const MAX_MARK_POINTS: usize = 384;
const MAX_MARK_FRAGMENTS: usize = 128;
pub const MAX_VERTS_ON_POLY: usize = 10;
/// How far down a shadow looks for the floor (`SHADOW_DISTANCE`).
pub const SHADOW_DISTANCE: f32 = 128.0;
/// How far from the view shadows are drawn (`r_shadowRange`'s default).
pub const SHADOW_RANGE: f32 = 1000.0;
/// An NPC's shadow radius (`CG_PlayerShadow`: 16, but 64 for a rancor and an
/// AT-ST's body, 28 for each of its feet).
pub const SHADOW_RADIUS: f32 = 16.0;

/// One world surface marks can land on: its bounds and triangles.
#[derive(Debug, Clone)]
struct MarkSurface {
    bounds: [Vec3; 2],
    kind: Kind,
    triangles: Vec<[Vec3; 3]>,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    /// A planar face (`SF_FACE`): taken whole if its plane goes through the
    /// projection's box and faces against the projection.
    Face { normal: Vec3, dist: f32 },
    /// A patch (`SF_GRID`): each cell's two triangles, the first taken if it faces
    /// against the projection by more than 0.1, the second by more than 0.05.
    Grid,
    /// A triangle soup (`SF_TRIANGLES`): each triangle facing against the
    /// projection by more than 0.1.
    Triangles,
}

/// The world's surfaces that take marks.
#[derive(Debug, Clone, Default)]
pub struct MarkSurfaces {
    surfaces: Vec<MarkSurface>,
}

impl MarkSurfaces {
    /// The surfaces of `surfaces` that take marks (faces, patches and triangle
    /// soups), those whose shader `flags` (surface and content flags) have no
    /// `SURF_NOIMPACT`, `SURF_NOMARKS` or `CONTENTS_FOG`.
    pub fn new<'a>(surfaces: impl IntoIterator<Item = (&'a Surface, (i32, i32))>) -> Self {
        let mut out = Vec::new();
        for (s, (surface_flags, content_flags)) in surfaces {
            if surface_flags & (SURF_NOIMPACT | SURF_NOMARKS) != 0
                || content_flags & CONTENTS_FOG != 0
            {
                continue;
            }
            let indexed = |verts: &[swjkja_bsp::DrawVert], indexes: &[u32]| {
                indexes
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter_map(|t| {
                        let v = |k: usize| verts.get(t[k] as usize).map(|v| v.xyz);
                        Some([v(0)?, v(1)?, v(2)?])
                    })
                    .collect::<Vec<_>>()
            };
            let (kind, triangles) = match &s.geometry {
                Geometry::Face {
                    verts,
                    indexes,
                    normal,
                    dist,
                } => (
                    Kind::Face {
                        normal: *normal,
                        dist: *dist,
                    },
                    indexed(verts, indexes),
                ),
                Geometry::Triangles { verts, indexes, .. } => {
                    (Kind::Triangles, indexed(verts, indexes))
                }
                Geometry::Grid(g) => {
                    let mut triangles = Vec::new();
                    for row in 0..g.height - 1 {
                        for column in 0..g.width - 1 {
                            let v = |c: usize, r: usize| g.vert(column + c, row + r).xyz;
                            triangles.push([v(0, 0), v(0, 1), v(1, 0)]);
                            triangles.push([v(1, 0), v(0, 1), v(1, 1)]);
                        }
                    }
                    (Kind::Grid, triangles)
                }
                Geometry::Flare { .. } | Geometry::Skip => continue,
            };
            let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
            for p in triangles.iter().flatten() {
                for k in 0..3 {
                    bounds[0][k] = bounds[0][k].min(p[k]);
                    bounds[1][k] = bounds[1][k].max(p[k]);
                }
            }
            if !triangles.is_empty() {
                out.push(MarkSurface {
                    bounds,
                    kind,
                    triangles,
                });
            }
        }
        Self { surfaces: out }
    }

    /// `R_MarkFragments`: the pieces of the surfaces under the polygon `points`
    /// projected along `projection`, each a polygon of at most
    /// [`MAX_CHOP_POINTS`] points; at most [`MAX_MARK_FRAGMENTS`] pieces, and
    /// [`MAX_MARK_POINTS`] points in all (a piece that would pass that is dropped).
    pub fn fragments(&self, points: &[Vec3], projection: Vec3) -> Vec<Vec<Vec3>> {
        let dir = normalize(projection);
        let mut mins = [f32::MAX; 3];
        let mut maxs = [f32::MIN; 3];
        for &p in points {
            for q in [p, add(p, projection), madd(p, -20.0, dir)] {
                for k in 0..3 {
                    mins[k] = mins[k].min(q[k]);
                    maxs[k] = maxs[k].max(q[k]);
                }
            }
        }
        let points = &points[..points.len().min(MAX_CHOP_POINTS)];
        // The projected polygon's sides, then its near and far planes.
        let mut planes: Vec<(Vec3, f32)> = (0..points.len())
            .map(|i| {
                let v1 = sub(points[(i + 1) % points.len()], points[i]);
                let v2 = scale(projection, -1.0);
                let n = normalize(cross(v1, v2));
                (n, dot(n, points[i]))
            })
            .collect();
        planes.push((dir, dot(dir, points[0]) - 32.0));
        let back = scale(dir, -1.0);
        planes.push((back, dot(back, points[0]) - 20.0));

        let mut out: Vec<Vec<Vec3>> = Vec::new();
        let mut total = 0;
        for s in &self.surfaces {
            if (0..3).any(|k| s.bounds[0][k] > maxs[k] || s.bounds[1][k] < mins[k]) {
                continue;
            }
            if let Kind::Face { normal, dist } = s.kind
                && (box_on_plane_side(mins, maxs, normal, dist) != 3 || dot(normal, dir) > -0.5)
            {
                continue;
            }
            for (i, t) in s.triangles.iter().enumerate() {
                let facing = match s.kind {
                    Kind::Face { .. } => true,
                    Kind::Grid => {
                        let limit = if i % 2 == 0 { -0.1 } else { -0.05 };
                        dot(triangle_normal(t), dir) < limit
                    }
                    Kind::Triangles => dot(triangle_normal(t), dir) < -0.1,
                };
                if !facing {
                    continue;
                }
                let mut poly = t.to_vec();
                for &(normal, dist) in &planes {
                    poly = chop_behind(&poly, normal, dist, 0.5);
                    if poly.is_empty() {
                        break;
                    }
                }
                if poly.is_empty() || total + poly.len() > MAX_MARK_POINTS {
                    continue;
                }
                total += poly.len();
                out.push(poly);
                if out.len() == MAX_MARK_FRAGMENTS {
                    return out;
                }
            }
        }
        out
    }
}

/// A point of a mark: where it is, its texture coordinates, and its alpha (0-255).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkVert {
    pub xyz: Vec3,
    pub st: [f32; 2],
    pub alpha: u8,
}

/// `CG_ImpactMark` for a temporary mark: a square of `radius` around `origin`
/// facing `dir`, turned `orientation` degrees about it, chopped onto the
/// surfaces 20 units behind it; each piece's points (at most
/// [`MAX_VERTS_ON_POLY`]) with the square's texture coordinates and `alpha` (0-1).
pub fn impact_mark(
    surfaces: &MarkSurfaces,
    origin: Vec3,
    dir: Vec3,
    orientation: f32,
    alpha: f32,
    radius: f32,
) -> Vec<Vec<MarkVert>> {
    let axis0 = normalize(dir);
    let axis1 = perpendicular(axis0);
    let axis2 = rotate_around(axis0, axis1, orientation);
    let axis1 = cross(axis0, axis2);
    let tc_scale = 0.5 / radius;
    let corner = |a: f32, b: f32| {
        [0, 1, 2].map(|i| origin[i] + a * radius * axis1[i] + b * radius * axis2[i])
    };
    let square = [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ];
    // The colour's bytes: a float stored to a byte truncates.
    let alpha = (alpha * 255.0) as u8;
    surfaces
        .fragments(&square, scale(dir, -20.0))
        .into_iter()
        .map(|f| {
            f.into_iter()
                .take(MAX_VERTS_ON_POLY)
                .map(|xyz| {
                    let delta = sub(xyz, origin);
                    MarkVert {
                        xyz,
                        st: [
                            0.5 + dot(delta, axis1) * tc_scale,
                            0.5 + dot(delta, axis2) * tc_scale,
                        ],
                        alpha,
                    }
                })
                .collect()
        })
        .collect()
}

/// Where `_PlayerShadow` lays a shadow cast from `origin`: the floor point and
/// normal a 14 x 14 x 2 box traced [`SHADOW_DISTANCE`] down through the world
/// (`MASK_PLAYERSOLID`) hits, and the shadow's alpha, fading with the height (1
/// minus the trace's fraction); `None` with no floor in reach or from inside a
/// brush.
pub fn shadow_floor(world: &Collision, origin: Vec3) -> Option<(Vec3, Vec3, f32)> {
    let end = [origin[0], origin[1], origin[2] - SHADOW_DISTANCE];
    let tr = world.trace(
        origin,
        end,
        [-7.0, -7.0, 0.0],
        [7.0, 7.0, 2.0],
        MASK_PLAYERSOLID,
    );
    if tr.fraction == 1.0 || (tr.start_solid && tr.all_solid) {
        return None;
    }
    Some((tr.end, tr.normal, 1.0 - tr.fraction))
}

/// `R_ChopPolyBehindPlane`: what of `points` lies in front of the plane (within
/// `epsilon` counts as on it); nothing for a polygon that might pass
/// [`MAX_CHOP_POINTS`].
fn chop_behind(points: &[Vec3], normal: Vec3, dist: f32, epsilon: f32) -> Vec<Vec3> {
    const FRONT: u8 = 0;
    const BACK: u8 = 1;
    const ON: u8 = 2;
    if points.len() >= MAX_CHOP_POINTS - 2 {
        return Vec::new();
    }
    let dists: Vec<f32> = points.iter().map(|&p| dot(p, normal) - dist).collect();
    let sides: Vec<u8> = dists
        .iter()
        .map(|&d| {
            if d > epsilon {
                FRONT
            } else if d < -epsilon {
                BACK
            } else {
                ON
            }
        })
        .collect();
    if !sides.contains(&FRONT) {
        return Vec::new();
    }
    if !sides.contains(&BACK) {
        return points.to_vec();
    }
    let n = points.len();
    let mut out = Vec::with_capacity(n + 2);
    for i in 0..n {
        let p1 = points[i];
        if sides[i] == ON {
            out.push(p1);
            continue;
        }
        if sides[i] == FRONT {
            out.push(p1);
        }
        let next = (i + 1) % n;
        if sides[next] == ON || sides[next] == sides[i] {
            continue;
        }
        let p2 = points[next];
        let d = dists[i] - dists[next];
        let t = if d == 0.0 { 0.0 } else { dists[i] / d };
        out.push([0, 1, 2].map(|j| p1[j] + t * (p2[j] - p1[j])));
    }
    out
}

/// `BoxOnPlaneSide`: 1 if the box is wholly in front of the plane, 2 if wholly
/// behind, 3 if it crosses.
fn box_on_plane_side(mins: Vec3, maxs: Vec3, normal: Vec3, dist: f32) -> u8 {
    let (mut near, mut far) = (0.0, 0.0);
    for k in 0..3 {
        if normal[k] >= 0.0 {
            far += normal[k] * maxs[k];
            near += normal[k] * mins[k];
        } else {
            far += normal[k] * mins[k];
            near += normal[k] * maxs[k];
        }
    }
    let mut sides = 0;
    if far >= dist {
        sides = 1;
    }
    if near < dist {
        sides |= 2;
    }
    sides
}

/// A triangle's normal as `R_MarkFragments` takes it: (a - b) x (c - b).
fn triangle_normal([a, b, c]: &[Vec3; 3]) -> Vec3 {
    normalize(cross(sub(*a, *b), sub(*c, *b)))
}

/// `PerpendicularVector`: `v`'s axis of least size, made perpendicular to `v`
/// (unit `v`).
fn perpendicular(v: Vec3) -> Vec3 {
    let mut pos = 0;
    let mut least = 1.0f32;
    for (k, x) in v.iter().enumerate() {
        if x.abs() < least {
            pos = k;
            least = x.abs();
        }
    }
    let mut axis = [0.0; 3];
    axis[pos] = 1.0;
    // ProjectPointOnPlane with a unit normal.
    normalize(sub(axis, scale(v, dot(axis, v))))
}

/// `RotatePointAroundVector`: `point` turned `degrees` about the unit `dir`.
fn rotate_around(dir: Vec3, point: Vec3, degrees: f32) -> Vec3 {
    let (s, c) = degrees.to_radians().sin_cos();
    let k = cross(dir, point);
    let along = dot(dir, point) * (1.0 - c);
    [0, 1, 2].map(|i| point[i] * c + k[i] * s + dir[i] * along)
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Vec3, s: f32) -> Vec3 {
    a.map(|x| x * s)
}
fn madd(a: Vec3, s: f32, b: Vec3) -> Vec3 {
    add(a, scale(b, s))
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
/// `VectorNormalize2`: a zero vector stays zero.
fn normalize(v: Vec3) -> Vec3 {
    let length = dot(v, v).sqrt();
    if length == 0.0 {
        v
    } else {
        scale(v, 1.0 / length)
    }
}
