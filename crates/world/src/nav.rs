//! The navigation graph single-player NPCs walk along when a goal is not in reach
//! (`code/game/g_navigator.cpp`, `NAV::`): its points (`waypoint`,
//! `waypoint_small`, `waypoint_navgoal`, `point_combat`), the edges designers place
//! and those combat and goal points make to their nearest neighbors, tested by box
//! traces as the level loads (`LoadFromEntitiesAndSaveToFile`), the nearest point
//! to a position (`GetNearestNode`) and the path from it (`FindPath`).
//!
//! Left out: doors, walls and breakables along an edge (each edge is tested against
//! the map's brushes alone, as if they were not there), danger, the area portals of
//! the PVS test, and points in solid (`G_CheckInSolid`).
use std::collections::HashMap;

use swjkja_bsp::{Bsp, Entity};

use crate::Vec3;
use crate::collision::{
    CONTENTS_BOTCLIP, CONTENTS_MONSTERCLIP, CONTENTS_PLAYERCLIP, CONTENTS_SOLID, CONTENTS_TERRAIN,
    Collision, MASK_NPCSOLID,
};
use crate::static_model::{atof, spawns_at, vector_prefix};
use crate::style::atoi;

/// `CELL_RANGE`, `VIEW_RANGE`, `NUM_CELLS`, `NUM_NODES_PER_CELL`.
const CELL_RANGE: f32 = 1000.0;
const VIEW_RANGE: f32 = 550.0;
const CELLS: usize = 32;
const NODES_PER_CELL: usize = 60;
/// `MAX_EDGE_FLOOR_DIST`, `MAX_EDGE_AUTO_LEN`, `MIN_WAY_NEIGHBORS`,
/// `MAX_NONWP_NEIGHBORS`, `BIAS_NONWAYPOINT`, `Z_CULL_OFFSET`.
const FLOOR_DIST: f32 = 60.0;
const AUTO_LEN: f32 = 500.0;
const WAY_NEIGHBORS: usize = 4;
const NONWP_NEIGHBORS: i32 = 1;
const BIAS_NONWAYPOINT: f32 = 500.0;
pub const Z_CULL_OFFSET: f32 = 60.0;
/// The two sizes an edge is tested at (`SC_LARGE_*`, `SC_MEDIUM_*`).
const LARGE: (f32, f32) = (60.0, 120.0);
const MEDIUM: (f32, f32) = (20.0, 60.0);
/// `SAFE_NEIGHBORINGPOINT_DIST`.
const SAFE_NEIGHBOR_DIST: f32 = 400.0;
/// `MAX_RADIUS_CHECK`, `YAW_ITERATIONS`, `DEFAULT_MAXS_0`, `DEFAULT_MINS_2`,
/// `DEFAULT_MAXS_2`, `CROUCH_MAXS_2`, `STEPSIZE`.
const RADIUS_CHECK: f32 = 1024.0;
const YAW_ITERATIONS: usize = 16;
const DEFAULT_MAXS_0: f32 = 16.0;
const DEFAULT_MINS_2: f32 = -24.0;
const DEFAULT_MAXS_2: f32 = 40.0;
const CROUCH_MAXS_2: f32 = 16.0;
const STEPSIZE: f32 = 18.0;

/// What a point is (`EPointType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Way,
    Goal,
    Combat,
}

/// A point of the graph (`CWayNode`).
#[derive(Debug, Clone)]
pub struct Node {
    pub point: Vec3,
    pub radius: f32,
    pub kind: Kind,
    pub name: Option<String>,
    /// `WN_ISLAND`: no edge left once tested; `WN_FLOATING`: not on the floor.
    pub island: bool,
    pub floating: bool,
    drop: bool,
    no_auto: bool,
    /// target, target2, target3, target4, targetJump.
    targets: [Option<String>; 5],
}

/// An edge between two points (`CWayEdge`).
#[derive(Debug, Clone)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub distance: f32,
    /// `WE_SIZE_LARGE`, else medium.
    pub large: bool,
    pub flying: bool,
    pub jumping: bool,
}

/// The cells points are found by (`TGraphCells`): a grid over the points' bounds,
/// each cell listing the [`NODES_PER_CELL`] nearest its center within range.
#[derive(Debug, Clone, Default)]
struct Cells {
    mins: [f32; 2],
    maxs: [f32; 2],
    scale: [f32; 2],
    nodes: Vec<Vec<usize>>,
}

impl Cells {
    /// `grid2_vs::get_cell_coords`.
    fn coords(&self, x: f32, y: f32) -> (usize, usize) {
        let x = x.min(self.maxs[0] - 1.0).max(self.mins[0]);
        let y = y.min(self.maxs[1] - 1.0).max(self.mins[1]);
        (
            ((x - self.mins[0]) / self.scale[0]) as usize,
            ((y - self.mins[1]) / self.scale[1]) as usize,
        )
    }

    fn get(&self, p: Vec3) -> &[usize] {
        let (x, y) = self.coords(p[0], p[1]);
        &self.nodes[x + y * CELLS]
    }
}

/// The graph, built as the level loads.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Each node's neighbors and the edge to each.
    links: Vec<Vec<(usize, usize)>>,
    cells: Cells,
    /// Each node's region: those an edge path joins share one, islands another.
    regions: Vec<usize>,
}

/// The nearest point to a position: a node, or an edge it stands beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nearest {
    Node(usize),
    Edge(usize),
}

/// A point of a path (`SPathPoint`), the last to reach first.
#[derive(Debug, Clone, PartialEq)]
pub struct PathPoint {
    pub node: usize,
    pub point: Vec3,
    /// Within it, the NPC slows as it nears.
    pub slowing_radius: f32,
    /// Squared.
    pub reached_radius: f32,
    /// When the NPC should have reached it (`mETA`, level time in ms): a path
    /// whose next point is overdue is found again.
    pub eta: f32,
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn dist2(a: Vec3, b: Vec3) -> f32 {
    let d = sub(a, b);
    dot(d, d)
}

fn dist(a: Vec3, b: Vec3) -> f32 {
    dist2(a, b).sqrt()
}

/// `CVec3::ProjectToLine`: `p` moved onto the line through `a` and `b`, and how far
/// along it lies (0 at `a`, 1 at `b`).
fn project_to_line(p: Vec3, a: Vec3, b: Vec3) -> (Vec3, f32) {
    let ab = sub(b, a);
    let scale = dot(sub(p, a), ab) / dot(ab, ab);
    ([0, 1, 2].map(|k| a[k] + ab[k] * scale), scale)
}

/// `ViewTrace` (with botclip, `ViewNavTrace`): whether a line is clear.
fn view_trace(world: &Collision, a: Vec3, b: Vec3, botclip: bool) -> (bool, Vec3) {
    let mut mask = CONTENTS_SOLID | CONTENTS_TERRAIN | CONTENTS_MONSTERCLIP;
    if botclip {
        mask |= CONTENTS_BOTCLIP;
    }
    let tr = world.trace(a, b, [0.0; 3], [0.0; 3], mask);
    (
        !tr.all_solid && !tr.start_solid && tr.fraction == 1.0,
        tr.end,
    )
}

/// `MoveTrace` with `CheckForDoNotEnter` (botclip too), retried without botclip
/// when the box starts in it if `retry`.
pub fn move_trace(
    world: &Collision,
    a: Vec3,
    b: Vec3,
    mins: Vec3,
    maxs: Vec3,
    retry: bool,
) -> bool {
    let clear = |mask: i32| {
        let tr = world.trace(a, b, mins, maxs, mask);
        (
            !tr.all_solid && !tr.start_solid && tr.fraction == 1.0,
            tr.all_solid || tr.start_solid,
        )
    };
    let (ok, solid) = clear(MASK_NPCSOLID | CONTENTS_BOTCLIP);
    ok || retry && solid && clear(MASK_NPCSOLID).0
}

/// The cluster and area of the leaf holding `p` (`CM_PointLeafnum`).
fn leaf(bsp: &Bsp, p: Vec3) -> (i32, i32) {
    if bsp.nodes.is_empty() {
        return (-1, -1);
    }
    let mut node = 0i32;
    while node >= 0 {
        let n = &bsp.nodes[node as usize];
        let plane = &bsp.planes[n.plane as usize];
        node = if dot(p, plane.normal) - plane.dist < 0.0 {
            n.children[1]
        } else {
            n.children[0]
        };
    }
    let l = &bsp.leafs[(-(node + 1)) as usize];
    (l.cluster, l.area)
}

/// `SV_inPVS` as the level loads: `b`'s cluster in `a`'s potentially visible set,
/// in the same area (every area portal is shut until a door opens).
fn in_pvs(bsp: &Bsp, a: Vec3, b: Vec3) -> bool {
    let (ca, aa) = leaf(bsp, a);
    let (cb, ab) = leaf(bsp, b);
    bsp.visibility.can_see(ca, cb) && aa == ab
}

/// `waypoint_getRadius`: how far a waypoint's box (from a step up) goes in 16
/// directions, each no further than the shortest so far, plus its width.
fn waypoint_radius(world: &Collision, origin: Vec3) -> f32 {
    let mins = [-16.0, -16.0, -24.0 + STEPSIZE];
    let maxs = [16.0, 16.0, 40.0];
    let mut min = RADIUS_CHECK + 1.0;
    for i in 0..YAW_ITERATIONS {
        let yaw = ((360.0 / YAW_ITERATIONS as f32) * i as f32).to_radians();
        let end = [
            origin[0] + min * yaw.cos(),
            origin[1] + min * yaw.sin(),
            origin[2],
        ];
        let mask = CONTENTS_SOLID | CONTENTS_MONSTERCLIP | CONTENTS_BOTCLIP;
        let d = min * world.trace(origin, end, mins, maxs, mask).fraction;
        min = min.min(d);
    }
    min + DEFAULT_MAXS_0
}

impl Graph {
    /// The graph of map `bsp` under difficulty `skill`, its edges tested against
    /// `world` (the map's brushes).
    pub fn build(bsp: &Bsp, skill: u32, world: &Collision) -> Self {
        let mut g = Self::default();
        for e in bsp.entities.iter().filter(|e| spawns_at(e, skill)) {
            if let Some(node) = point(e, world) {
                g.nodes.push(node);
            }
        }
        g.links = vec![Vec::new(); g.nodes.len()];
        // LoadFromEntitiesAndSaveToFile does nothing for three points or fewer.
        if g.nodes.len() <= 3 {
            return g;
        }
        g.fill_cells();
        for n in &mut g.nodes {
            let roof = n.point;
            let mut floor = n.point;
            if n.drop {
                floor[2] -= FLOOR_DIST;
            }
            floor[2] -= FLOOR_DIST * 1.5;
            let (clear, end) = view_trace(world, roof, floor, false);
            if n.drop {
                n.point = end;
                n.point[2] += 5.0;
            } else if clear && matches!(n.kind, Kind::Way | Kind::Goal) {
                n.floating = true;
            }
        }
        let mut names: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, n) in g.nodes.iter().enumerate() {
            if let Some(name) = &n.name {
                names.entry(name.clone()).or_default().push(i);
            }
        }
        let mut jumping = Vec::new();
        for at in 0..g.nodes.len() {
            for t in 0..5 {
                let Some(name) = g.nodes[at].targets[t].take() else {
                    continue;
                };
                for &to in names.get(&name).map_or(&[][..], Vec::as_slice) {
                    if to == at {
                        continue;
                    }
                    let edge = match g.edge_across(at, to) {
                        Some(e) => e,
                        None => g.connect(at, to),
                    };
                    if t == 4 {
                        jumping.push(edge);
                    }
                }
            }
            if !g.nodes[at].no_auto && matches!(g.nodes[at].kind, Kind::Combat | Kind::Goal) {
                g.auto_connect(bsp, at);
            }
        }
        for e in jumping {
            g.edges[e].jumping = true;
        }
        let mut removed = Vec::new();
        for e in 0..g.edges.len() {
            if g.edges[e].jumping {
                g.edges[e].large = true;
                continue;
            }
            g.edges[e].large = true;
            if !g.test_edge(e, world) {
                g.edges[e].large = false;
                if !g.test_edge(e, world) {
                    removed.push(e);
                }
            }
        }
        g.remove_edges(&removed);
        for (n, links) in g.nodes.iter_mut().zip(&g.links) {
            n.island = links.is_empty();
        }
        g.find_regions();
        g
    }

    /// `fill_cells_nodes`: each cell lists, nearest its center first, the points
    /// of the cells within range of it (range in cells, from the larger scale).
    fn fill_cells(&mut self) {
        let mut c = Cells {
            mins: [f32::MAX; 2],
            maxs: [f32::MIN; 2],
            ..Cells::default()
        };
        for n in &self.nodes {
            for k in 0..2 {
                c.mins[k] = c.mins[k].min(n.point[k]);
                c.maxs[k] = c.maxs[k].max(n.point[k]);
            }
        }
        c.scale = [0, 1].map(|k| (c.maxs[k] - c.mins[k]) / CELLS as f32);
        let mut contained = vec![Vec::new(); CELLS * CELLS];
        for (i, n) in self.nodes.iter().enumerate() {
            let (x, y) = c.coords(n.point[0], n.point[1]);
            contained[x + y * CELLS].push(i);
        }
        let range = (CELL_RANGE / c.scale[0].max(c.scale[1])) as i32 + 1;
        c.nodes = vec![Vec::new(); CELLS * CELLS];
        for x in 0..CELLS as i32 {
            for y in 0..CELLS as i32 {
                let center = [
                    x as f32 * c.scale[0] + c.mins[0] + c.scale[0] * 0.5,
                    y as f32 * c.scale[1] + c.mins[1] + c.scale[1] * 0.5,
                ];
                let mut sorted = Vec::new();
                let span = |s: i32| (s - range).max(0)..=(s + range).min(CELLS as i32 - 1);
                for cy in span(y) {
                    for cx in span(x) {
                        for &i in &contained[(cx + cy * CELLS as i32) as usize] {
                            let p = self.nodes[i].point;
                            let d = [p[0] - center[0], p[1] - center[1]];
                            sorted.push((d[0] * d[0] + d[1] * d[1], i));
                        }
                    }
                }
                sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
                c.nodes[(x + y * CELLS as i32) as usize] = sorted
                    .iter()
                    .take(NODES_PER_CELL)
                    .map(|&(_, i)| i)
                    .collect();
            }
        }
        self.cells = c;
    }

    fn edge_across(&self, a: usize, b: usize) -> Option<usize> {
        self.links[a].iter().find(|l| l.0 == b).map(|l| l.1)
    }

    /// `setup_edge` and `connect_node`.
    fn connect(&mut self, a: usize, b: usize) -> usize {
        let e = self.edges.len();
        self.edges.push(Edge {
            a,
            b,
            distance: dist(self.nodes[a].point, self.nodes[b].point),
            large: false,
            flying: false,
            jumping: false,
        });
        self.links[a].push((b, e));
        self.links[b].push((a, e));
        e
    }

    /// A combat or goal point's edges to the (up to) four nearest points of its
    /// cell on its floor, within [`AUTO_LEN`] (half that and only one for another
    /// combat or goal point) and its PVS, kept as the original's array keeps them.
    fn auto_connect(&mut self, bsp: &Bsp, at: usize) {
        let here = self.nodes[at].point;
        let mut closest = [(None::<usize>, 0.0f32); WAY_NEIGHBORS];
        let mut highest = 0;
        let mut non_way = 0;
        for &to in self.cells.get(here) {
            if to == at || self.edge_across(at, to).is_some() {
                continue;
            }
            let node = &self.nodes[to];
            if (node.point[2] - here[2]).abs() > FLOOR_DIST {
                continue;
            }
            let cost = dist(node.point, here);
            if cost > AUTO_LEN {
                continue;
            }
            if matches!(node.kind, Kind::Combat | Kind::Goal) {
                non_way += 1;
                if non_way > NONWP_NEIGHBORS || cost > AUTO_LEN / 2.0 {
                    continue;
                }
            }
            if closest[highest].0.is_some() && closest[highest].1 < cost {
                continue;
            }
            if !in_pvs(bsp, node.point, here) {
                continue;
            }
            closest[highest] = (Some(to), cost);
            for i in 0..WAY_NEIGHBORS {
                if closest[i].0.is_none() {
                    highest = i;
                    break;
                }
                if closest[i].1 > closest[highest].1 {
                    highest = i;
                }
            }
        }
        for (to, _) in closest {
            if let Some(to) = to {
                self.connect(at, to);
            }
        }
    }

    /// `NAV::TestEdge` at the edge's size: no waypoint narrower than it, and its
    /// box sweeps from one end to the other.
    fn test_edge(&mut self, e: usize, world: &Collision) -> bool {
        let edge = &self.edges[e];
        let (a, b) = (&self.nodes[edge.a], &self.nodes[edge.b]);
        let (radius, height) = if edge.large { LARGE } else { MEDIUM };
        if a.kind == Kind::Way && a.radius < radius || b.kind == Kind::Way && b.radius < radius {
            return false;
        }
        let ok = move_trace(
            world,
            a.point,
            b.point,
            [-radius, -radius, 0.0],
            [radius, radius, height],
            false,
        );
        let flying = a.floating || b.floating;
        self.edges[e].flying |= flying;
        ok
    }

    fn remove_edges(&mut self, removed: &[usize]) {
        if removed.is_empty() {
            return;
        }
        let gone: std::collections::HashSet<usize> = removed.iter().copied().collect();
        for links in &mut self.links {
            links.retain(|l| !gone.contains(&l.1));
        }
    }

    /// Regions: islands share one, the rest one per connected group.
    fn find_regions(&mut self) {
        const NONE: usize = usize::MAX;
        self.regions = vec![NONE; self.nodes.len()];
        for (i, n) in self.nodes.iter().enumerate() {
            if n.island {
                self.regions[i] = 0;
            }
        }
        let mut next = 1;
        for start in 0..self.nodes.len() {
            if self.regions[start] != NONE {
                continue;
            }
            let mut stack = vec![start];
            self.regions[start] = next;
            while let Some(n) = stack.pop() {
                for &(to, _) in &self.links[n] {
                    if self.regions[to] == NONE {
                        self.regions[to] = next;
                        stack.push(to);
                    }
                }
            }
            next += 1;
        }
    }

    fn same_region(&self, a: usize, b: usize) -> bool {
        a == b || self.regions[a] == self.regions[b]
    }

    /// `NAV::GetNearestNode` for `pos`: of its cell's points within view range
    /// (those well above or below left out, farther for being off its floor, an
    /// island, or off the region of `previous` or `goal`), the nearest it stands
    /// within or sees; else an edge beside it that it sees.
    pub fn nearest(
        &self,
        world: &Collision,
        pos: Vec3,
        previous: Option<usize>,
        goal: Option<usize>,
    ) -> Option<Nearest> {
        if self.edges.is_empty() {
            return None;
        }
        let mut sorted = Vec::new();
        for &i in self.cells.get(pos) {
            let node = &self.nodes[i];
            let mut d = dist2(node.point, pos);
            let in_radius = d < node.radius * node.radius;
            let z = (node.point[2] - pos[2]).abs();
            if z > VIEW_RANGE / 4.0 {
                continue;
            }
            if z > 30.0 {
                d += z * z;
            }
            if d > VIEW_RANGE * VIEW_RANGE {
                continue;
            }
            if node.island {
                d *= 3.0;
            }
            if let Some(p) = previous.filter(|&p| p != i) {
                if !self.same_region(p, i) {
                    d += 100.0 * 100.0;
                }
                if self.edge_across(p, i).is_none() {
                    d += 200.0 * 200.0;
                }
            }
            if goal.is_some_and(|g| g != i && !self.same_region(g, i)) {
                d += 300.0 * 300.0;
            }
            sorted.push((d, i, in_radius));
        }
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, i, in_radius) in &sorted {
            if in_radius || view_trace(world, pos, self.nodes[i].point, true).0 {
                return Some(Nearest::Node(i));
            }
        }
        // The cells' edges (`fill_cells_edges`) are not listed here: an NPC beside
        // an edge but seeing none of its cell's points finds none.
        None
    }

    /// Whether `a` and `b` are joined by a short edge one walks
    /// (`NAV::OnNeighboringPoints`).
    pub fn neighboring(&self, a: usize, b: usize) -> bool {
        a == b
            || self.edge_across(a, b).is_some_and(|e| {
                let e = &self.edges[e];
                !e.jumping && !e.flying && e.distance < SAFE_NEIGHBOR_DIST
            })
    }

    /// `NAV::InSafeRadius`: `at` within the radius of `node` or of `target`, or
    /// beside the edge between them (within its size's radius).
    pub fn in_safe_radius(&self, at: Vec3, node: usize, target: usize) -> bool {
        if dist(at, self.nodes[node].point) < self.nodes[node].radius {
            return true;
        }
        if node == target {
            return false;
        }
        if dist(at, self.nodes[target].point) < self.nodes[target].radius {
            return true;
        }
        let Some(e) = self.edge_across(node, target) else {
            return false;
        };
        let e = &self.edges[e];
        if e.flying || e.jumping {
            return false;
        }
        let (on, _) = project_to_line(at, self.nodes[e.a].point, self.nodes[e.b].point);
        dist(at, on) < if e.large { LARGE.0 } else { MEDIUM.0 }
    }

    /// A* from `start` to `end` (`graph_vs::astar`): the cost of an edge its length,
    /// [`BIAS_NONWAYPOINT`] more into a point that is not a waypoint, guided by the
    /// straight distance. Edges too narrow for `large` NPCs, flown or jumped, are
    /// not taken. The path from `end` back to `start`.
    fn astar(&self, start: usize, end: usize, large: bool) -> Option<Vec<usize>> {
        let n = self.nodes.len();
        let mut cost = vec![f32::INFINITY; n];
        let mut from = vec![usize::MAX; n];
        let mut closed = vec![false; n];
        cost[start] = 0.0;
        let goal = self.nodes[end].point;
        let mut open = vec![start];
        while !open.is_empty() {
            let (k, _) = open
                .iter()
                .enumerate()
                .map(|(k, &i)| (k, cost[i] + dist(self.nodes[i].point, goal)))
                .min_by(|a, b| a.1.total_cmp(&b.1))?;
            let at = open.swap_remove(k);
            if at == end {
                let mut path = vec![end];
                while *path.last()? != start {
                    path.push(from[*path.last()?]);
                }
                return Some(path);
            }
            closed[at] = true;
            for &(to, e) in &self.links[at] {
                let edge = &self.edges[e];
                if closed[to] || edge.flying || edge.jumping || large && !edge.large {
                    continue;
                }
                let mut c = cost[at] + edge.distance;
                if self.nodes[to].kind != Kind::Way {
                    c += BIAS_NONWAYPOINT;
                }
                if c < cost[to] {
                    if cost[to] == f32::INFINITY {
                        open.push(to);
                    }
                    cost[to] = c;
                    from[to] = at;
                }
            }
        }
        None
    }

    /// `NAV::FindPath` for an NPC at `at` (box `mins`..`maxs`) from point `start` to
    /// `target`: the points of the shortest path, the last first, each reached
    /// within three times the NPC's radius (at most 20) or 40% of the point's
    /// radius, the last within its radius and slowed to over 70; the path cut where
    /// it passes beside the NPC (an edge it stands within 10% to 90% along, near
    /// enough or with a clear move to it), and sharp turns slowed through. Each
    /// point is due by level time `level` plus its distance along the path at
    /// `speed` (units a second, taken at a quarter for speeding up and slowing).
    #[allow(clippy::too_many_arguments)]
    pub fn find_path(
        &self,
        world: &Collision,
        at: Vec3,
        mins: Vec3,
        maxs: Vec3,
        start: usize,
        target: usize,
        level: i32,
        speed: f32,
    ) -> Option<Vec<PathPoint>> {
        if !self.same_region(start, target) {
            return None;
        }
        let nodes = self.astar(start, target, false)?;
        let radius = mins[0]
            .min(mins[1])
            .abs()
            .max(maxs[0].max(maxs[1]))
            .min(20.0);
        let mut path: Vec<PathPoint> = nodes
            .iter()
            .map(|&n| {
                let node = &self.nodes[n];
                let mut reached = (radius * 3.0).max(node.radius * 0.4);
                if node.floating {
                    reached = 20.0;
                }
                PathPoint {
                    node: n,
                    point: node.point,
                    slowing_radius: 0.0,
                    reached_radius: reached * reached,
                    eta: 0.0,
                }
            })
            .collect();
        // PPoint is left at the last copied, the start: its radius sets the end's.
        let first = &self.nodes[start];
        path[0].slowing_radius = 70.0f32.max(first.radius * 0.75);
        let reached = radius.max(first.radius * 0.15);
        path[0].reached_radius = reached * reached;
        for edge in 0..path.len() - 1 {
            let (a, b) = (path[edge].point, path[edge + 1].point);
            let (mut on, scale) = project_to_line(at, a, b);
            if scale < 0.0 {
                on = a;
            } else if scale > 1.0 {
                on = b;
            }
            let d = dist(on, at);
            if scale > 0.1
                && scale < 0.9
                && (d < radius || d < radius * 20.0 && move_trace(world, at, on, mins, maxs, false))
            {
                path.truncate(edge + 2);
                path[edge + 1].point = on;
                break;
            }
        }
        let speed = speed * 0.001 * 0.25;
        let mut from = at;
        let mut time = level as f32;
        for i in (0..path.len()).rev() {
            let mut to_next = sub(path[i].point, from);
            if to_next[2].abs() > Z_CULL_OFFSET {
                to_next[2] = 0.0;
            }
            let d = dot(to_next, to_next).sqrt();
            if d > 1e-10 {
                to_next = to_next.map(|x| x / d);
            }
            path[i].eta = d / speed + time;
            time = path[i].eta;
            if i != 0 && !self.nodes[path[i].node].floating {
                let mut beyond = sub(path[i - 1].point, path[i].point);
                if beyond[2].abs() > Z_CULL_OFFSET {
                    beyond[2] = 0.0;
                }
                let bd = dot(beyond, beyond).sqrt();
                if bd > 1e-10 {
                    beyond = beyond.map(|x| x / bd);
                }
                let turn = dot(beyond, to_next);
                if bd > 150.0 && d > 150.0 && turn < 0.64 || bd > 30.0 && turn < 0.5 {
                    path[i].slowing_radius = 40.0f32.max(self.nodes[path[i].node].radius);
                }
            }
            from = path[i].point;
        }
        Some(path)
    }
}

/// `G_CheckInSolid` with its fix, for a point at `origin` with the box
/// `mins`..`maxs`: its box from its origin up (`MASK_DEADSOLID`) swept down to its
/// feet; stopped on the way, the point is moved to where it stopped (resting a
/// little above the floor it stands on: a point placed on the floor rises by
/// [`crate::collision::SURFACE_CLIP_EPSILON`]) and checked again. Whether it is in
/// solid.
fn check_in_solid(world: &Collision, origin: &mut Vec3, mins: Vec3, maxs: Vec3) -> bool {
    const MASK_DEADSOLID: i32 = CONTENTS_SOLID | CONTENTS_PLAYERCLIP | CONTENTS_TERRAIN;
    let mut fix = true;
    loop {
        let feet = [origin[0], origin[1], origin[2] + mins[2]];
        let tr = world.trace(*origin, feet, [mins[0], mins[1], 0.0], maxs, MASK_DEADSOLID);
        if tr.all_solid || tr.start_solid {
            return true;
        }
        if tr.fraction == 1.0 {
            return false;
        }
        if !fix {
            return true;
        }
        *origin = [tr.end[0], tr.end[1], tr.end[2] - mins[2]];
        fix = false;
    }
}

/// The point entity `e` spawns, if it is one and its box is clear where it stands
/// (`NAV::SpawnedPoint`'s check).
fn point(e: &Entity, world: &Collision) -> Option<Node> {
    let class = e.get("classname")?.to_ascii_lowercase();
    let flags = e.get("spawnflags").map_or(0, atoi);
    let mut origin = e.get("origin").map_or([0.0; 3], vector_prefix);
    let spawn_radius = e.get("radius").map_or(0.0, atof);
    // SP_waypoint and SP_waypoint_small set a box and check it is clear, with its
    // fix, unless SOLID_OK; a point still in solid with a crouch's height is freed.
    let checked = |origin: &mut Vec3, half: f32| {
        if flags & 1 != 0 {
            return true;
        }
        let mins = [-half, -half, DEFAULT_MINS_2];
        if !check_in_solid(world, origin, mins, [half, half, DEFAULT_MAXS_2]) {
            return true;
        }
        !check_in_solid(world, origin, mins, [half, half, CROUCH_MAXS_2])
    };
    let (kind, radius) = match class.as_str() {
        "waypoint" => {
            if flags & 2 != 0 {
                origin[2] += 128.0;
            }
            if !checked(&mut origin, DEFAULT_MAXS_0) {
                return None;
            }
            (Kind::Way, waypoint_radius(world, origin))
        }
        "waypoint_small" => {
            if !checked(&mut origin, 2.0) {
                return None;
            }
            (Kind::Way, 2.0)
        }
        // SP_waypoint_navgoal raises its tag, not its point.
        "waypoint_navgoal" => (Kind::Goal, spawn_radius),
        "point_combat" => {
            origin[2] += 0.125;
            (Kind::Combat, spawn_radius)
        }
        _ => return None,
    };
    let up = [origin[0], origin[1], origin[2] + 5.0];
    let (mins, maxs) = ([-MEDIUM.0, -MEDIUM.0, 0.0], [MEDIUM.0, MEDIUM.0, MEDIUM.1]);
    if !move_trace(world, origin, up, mins, maxs, false) {
        return None;
    }
    let target = |k: &str| e.get(k).filter(|t| !t.is_empty()).map(str::to_owned);
    Some(Node {
        point: origin,
        radius,
        kind,
        name: target("targetname"),
        island: false,
        floating: false,
        drop: kind == Kind::Way && flags & 2 != 0,
        no_auto: flags & 4 != 0,
        targets: ["target", "target2", "target3", "target4", "targetJump"].map(target),
    })
}
