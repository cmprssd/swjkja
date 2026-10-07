//! Suggests reference views of every multiplayer fog volume, as views.txt lines:
//! one from inside the fog looking along its longer side, one from 64 units above
//! it looking down. Each camera is at the open point (not in a solid leaf, with 32
//! units of open space around it) nearest the middle of the fog.
//! Usage: fog_views <GameData folder>
use swjkja_bsp::Bsp;

fn leaf_cluster(bsp: &Bsp, p: [f32; 3]) -> i32 {
    let mut node = 0i32;
    loop {
        let n = &bsp.nodes[node as usize];
        let plane = &bsp.planes[n.plane as usize];
        let d =
            p[0] * plane.normal[0] + p[1] * plane.normal[1] + p[2] * plane.normal[2] - plane.dist;
        let child = if d > 0.0 {
            n.children[0]
        } else {
            n.children[1]
        };
        if child < 0 {
            return bsp.leafs[(-(child + 1)) as usize].cluster;
        }
        node = child;
    }
}

fn open(bsp: &Bsp, p: [f32; 3]) -> bool {
    let offsets = [
        [0.0, 0.0, 0.0],
        [32.0, 0.0, 0.0],
        [-32.0, 0.0, 0.0],
        [0.0, 32.0, 0.0],
        [0.0, -32.0, 0.0],
        [0.0, 0.0, 32.0],
        [0.0, 0.0, -32.0],
    ];
    offsets
        .iter()
        .all(|o| leaf_cluster(bsp, [p[0] + o[0], p[1] + o[1], p[2] + o[2]]) >= 0)
}

/// The open point at height `z` nearest the middle of the box, on a 16-unit grid.
fn nearest_open(bsp: &Bsp, lo: [f32; 3], hi: [f32; 3], z: f32) -> Option<[f32; 3]> {
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let mut best: Option<([f32; 3], f32)> = None;
    let mut x = lo[0] + 16.0;
    while x < hi[0] {
        let mut y = lo[1] + 16.0;
        while y < hi[1] {
            let p = [x, y, z];
            let d = (x - mid[0]).powi(2) + (y - mid[1]).powi(2);
            if best.is_none_or(|b| d < b.1) && open(bsp, p) {
                best = Some((p, d));
            }
            y += 16.0;
        }
        x += 16.0;
    }
    best.map(|b| b.0)
}

fn main() {
    let gamedata = std::env::args()
        .nth(1)
        .expect("usage: fog_views <GameData>");
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let mut maps: Vec<&str> = vfs
        .files()
        .filter(|f| f.starts_with("maps/mp/") && f.ends_with(".bsp"))
        .collect();
    maps.sort_unstable();
    for name in maps {
        let bsp = Bsp::parse(&vfs.read(name).unwrap()).unwrap();
        let map = name.trim_start_matches("maps/").trim_end_matches(".bsp");
        for (i, f) in bsp.fogs.iter().enumerate().filter(|(_, f)| f.brush >= 0) {
            // R_LoadFogs's bounds: brushes list their six axial sides first.
            let first = bsp.brushes[f.brush as usize].first_side as usize;
            let d: Vec<f32> = (0..6)
                .map(|k| bsp.planes[bsp.brush_sides[first + k].plane as usize].dist)
                .collect();
            let (lo, hi) = ([-d[0], -d[2], -d[4]], [d[1], d[3], d[5]]);
            let yaw = if hi[0] - lo[0] >= hi[1] - lo[1] {
                0
            } else {
                90
            };
            let short = map.trim_start_matches("mp/");
            if let Some(p) = nearest_open(&bsp, lo, hi, lo[2] + (hi[2] - lo[2]) * 0.6) {
                println!(
                    "{map:9} {short}_fog{i}_in {} {} {}  0 {yaw} 0",
                    p[0], p[1], p[2]
                );
            }
            if let Some(p) = nearest_open(&bsp, lo, hi, hi[2] + 64.0) {
                println!(
                    "{map:9} {short}_fog{i}_above {} {} {}  50 {yaw} 0",
                    p[0], p[1], p[2]
                );
            }
        }
    }
}
