//! Which world surfaces a view draws, as the original decides it (OpenJK
//! `rd-vanilla/tr_world.cpp`): `R_MarkLeaves` (the view cluster's PVS),
//! `R_RecursiveWorldNode` (node and leaf boxes against the view frustum, front child
//! first) and `R_CullSurface` (faces by their plane with an 8-unit margin, patches and
//! triangle soups by their bounds). Every area counts as open: area portals (doors)
//! are game state.

use swjkja_bsp::Bsp;

use crate::camera::Camera;

pub type Vec3 = [f32; 3];

/// How a surface can be culled once its leaf is visible.
#[derive(Debug, Clone, Copy)]
pub enum SurfaceCull {
    /// A face: its plane, and whether the material is front-sided (else back-sided);
    /// two-sided faces use `Never`.
    Plane {
        normal: Vec3,
        dist: f32,
        front_sided: bool,
    },
    /// Patches and triangle soups: their bounds against the frustum.
    Bounds {
        mins: Vec3,
        maxs: Vec3,
    },
    Never,
}

#[derive(Debug, Clone, Copy)]
struct Plane {
    normal: Vec3,
    dist: f32,
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `BoxOnPlaneSide`: 1 if the box is entirely in front, 2 entirely behind, 3 both.
fn box_on_plane_side(mins: Vec3, maxs: Vec3, p: &Plane) -> u8 {
    let (mut front, mut back) = (0.0f32, 0.0f32);
    for i in 0..3 {
        let (a, b) = (p.normal[i] * maxs[i], p.normal[i] * mins[i]);
        front += a.max(b);
        back += a.min(b);
    }
    let mut sides = 0;
    if front >= p.dist {
        sides = 1;
    }
    if back < p.dist {
        sides |= 2;
    }
    sides
}

/// The world's BSP tree and what visibility needs per surface.
pub struct WorldVis {
    nodes: Vec<swjkja_bsp::Node>,
    leafs: Vec<swjkja_bsp::Leaf>,
    planes: Vec<Plane>,
    leaf_surfaces: Vec<i32>,
    vis: swjkja_bsp::Visibility,
    /// Parent of each node (for nodes) and leaf, -1 for the root.
    node_parent: Vec<i32>,
    leaf_parent: Vec<i32>,
    pub surface_cull: Vec<SurfaceCull>,
    // Per-frame marks.
    node_mark: Vec<u32>,
    surface_mark: Vec<u32>,
    frame: u32,
}

impl WorldVis {
    pub fn new(bsp: &Bsp, surface_cull: Vec<SurfaceCull>) -> Self {
        let mut node_parent = vec![-1; bsp.nodes.len()];
        let mut leaf_parent = vec![-1; bsp.leafs.len()];
        for (i, n) in bsp.nodes.iter().enumerate() {
            for c in n.children {
                if c >= 0 {
                    node_parent[c as usize] = i as i32;
                } else {
                    leaf_parent[(-(c + 1)) as usize] = i as i32;
                }
            }
        }
        Self {
            nodes: bsp.nodes.clone(),
            leafs: bsp.leafs.clone(),
            planes: bsp
                .planes
                .iter()
                .map(|p| Plane {
                    normal: p.normal,
                    dist: p.dist,
                })
                .collect(),
            leaf_surfaces: bsp.leaf_surfaces.clone(),
            vis: bsp.visibility.clone(),
            node_mark: vec![0; bsp.nodes.len()],
            surface_mark: vec![0; surface_cull.len()],
            node_parent,
            leaf_parent,
            surface_cull,
            frame: 0,
        }
    }

    /// `R_PointInLeaf`: the leaf containing `p`.
    fn point_in_leaf(&self, p: Vec3) -> usize {
        let mut node = 0i32;
        loop {
            let n = &self.nodes[node as usize];
            let plane = &self.planes[n.plane as usize];
            let child = if dot(p, plane.normal) - plane.dist > 0.0 {
                n.children[0]
            } else {
                n.children[1]
            };
            if child < 0 {
                return (-(child + 1)) as usize;
            }
            node = child;
        }
    }

    /// The surfaces the original draws for this view, in the order it adds them.
    pub fn visible(&mut self, camera: &Camera, out: &mut Vec<usize>) {
        out.clear();
        if self.nodes.is_empty() {
            return;
        }
        self.frame = self.frame.wrapping_add(1).max(1);
        let frame = self.frame;
        // R_MarkLeaves: leaves in the view cluster's PVS, and their ancestors.
        let view_cluster = self.leafs[self.point_in_leaf(camera.origin)].cluster;
        // Leaf marks share node_mark's numbering: leaf i is marked through its parent
        // chain; a separate leaf mark keeps the leaf test itself.
        let mut leaf_marked = vec![false; self.leafs.len()];
        for (i, leaf) in self.leafs.iter().enumerate() {
            let visible = view_cluster < 0
                || (leaf.cluster >= 0 && self.vis.can_see(view_cluster, leaf.cluster));
            if !visible || (view_cluster >= 0 && leaf.cluster < 0) {
                continue;
            }
            leaf_marked[i] = true;
            let mut parent = self.leaf_parent[i];
            while parent >= 0 && self.node_mark[parent as usize] != frame {
                self.node_mark[parent as usize] = frame;
                parent = self.node_parent[parent as usize];
            }
        }

        // R_SetupFrustum: four side planes from the field of view.
        let [f, r, u] = camera.axes();
        let left = [-r[0], -r[1], -r[2]];
        let (tx, ty) = camera.tan_half_fov();
        let side = |ang: f32, axis: Vec3, sign: f32| {
            let (xs, xc) = ang.sin_cos();
            let normal = [0, 1, 2].map(|k| f[k] * xs + sign * xc * axis[k]);
            Plane {
                normal,
                dist: dot(camera.origin, normal),
            }
        };
        let (ax, ay) = (tx.atan(), ty.atan());
        let frustum = [
            side(ax, left, 1.0),
            side(ax, left, -1.0),
            side(ay, u, 1.0),
            side(ay, u, -1.0),
        ];

        // R_RecursiveWorldNode, front child first.
        let mut stack: Vec<(i32, u8)> = vec![(0, 0b1111)];
        while let Some((node, mut bits)) = stack.pop() {
            let (mins, maxs, is_leaf) = if node >= 0 {
                if self.node_mark[node as usize] != frame {
                    continue;
                }
                let n = &self.nodes[node as usize];
                (n.mins.map(|v| v as f32), n.maxs.map(|v| v as f32), false)
            } else {
                let leaf = (-(node + 1)) as usize;
                if !leaf_marked[leaf] {
                    continue;
                }
                let l = &self.leafs[leaf];
                (l.mins.map(|v| v as f32), l.maxs.map(|v| v as f32), true)
            };
            let mut culled = false;
            for (i, plane) in frustum.iter().enumerate() {
                if bits & (1 << i) != 0 {
                    match box_on_plane_side(mins, maxs, plane) {
                        2 => {
                            culled = true;
                            break;
                        }
                        1 => bits &= !(1 << i),
                        _ => {}
                    }
                }
            }
            if culled {
                continue;
            }
            if !is_leaf {
                let n = &self.nodes[node as usize];
                // Pushed in reverse so children[0] comes off the stack first.
                stack.push((n.children[1], bits));
                stack.push((n.children[0], bits));
                continue;
            }
            let leaf = &self.leafs[(-(node + 1)) as usize];
            let first = leaf.first_leaf_surface as usize;
            for &s in &self.leaf_surfaces[first..first + leaf.num_leaf_surfaces as usize] {
                let s = s as usize;
                if self.surface_mark[s] == frame {
                    continue;
                }
                self.surface_mark[s] = frame;
                if !self.culled(s, camera.origin, &frustum) {
                    out.push(s);
                }
            }
        }
    }

    /// `R_CullSurface`.
    fn culled(&self, s: usize, origin: Vec3, frustum: &[Plane; 4]) -> bool {
        match self.surface_cull[s] {
            SurfaceCull::Plane {
                normal,
                dist,
                front_sided,
            } => {
                let d = dot(origin, normal);
                if front_sided {
                    d < dist - 8.0
                } else {
                    d > dist + 8.0
                }
            }
            SurfaceCull::Bounds { mins, maxs } => frustum
                .iter()
                .any(|p| box_on_plane_side(mins, maxs, p) == 2),
            SurfaceCull::Never => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_sides() {
        let p = Plane {
            normal: [1.0, 0.0, 0.0],
            dist: 10.0,
        };
        assert_eq!(box_on_plane_side([11.0; 3], [20.0; 3], &p), 1);
        assert_eq!(box_on_plane_side([0.0; 3], [9.0; 3], &p), 2);
        assert_eq!(box_on_plane_side([0.0; 3], [20.0; 3], &p), 3);
    }
}
