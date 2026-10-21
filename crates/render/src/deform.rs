//! Vertex deforms, done on the CPU each frame like the original's
//! `RB_DeformTessGeometry` (OpenJK `rd-vanilla/tr_shade_calc.cpp`), over all the
//! vertices of one material batch: wave, bulge, move, autoSprite and autoSprite2.
//!
//! Not yet: `normal` (needs the original's noise table) and `text0`-`text7` (map text).

use swjkja_shader::{Deform, Func, Wave};

pub type Vec3 = [f32; 3];

/// A vertex as deforms see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeformVertex {
    pub xyz: Vec3,
    pub normal: Vec3,
    pub st: [f32; 2],
    pub lightmap: [[f32; 2]; 4],
    pub color: [u8; 4],
    /// A vertex-lit surface's colours for its second to fourth light styles.
    pub styled: [[u8; 4]; 3],
}

/// The view, for sprites: forward, left, up axes.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub forward: Vec3,
    pub left: Vec3,
    pub up: Vec3,
}

/// One entry of the original's 1024-entry function tables, at `x` (WAVEVALUE's
/// index: truncate `x * 1024`, wrap with `& 1023`). Noise and random are not
/// implemented and give 0 (so a wave yields its base).
pub fn table(func: Func, x: f32) -> f32 {
    let i = (x * 1024.0) as i32 & 1023;
    sin_or(func, i)
}

fn sin_or(func: Func, i: i32) -> f32 {
    let f = i as f32;
    match func {
        Func::Sin => (f * 360.0 / 1023.0).to_radians().sin(),
        Func::Square => {
            if i < 512 {
                1.0
            } else {
                -1.0
            }
        }
        Func::Triangle => {
            let j = i & 511;
            let t = if j < 256 {
                j as f32 / 256.0
            } else {
                1.0 - (j - 256) as f32 / 256.0
            };
            if i < 512 { t } else { -t }
        }
        Func::Sawtooth => f / 1024.0,
        Func::InverseSawtooth => 1.0 - f / 1024.0,
        Func::Noise | Func::Random => 0.0,
    }
}

/// The sine table entry `index & 1023`.
pub fn sin_table(index: i32) -> f32 {
    sin_or(Func::Sin, index & 1023)
}

/// `EvalWaveForm` with an extra phase offset.
pub fn wave(w: &Wave, time: f32, phase_offset: f32) -> f32 {
    w.base + table(w.func, w.phase + phase_offset + time * w.frequency) * w.amplitude
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(a: Vec3) -> Vec3 {
    let l = dot(a, a).sqrt();
    if l == 0.0 { a } else { scale(a, 1.0 / l) }
}

/// Applies `deforms` in order to a batch. `time` is the shader time in seconds and
/// `refdef_time_ms` the frame time in milliseconds (bulge uses that one).
pub fn apply(
    deforms: &[Deform],
    verts: &mut Vec<DeformVertex>,
    indexes: &mut Vec<u32>,
    view: &View,
    time: f32,
    refdef_time_ms: f32,
) {
    for d in deforms {
        match *d {
            Deform::Wave { spread, wave: w } => {
                if w.frequency == 0.0 {
                    let s = wave(&w, time, 0.0);
                    for v in verts.iter_mut() {
                        v.xyz = add(v.xyz, scale(v.normal, s));
                    }
                } else {
                    for v in verts.iter_mut() {
                        let off = (v.xyz[0] + v.xyz[1] + v.xyz[2]) * spread;
                        let s = wave(&w, time, off);
                        v.xyz = add(v.xyz, scale(v.normal, s));
                    }
                }
            }
            Deform::Bulge {
                width,
                height,
                speed,
            } => {
                if speed == 0.0 && width == 0.0 {
                    for v in verts.iter_mut() {
                        v.xyz = add(v.xyz, scale(v.normal, height));
                    }
                } else {
                    let now = refdef_time_ms * speed * 0.001;
                    for v in verts.iter_mut() {
                        let off = ((1024.0 / (std::f32::consts::PI * 2.0))
                            * (v.st[0] * width + now)) as i32;
                        let s = sin_table(off) * height;
                        v.xyz = add(v.xyz, scale(v.normal, s));
                    }
                }
            }
            Deform::Move { vector, wave: w } => {
                let offset = scale(vector, wave(&w, time, 0.0));
                for v in verts.iter_mut() {
                    v.xyz = add(v.xyz, offset);
                }
            }
            Deform::AutoSprite => autosprite(verts, indexes, view),
            Deform::AutoSprite2 => autosprite2(verts, indexes, view),
            Deform::None | Deform::Normals { .. } | Deform::ProjectionShadow | Deform::Text(_) => {}
        }
    }
}

/// `AutospriteDeform`: every 4 vertices are a quad, rebuilt around its centre to face
/// the viewer (`RB_AddQuadStamp`: texture and lightmap coordinates (0,0)-(1,1), the
/// quad's first colour, a normal towards the viewer).
fn autosprite(verts: &mut Vec<DeformVertex>, indexes: &mut Vec<u32>, view: &View) {
    let old = std::mem::take(verts);
    indexes.clear();
    for quad in old.as_chunks::<4>().0 {
        let mid = scale(
            add(add(quad[0].xyz, quad[1].xyz), add(quad[2].xyz, quad[3].xyz)),
            0.25,
        );
        let radius = dot(sub(quad[0].xyz, mid), sub(quad[0].xyz, mid)).sqrt() * 0.707;
        let (left, up) = (scale(view.left, radius), scale(view.up, radius));
        let normal = scale(view.forward, -1.0);
        let n = verts.len() as u32;
        let corners = [
            (add(add(mid, left), up), [0.0, 0.0]),
            (add(sub(mid, left), up), [1.0, 0.0]),
            (sub(sub(mid, left), up), [1.0, 1.0]),
            (sub(add(mid, left), up), [0.0, 1.0]),
        ];
        for (xyz, st) in corners {
            verts.push(DeformVertex {
                xyz,
                normal,
                st,
                lightmap: [
                    st,
                    quad[0].lightmap[1],
                    quad[0].lightmap[2],
                    quad[0].lightmap[3],
                ],
                color: quad[0].color,
                styled: quad[0].styled,
            });
        }
        indexes.extend([n, n + 1, n + 3, n + 3, n + 1, n + 2]);
    }
}

/// `Autosprite2Deform`: each quad pivots around the centre line of its long axis to
/// face the viewer as much as it can.
fn autosprite2(verts: &mut [DeformVertex], indexes: &[u32], view: &View) {
    const EDGES: [[usize; 2]; 6] = [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]];
    for (q, quad_start) in (0..verts.len()).step_by(4).enumerate() {
        if quad_start + 4 > verts.len() {
            break;
        }
        let xyz = |k: usize, verts: &[DeformVertex]| verts[quad_start + k].xyz;
        // The two shortest edges.
        let (mut nums, mut lengths) = ([0usize; 2], [999999.0f32; 2]);
        for (j, e) in EDGES.iter().enumerate() {
            let d = sub(xyz(e[0], verts), xyz(e[1], verts));
            let l = dot(d, d);
            if l < lengths[0] {
                nums[1] = nums[0];
                lengths[1] = lengths[0];
                nums[0] = j;
                lengths[0] = l;
            } else if l < lengths[1] {
                nums[1] = j;
                lengths[1] = l;
            }
        }
        let mid = nums.map(|j| scale(add(xyz(EDGES[j][0], verts), xyz(EDGES[j][1], verts)), 0.5));
        let major = sub(mid[1], mid[0]);
        let minor = normalize(cross(major, view.forward));
        for j in 0..2 {
            let [a, b] = EDGES[nums[j]];
            let l = 0.5 * lengths[j].sqrt();
            // Which way the edge runs in this quad's indexes decides the direction.
            let first = q * 6;
            let forward_edge = (0..5).any(|k| {
                indexes.get(first + k) == Some(&((quad_start + a) as u32))
                    && indexes.get(first + k + 1) == Some(&((quad_start + b) as u32))
            });
            let (pa, pb) = if forward_edge {
                (add(mid[j], scale(minor, -l)), add(mid[j], scale(minor, l)))
            } else {
                (add(mid[j], scale(minor, l)), add(mid[j], scale(minor, -l)))
            };
            verts[quad_start + a].xyz = pa;
            verts[quad_start + b].xyz = pb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vert(xyz: Vec3) -> DeformVertex {
        DeformVertex {
            xyz,
            normal: [0.0, 0.0, 1.0],
            st: [0.5, 0.5],
            lightmap: [[0.0; 2]; 4],
            color: [9, 8, 7, 6],
            styled: [[0; 4]; 3],
        }
    }

    #[test]
    fn tables_match_the_originals() {
        assert_eq!(table(Func::Square, 0.25), 1.0);
        assert_eq!(table(Func::Square, 0.75), -1.0);
        assert_eq!(table(Func::Sawtooth, 0.5), 0.5);
        assert_eq!(table(Func::Triangle, 0.25), 1.0);
        assert_eq!(table(Func::Triangle, 0.75), -1.0);
        // Sine over 1023, not 1024: index 256 is a little past 90 degrees.
        assert!(
            (table(Func::Sin, 0.25) - (256.0f32 * 360.0 / 1023.0).to_radians().sin()).abs() < 1e-6
        );
        // Negative arguments wrap like the original's int & 1023.
        assert_eq!(table(Func::Sawtooth, -0.25), 0.75);
    }

    #[test]
    fn autosprite_faces_the_viewer() {
        // A quad lying flat on the floor, seen by a viewer looking along +x.
        let mut verts: Vec<_> = [
            [-8.0, -8.0, 0.0],
            [8.0, -8.0, 0.0],
            [8.0, 8.0, 0.0],
            [-8.0, 8.0, 0.0],
        ]
        .map(vert)
        .to_vec();
        let mut idx = vec![0, 1, 2, 0, 2, 3];
        let view = View {
            forward: [1.0, 0.0, 0.0],
            left: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
        };
        apply(&[Deform::AutoSprite], &mut verts, &mut idx, &view, 0.0, 0.0);
        let r = (8.0f32 * 8.0 * 2.0).sqrt() * 0.707;
        // Now upright in the y-z plane, centred on the old centre.
        assert!(verts.iter().all(|v| v.xyz[0] == 0.0));
        assert!((verts[0].xyz[1] - r).abs() < 1e-4 && (verts[0].xyz[2] - r).abs() < 1e-4);
        assert_eq!(verts[2].st, [1.0, 1.0]);
        assert_eq!(verts[1].color, [9, 8, 7, 6]);
        assert_eq!(idx, [0, 1, 3, 3, 1, 2]);
    }

    #[test]
    fn wave_and_move_deforms_offset_vertices() {
        let w = Wave {
            func: Func::Square,
            base: 2.0,
            amplitude: 1.0,
            phase: 0.0,
            frequency: 0.0,
        };
        let mut verts = vec![vert([0.0; 3])];
        let mut idx = vec![];
        let view = View {
            forward: [1.0, 0.0, 0.0],
            left: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
        };
        // Frequency 0: base + table(phase) * amplitude = 2 + 1 = 3 along the normal.
        apply(
            &[Deform::Wave {
                spread: 0.0,
                wave: w,
            }],
            &mut verts,
            &mut idx,
            &view,
            5.0,
            0.0,
        );
        assert_eq!(verts[0].xyz, [0.0, 0.0, 3.0]);
        apply(
            &[Deform::Move {
                vector: [1.0, 0.0, 0.0],
                wave: w,
            }],
            &mut verts,
            &mut idx,
            &view,
            0.0,
            0.0,
        );
        assert_eq!(verts[0].xyz, [3.0, 0.0, 3.0]);
    }
}
