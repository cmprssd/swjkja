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

/// The collision model's `BoxOnPlaneSide`, which takes a fast path for planes
/// whose normal is exactly +x, +y or +z (`PlaneTypeForNormal`): there a box touching
/// the plane from behind counts as behind only.
fn box_on_plane_side_typed(mins: Vec3, maxs: Vec3, p: &Plane) -> u8 {
    match p.normal.iter().position(|&n| n == 1.0) {
        Some(t) if p.dist <= mins[t] => 1,
        Some(t) if p.dist >= maxs[t] => 2,
        Some(_) => 3,
        None => box_on_plane_side(mins, maxs, p),
    }
}

/// `MAX_TOTAL_ENT_LEAFS` and `MAX_ENT_CLUSTERS`: how many leafs `SV_LinkEntity`
/// gathers for an entity's box, and how many of their clusters it keeps.
const MAX_ENT_LEAFS: usize = 128;
const MAX_ENT_CLUSTERS: usize = 16;

/// Where the server links an entity (`SV_LinkEntity`), for deciding whether a view
/// is sent it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntityLink {
    /// The clusters of the first leafs the box touches (up to 16).
    pub clusters: Vec<i32>,
    /// When the box touches more clusters than are kept: the cluster of the last
    /// leaf with one, else 0.
    pub last_cluster: i32,
    /// Whether one of the leafs is in an area (not solid).
    pub in_area: bool,
}

/// The view's four side planes (`R_SetupFrustum`; cgame's `CG_SetupFrustum` builds
/// the same ones).
#[derive(Debug, Clone, Copy)]
pub struct Frustum {
    planes: [Plane; 4],
}

impl Frustum {
    pub fn new(camera: &Camera) -> Self {
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
        Self {
            planes: [
                side(ax, left, 1.0),
                side(ax, left, -1.0),
                side(ay, u, 1.0),
                side(ay, u, -1.0),
            ],
        }
    }

    /// The frustum in the space of an entity whose `axes` (forward, left, up) and
    /// `origin` place it in the world.
    pub fn local(&self, axes: [Vec3; 3], origin: Vec3) -> Self {
        Self {
            planes: self.planes.map(|p| Plane {
                normal: axes.map(|a| dot(a, p.normal)),
                dist: p.dist - dot(p.normal, origin),
            }),
        }
    }

    /// `CG_CullPointAndRadius`: the sphere is entirely behind a plane.
    pub fn culls_sphere(&self, center: Vec3, radius: f32) -> bool {
        self.planes
            .iter()
            .any(|p| dot(center, p.normal) - p.dist < -radius)
    }

    /// `R_CullLocalBox`'s CULL_OUT: every corner is on or behind one plane.
    pub fn culls_points(&self, points: &[Vec3]) -> bool {
        self.planes
            .iter()
            .any(|p| points.iter().all(|&v| dot(v, p.normal) <= p.dist))
    }
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

    /// The cluster of the leaf containing `p` (-1 outside the map, or without nodes).
    pub fn cluster(&self, p: Vec3) -> i32 {
        if self.nodes.is_empty() {
            return -1;
        }
        self.leafs[self.point_in_leaf(p)].cluster
    }

    /// `R_inPVS` for two points' clusters: whether `to` is in the potentially
    /// visible set of `from` (a point outside the map sees, and is seen by,
    /// everything).
    pub fn can_see(&self, from: i32, to: i32) -> bool {
        self.vis.can_see(from, to)
    }

    /// `CM_BoxLeafnums`: the leafs the box touches (at most 128), front child first,
    /// and the last of them with a cluster, overflowed or not.
    fn box_leafs(&self, mins: Vec3, maxs: Vec3) -> (Vec<usize>, usize) {
        let mut leafs = Vec::new();
        let mut last = 0;
        if self.nodes.is_empty() {
            return (leafs, last);
        }
        let mut stack = vec![0i32];
        while let Some(mut node) = stack.pop() {
            loop {
                if node < 0 {
                    let leaf = (-(node + 1)) as usize;
                    if self.leafs[leaf].cluster != -1 {
                        last = leaf;
                    }
                    if leafs.len() < MAX_ENT_LEAFS {
                        leafs.push(leaf);
                    }
                    break;
                }
                let n = &self.nodes[node as usize];
                node = match box_on_plane_side_typed(mins, maxs, &self.planes[n.plane as usize]) {
                    1 => n.children[0],
                    2 => n.children[1],
                    _ => {
                        stack.push(n.children[1]);
                        n.children[0]
                    }
                };
            }
        }
        (leafs, last)
    }

    /// `SV_LinkEntity`'s clusters and area for an entity's box (`absmin`, `absmax`).
    pub fn link(&self, [mins, maxs]: [Vec3; 2]) -> EntityLink {
        let (leafs, last_leaf) = self.box_leafs(mins, maxs);
        let mut link = EntityLink {
            in_area: leafs.iter().any(|&l| self.leafs[l].area != -1),
            ..EntityLink::default()
        };
        let mut i = 0;
        while i < leafs.len() {
            let cluster = self.leafs[leafs[i]].cluster;
            if cluster != -1 {
                link.clusters.push(cluster);
                if link.clusters.len() == MAX_ENT_CLUSTERS {
                    break;
                }
            }
            i += 1;
        }
        if i != leafs.len() {
            link.last_cluster = self.leafs[last_leaf].cluster;
        }
        link
    }

    /// `SV_AddEntitiesToPacket`: whether the server sends a linked entity to a view in
    /// `view_cluster`. Every area counts as open, so a view in solid (area -1) is sent
    /// nothing and any other is sent what is in an area. Past the kept clusters, the
    /// original scans the cluster numbers up to the last one with a slip: it sends the
    /// entity unless the first visible cluster it finds is the last one.
    pub fn sends(&self, view_cluster: i32, link: &EntityLink) -> bool {
        if view_cluster < 0 || !link.in_area || link.clusters.is_empty() {
            return false;
        }
        if link.clusters.iter().any(|&c| self.can_see(view_cluster, c)) {
            return true;
        }
        if link.last_cluster == 0 {
            return false;
        }
        let from = *link.clusters.last().unwrap_or(&0);
        let first_seen = (from..=link.last_cluster).find(|&c| self.can_see(view_cluster, c));
        first_seen != Some(link.last_cluster)
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

        let frustum = Frustum::new(camera).planes;

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

    /// `R_CullSurface` for a surface drawn outside the world's leaves (a brush
    /// entity's, placed in the world).
    pub fn culls_surface(&self, s: usize, origin: Vec3, frustum: &Frustum) -> bool {
        self.culled(s, origin, &frustum.planes)
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

    #[test]
    fn a_local_frustum_culls_an_entity_box_as_the_world_one_culls_its_corners() {
        // Looking down +x from the origin; an entity at (500, 0, 0) turned 90 degrees
        // in yaw (its forward is the world's +y).
        let frustum = Frustum::new(&Camera::new([0.0; 3], [0.0; 3], 1280, 720));
        let (axes, origin) = (
            [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [500.0, 0.0, 0.0],
        );
        let local = frustum.local(axes, origin);
        let place =
            |v: Vec3| [0, 1, 2].map(|k| origin[k] + (0..3).map(|i| v[i] * axes[i][k]).sum::<f32>());
        let culled = |f: &Frustum, mins: Vec3, maxs: Vec3| {
            f.planes
                .iter()
                .any(|p| box_on_plane_side(mins, maxs, p) == 2)
        };
        // At the entity, in view; 2000 units along the model's -y (the world's +x,
        // farther ahead), in view; 2000 along the model's +x (the world's +y, far to
        // the left), out of it.
        for (mins, maxs, out) in [
            ([-8.0; 3], [8.0; 3], false),
            ([-8.0, -2008.0, -8.0], [8.0, -1992.0, 8.0], false),
            ([1992.0, -8.0, -8.0], [2008.0, 8.0, 8.0], true),
        ] {
            let corners: [Vec3; 8] = std::array::from_fn(|i| {
                place([
                    if i & 1 == 0 { mins[0] } else { maxs[0] },
                    if i & 2 == 0 { mins[1] } else { maxs[1] },
                    if i & 4 == 0 { mins[2] } else { maxs[2] },
                ])
            });
            assert_eq!(culled(&local, mins, maxs), out);
            assert_eq!(frustum.culls_points(&corners), out);
        }
    }

    /// A map split at x = 0 (an axial plane), its front half split again at x + y =
    /// 100 (not axial): leaf 0 (x > 0, x + y > 100) is cluster 0, leaf 1 (x > 0, x +
    /// y <= 100) cluster 1, leaf 2 (x <= 0) solid. Each cluster sees only itself.
    fn split_map(clusters: usize) -> WorldVis {
        let node = |plane, children| swjkja_bsp::Node {
            plane,
            children,
            mins: [0; 3],
            maxs: [0; 3],
        };
        let leaf = |cluster, area| swjkja_bsp::Leaf {
            cluster,
            area,
            mins: [0; 3],
            maxs: [0; 3],
            first_leaf_surface: 0,
            num_leaf_surfaces: 0,
            first_leaf_brush: 0,
            num_leaf_brushes: 0,
        };
        let bytes_per_cluster = clusters.div_ceil(8);
        let mut bits = vec![0u8; clusters * bytes_per_cluster];
        for c in 0..clusters {
            bits[c * bytes_per_cluster + c / 8] |= 1 << (c % 8);
        }
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        WorldVis {
            nodes: vec![node(0, [1, -3]), node(1, [-1, -2])],
            leafs: vec![leaf(0, 0), leaf(1, 0), leaf(-1, -1)],
            planes: vec![
                Plane {
                    normal: [1.0, 0.0, 0.0],
                    dist: 0.0,
                },
                Plane {
                    normal: [diagonal, diagonal, 0.0],
                    dist: 100.0 * diagonal,
                },
            ],
            leaf_surfaces: Vec::new(),
            vis: swjkja_bsp::Visibility {
                clusters,
                bytes_per_cluster,
                bits,
            },
            node_parent: Vec::new(),
            leaf_parent: Vec::new(),
            surface_cull: Vec::new(),
            node_mark: Vec::new(),
            surface_mark: Vec::new(),
            frame: 0,
        }
    }

    #[test]
    fn an_entity_links_to_the_clusters_its_box_touches() {
        let vis = split_map(2);
        let in_cluster_0 = vis.link([[100.0; 3], [120.0; 3]]);
        assert_eq!(in_cluster_0.clusters, [0]);
        assert!(vis.sends(0, &in_cluster_0));
        assert!(!vis.sends(1, &in_cluster_0));
        // A view in solid is sent nothing.
        assert!(!vis.sends(-1, &in_cluster_0));
        // Across both front leafs and the solid one: front children first.
        let across = vis.link([[-10.0, 40.0, 0.0], [70.0, 50.0, 0.0]]);
        assert_eq!(across.clusters, [0, 1]);
        assert!(across.in_area);
        assert!(vis.sends(1, &across));
        // Touching the axial plane from behind counts as behind only.
        let behind = vis.link([[-20.0; 3], [0.0, 0.0, 0.0]]);
        assert_eq!(behind, EntityLink::default());
        assert!(!vis.sends(0, &behind));
        // The diagonal plane is not axial: a box touching it from behind is on both
        // sides.
        let touching = vis.link([[10.0, 10.0, 0.0], [50.0, 50.0, 0.0]]);
        assert_eq!(touching.clusters, [0, 1]);
    }

    #[test]
    fn past_sixteen_clusters_the_server_slips() {
        let vis = split_map(40);
        let link = |last_cluster| EntityLink {
            clusters: (0..16).collect(),
            last_cluster,
            in_area: true,
        };
        // Cluster 20 sees only itself: none of 0..16 is visible from it.
        assert!(!vis.sends(
            20,
            &EntityLink {
                last_cluster: 0,
                ..link(0)
            }
        ));
        // The scan from cluster 15 finds 20 before the last cluster, 30: sent.
        assert!(vis.sends(20, &link(30)));
        // It finds nothing up to 18: sent all the same.
        assert!(vis.sends(20, &link(18)));
        // The first visible cluster is the last one: not sent.
        assert!(!vis.sends(20, &link(20)));
    }
}
