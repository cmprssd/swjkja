//! World effects: what the map's weather entities ask the renderer for, and where
//! the renderer counts as outside.
//!
//! In single player, `fx_rain`, `fx_snow`, `fx_wind`, `fx_wind_zone` (`g_fx.cpp`)
//! and `misc_weather_zone` (`g_misc.cpp`) each add commands to the `CS_WORLD_FX`
//! config strings as they spawn (`G_FindConfigstringIndex`: one string once, in the
//! order first added), and cgame hands every string to the renderer as the map
//! loads (`CG_RegisterEffects`, `R_WorldEffectCommand`). [`commands`] gives those
//! strings, [`Command::parse`] reads one as the renderer does.
//!
//! The renderer draws weather particles only where it counts as outside
//! (`tr_WorldEffects.cpp`, `COutside`): a map marks its outside (or its inside)
//! with brushes of `CONTENTS_OUTSIDE` (or `CONTENTS_INSIDE`), scanned once into a
//! grid of 32-unit cells over each weather zone, or over the world's bounds when the
//! map has none ([`Outside`]).
//!
//! Left out: `fx_puff` (`SP_CreatePuffSystem`), multiplayer's spawners, and the
//! cached `.weather` file (it holds what the scan finds).

use swjkja_bsp::{Bsp, Entity, Model};

use crate::Vec3;
use crate::static_model::{angles_to_axis, atof, spawns_at, vector_prefix};
use crate::style::atoi;

pub const CONTENTS_WATER: i32 = 0x4;
pub const CONTENTS_OUTSIDE: i32 = 0x1_0000;
pub const CONTENTS_INSIDE: i32 = 0x1000_0000;
/// `MAX_WORLD_FX`: config string 0 is never used, so a map has 65 at most.
pub const MAX_WORLD_FX: usize = 66;
/// `POINTCACHE_CELL_SIZE`.
pub const CELL_SIZE: f32 = 32.0;
/// `MAX_WEATHER_ZONES`.
pub const MAX_WEATHER_ZONES: usize = 50;

/// The `CS_WORLD_FX` strings the entities of a single-player map add as they spawn
/// at `skill`, in order. `models` are the map's brush models, whose bounds a zone
/// takes (`SetBrushModel`: the compiler's bounds spread by a unit, `CMod_LoadSubmodels`).
pub fn commands(entities: &[Entity], models: &[Model], skill: u32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: String| {
        if out.len() + 1 < MAX_WORLD_FX && !out.iter().any(|o| o.eq_ignore_ascii_case(&s)) {
            out.push(s);
        }
    };
    for e in entities.iter().filter(|e| spawns_at(e, skill)) {
        let class = e.get("classname").unwrap_or("").to_ascii_lowercase();
        let flags = e.get("spawnflags").map_or(0, atoi);
        let bounds = || {
            let model = e
                .get("model")
                .and_then(|m| m.strip_prefix('*'))
                .and_then(|m| models.get(atoi(m).max(0) as usize));
            model.map_or([[0.0; 3]; 2], |m| {
                [m.mins.map(|x| x - 1.0), m.maxs.map(|x| x + 1.0)]
            })
        };
        // `G_SpawnFloat("speed", "500")` along the entity's angles' forward.
        let wind = || {
            let angles = match (e.get("angles"), e.get("angle")) {
                (Some(a), _) => vector_prefix(a),
                (None, Some(yaw)) => [0.0, atof(yaw), 0.0],
                (None, None) => [0.0; 3],
            };
            let speed = e.get("speed").map_or(500.0, atof);
            angles_to_axis(angles)[0].map(|x| x * speed)
        };
        match class.as_str() {
            "fx_rain" => {
                if flags & 1 != 0 {
                    add("lightrain".into());
                } else if flags & 2 != 0 {
                    add("rain".into());
                } else if flags & 4 != 0 {
                    add("heavyrain".into());
                    add("heavyrainfog".into());
                } else if flags & 8 != 0 {
                    add("acidrain".into());
                } else {
                    add("lightrain".into());
                    add("wind".into());
                }
                if flags & 16 != 0 {
                    add("outsideShake".into());
                }
                if flags & 32 != 0 {
                    add("fog".into());
                }
            }
            "fx_snow" => {
                if flags & 1 != 0 {
                    add("lightsnow".into());
                } else if flags & 2 != 0 {
                    add("snow".into());
                } else if flags & 4 != 0 {
                    add("heavysnow".into());
                } else {
                    add("snow".into());
                    add("fog".into());
                }
                if flags & 8 != 0 {
                    add("fog".into());
                }
            }
            "fx_wind" => {
                if flags & 1 != 0 {
                    add("wind".into());
                }
                if flags & 2 != 0 {
                    add(format!("constantwind {}", vector(wind())));
                }
                if flags & 4 != 0 {
                    add("gustingwind".into());
                }
                if flags & 8 != 0 {
                    add("swirlingwind".into());
                }
                if flags & 32 != 0 {
                    add("fog".into());
                }
                if flags & 64 != 0 {
                    add("light_fog".into());
                }
            }
            "fx_wind_zone" => {
                let [mins, maxs] = bounds();
                add(format!(
                    "windzone {} {} {}",
                    vector(mins),
                    vector(maxs),
                    vector(wind())
                ));
            }
            "misc_weather_zone" => {
                let [mins, maxs] = bounds();
                add(format!("zone {} {}", vector(mins), vector(maxs)));
            }
            _ => {}
        }
    }
    out
}

/// `sprintf("( %f %f %f )")`.
fn vector(v: Vec3) -> String {
    format!(
        "( {:.6} {:.6} {:.6} )",
        f64::from(v[0]),
        f64::from(v[1]),
        f64::from(v[2])
    )
}

/// A world effect command as the renderer reads it (`R_WorldEffectCommand`).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// A weather zone: the outside scan covers it.
    Zone {
        mins: Vec3,
        maxs: Vec3,
    },
    /// A global wind changing at random.
    Wind,
    /// A global wind of this velocity.
    ConstantWind(Vec3),
    /// A global wind gusting at random.
    GustingWind,
    /// A wind of this velocity inside the box.
    WindZone {
        mins: Vec3,
        maxs: Vec3,
        velocity: Vec3,
    },
    LightRain,
    Rain,
    AcidRain,
    HeavyRain,
    /// One this renderer does not run (snow, fog clouds, space dust, ...), by its
    /// first word.
    Other(String),
}

impl Command {
    /// The command `s` reads as, `None` for one the original ignores (an empty one,
    /// or a zone whose vectors do not parse).
    pub fn parse(s: &str) -> Option<Self> {
        let mut words = s.split_ascii_whitespace();
        let name = words.next()?;
        let mut vector = || -> Option<Vec3> {
            // `WE_ParseVector`: "(", three numbers (atof), ")".
            (words.next()? == "(").then_some(())?;
            let mut v = [0.0; 3];
            for x in &mut v {
                *x = atof(words.next()?);
            }
            (words.next()? == ")").then_some(v)
        };
        let is = |n: &str| name.eq_ignore_ascii_case(n);
        Some(if is("zone") {
            Command::Zone {
                mins: vector()?,
                maxs: vector()?,
            }
        } else if is("wind") {
            Command::Wind
        } else if is("constantwind") {
            Command::ConstantWind(vector().unwrap_or([0.0, 800.0, 0.0]))
        } else if is("gustingwind") {
            Command::GustingWind
        } else if is("windzone") {
            Command::WindZone {
                mins: vector()?,
                maxs: vector()?,
                velocity: vector().unwrap_or([0.0, 800.0, 0.0]),
            }
        } else if is("lightrain") {
            Command::LightRain
        } else if is("rain") {
            Command::Rain
        } else if is("acidrain") {
            Command::AcidRain
        } else if is("heavyrain") {
            Command::HeavyRain
        } else {
            Command::Other(name.to_ascii_lowercase())
        })
    }
}

/// One weather zone's cells (`SWeatherZone`): a bit per cell, set where the scan
/// found an outside or inside brush.
#[derive(Debug, Clone, PartialEq)]
struct Zone {
    /// The zone's bounds snapped to the cell grid (`mExtents`).
    mins: Vec3,
    maxs: Vec3,
    /// The same in cells (`mSize.mMins`).
    cell_mins: Vec3,
    width: i32,
    height: i32,
    /// Columns of 32 cells in z (`mDepth`).
    depth: i32,
    bits: Vec<u32>,
}

impl Zone {
    /// `AddWeatherZone`.
    fn new(mins: Vec3, maxs: Vec3) -> Self {
        let (mins, maxs) = (mins.map(snap), maxs.map(snap));
        let cell_mins = mins.map(|x| x / CELL_SIZE);
        let cell_maxs = maxs.map(|x| x / CELL_SIZE);
        let width = (cell_maxs[0] - cell_mins[0]) as i32;
        let height = (cell_maxs[1] - cell_mins[1]) as i32;
        let depth = ((cell_maxs[2] - cell_mins[2]) as i32 + 31) >> 5;
        Self {
            mins,
            maxs,
            cell_mins,
            width,
            height,
            depth,
            bits: vec![0; (width.max(0) * height.max(0) * depth.max(0)) as usize],
        }
    }

    /// `SVecRange::In`: strictly inside the bounds.
    fn contains(&self, p: Vec3) -> bool {
        (0..3).all(|i| p[i] > self.mins[i] && p[i] < self.maxs[i])
    }

    /// `ConvertToCell`: the cell's x, y, z column and bit in it.
    fn cell(&self, p: Vec3) -> [i32; 4] {
        let c = |i: usize| (p[i] / CELL_SIZE - self.cell_mins[i]) as i32;
        let z = c(2);
        [c(0), c(1), z >> 5, z & 31]
    }

    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        (z * self.width * self.height + y * self.width + x) as usize
    }
}

/// `SnapFloatToGrid` to [`CELL_SIZE`]: truncated, then to the nearest multiple,
/// half a cell exactly going toward zero.
fn snap(f: f32) -> f32 {
    let grid = CELL_SIZE as i32;
    let f = f as i32;
    let (neg, mut a) = (f < 0, f.abs());
    let mut offset = a % grid;
    if offset > grid / 2 {
        offset = -(grid - offset);
    }
    a -= offset;
    (if neg { -a } else { a }) as f32
}

/// The renderer's outside cache (`COutside` once `Cache` has run).
#[derive(Debug, Clone, PartialEq)]
pub struct Outside {
    zones: Vec<Zone>,
    /// The scan found outside brushes (set bits mark outside), not inside ones
    /// (set bits mark inside). False when it found neither: all is outside.
    marked_outside: bool,
}

/// The scan found both outside and inside brushes: the original stops the map
/// load ("Both Indoor and Outdoor brushs encountered in map").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixedBrushes;

/// The world brushes' contents at a point, found through the map's BSP tree
/// (`CM_PointContents` with model 0): the brushes of the leaf holding the point.
pub struct PointContents {
    nodes: Vec<(Vec3, f32, [i32; 2])>,
    /// Per leaf: its brushes' indexes into `brushes`.
    leaves: Vec<Vec<usize>>,
    /// Per brush: its contents and planes.
    brushes: Vec<(i32, Vec<(Vec3, f32)>)>,
}

impl PointContents {
    pub fn new(bsp: &Bsp) -> Self {
        let range = |first: i32, n: i32| first.max(0) as usize..(first.max(0) + n.max(0)) as usize;
        let plane = |i: i32| {
            let p = bsp.planes[i as usize];
            (p.normal, p.dist)
        };
        Self {
            nodes: bsp
                .nodes
                .iter()
                .map(|n| {
                    let (normal, dist) = plane(n.plane);
                    (normal, dist, n.children)
                })
                .collect(),
            leaves: bsp
                .leafs
                .iter()
                .map(|l| {
                    bsp.leaf_brushes[range(l.first_leaf_brush, l.num_leaf_brushes)]
                        .iter()
                        .map(|&b| b as usize)
                        .collect()
                })
                .collect(),
            brushes: bsp
                .brushes
                .iter()
                .map(|b| {
                    let planes = bsp.brush_sides[range(b.first_side, b.num_sides)]
                        .iter()
                        .map(|s| plane(s.plane))
                        .collect();
                    (bsp.shaders[b.shader as usize].content_flags, planes)
                })
                .collect(),
        }
    }

    /// The contents of the brushes holding `p` in its leaf, on their faces
    /// included; 0 for a map without nodes.
    pub fn at(&self, p: Vec3) -> i32 {
        if self.nodes.is_empty() {
            return 0;
        }
        // `CM_PointLeafnum_r`: in front of a node's plane (or on it) goes to its
        // first child.
        let mut num = 0i32;
        while num >= 0 {
            let (normal, dist, children) = self.nodes[num as usize];
            let d = normal[0] * p[0] + normal[1] * p[1] + normal[2] * p[2] - dist;
            num = if d < 0.0 { children[1] } else { children[0] };
        }
        let Some(leaf) = self.leaves.get((-1 - num) as usize) else {
            return 0;
        };
        leaf.iter()
            .filter_map(|&b| self.brushes.get(b))
            .filter(|(_, planes)| {
                planes
                    .iter()
                    .all(|&(n, d)| n[0] * p[0] + n[1] * p[1] + n[2] * p[2] <= d)
            })
            .fold(0, |c, (contents, _)| c | contents)
    }
}

impl Outside {
    /// Scans the world brushes' `contents` at each cell's centre over `zones` (the
    /// `zone` commands' boxes, [`MAX_WEATHER_ZONES`] at most), or over
    /// `world_bounds` (the world model's) when there are none.
    pub fn scan(
        contents: impl Fn(Vec3) -> i32,
        zones: &[(Vec3, Vec3)],
        world_bounds: (Vec3, Vec3),
    ) -> Result<Self, MixedBrushes> {
        let boxes = if zones.is_empty() {
            &[world_bounds][..]
        } else {
            &zones[..zones.len().min(MAX_WEATHER_ZONES)]
        };
        let mut zones: Vec<Zone> = boxes.iter().map(|&(a, b)| Zone::new(a, b)).collect();
        let mut marked: Option<bool> = None;
        for zone in &mut zones {
            let base = zone.mins.map(|x| x + CELL_SIZE / 2.0);
            for z in 0..zone.depth {
                for q in 0..32 {
                    for x in 0..zone.width {
                        for y in 0..zone.height {
                            let p = [
                                x as f32 * CELL_SIZE + base[0],
                                y as f32 * CELL_SIZE + base[1],
                                ((z << 5) + q) as f32 * CELL_SIZE + base[2],
                            ];
                            let contents = contents(p);
                            if contents & (CONTENTS_INSIDE | CONTENTS_OUTSIDE) == 0 {
                                continue;
                            }
                            let outside = contents & CONTENTS_OUTSIDE != 0;
                            if *marked.get_or_insert(outside) != outside {
                                return Err(MixedBrushes);
                            }
                            let i = zone.index(x, y, z);
                            zone.bits[i] |= 1 << q;
                        }
                    }
                }
            }
        }
        Ok(Self {
            zones,
            marked_outside: marked.unwrap_or(false),
        })
    }

    /// `CellOutside`: a cell beyond the zone counts as unmarked.
    fn cell_outside(&self, zone: &Zone, x: i32, y: i32, z: i32, bit: i32) -> bool {
        if !(0..zone.width).contains(&x)
            || !(0..zone.height).contains(&y)
            || !(0..zone.depth).contains(&z)
            || !(0..32).contains(&bit)
        {
            return !self.marked_outside;
        }
        self.marked_outside == (zone.bits[zone.index(x, y, z)] & (1 << bit) != 0)
    }

    /// `PointOutside(pos)`: whether `p` is outside, by the first zone holding it;
    /// beyond every zone, outside unless the scan marked the outside.
    pub fn point(&self, p: Vec3) -> bool {
        match self.zones.iter().find(|z| z.contains(p)) {
            Some(zone) => {
                let [x, y, z, bit] = zone.cell(p);
                self.cell_outside(zone, x, y, z, bit)
            }
            None => !self.marked_outside,
        }
    }

    /// `PointOutside(pos, width, height)`: whether a particle `width` by `height`
    /// at `p` is outside. One under a cell either way tests its own cell; a larger
    /// one every cell its size spans around it (the original's y span is its
    /// width's, and its z steps the bit past the column's 32, which counts as
    /// unmarked).
    pub fn particle(&self, p: Vec3, width: f32, height: f32) -> bool {
        let Some(zone) = self.zones.iter().find(|z| z.contains(p)) else {
            return !self.marked_outside;
        };
        let [x, y, z, bit] = zone.cell(p);
        if width < CELL_SIZE || height < CELL_SIZE {
            return self.cell_outside(zone, x, y, z, bit);
        }
        let w = (width as i32) / CELL_SIZE as i32;
        let h = (height as i32) / CELL_SIZE as i32;
        for cx in x - w..=x + w {
            for cy in y - w..=y + w {
                for cb in bit - h..=bit + h {
                    if !self.cell_outside(zone, cx, cy, z, cb) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_the_nearest_cell_half_toward_zero() {
        assert_eq!(snap(47.9), 32.0);
        assert_eq!(snap(48.0), 32.0);
        assert_eq!(snap(49.0), 64.0);
        assert_eq!(snap(-49.5), -64.0);
        assert_eq!(snap(-16.0), 0.0);
        assert_eq!(snap(-17.0), -32.0);
    }

    /// A box from `mins` to `maxs`, its sides in the compiler's order.
    fn block(contents: i32, mins: Vec3, maxs: Vec3) -> (i32, Vec<(Vec3, f32)>) {
        let mut planes = Vec::new();
        for i in 0..3 {
            let mut n = [0.0; 3];
            n[i] = -1.0;
            planes.push((n, -mins[i]));
            n[i] = 1.0;
            planes.push((n, maxs[i]));
        }
        (contents, planes)
    }

    #[test]
    fn point_contents_are_the_brushes_of_the_points_leaf() {
        // One node splitting at x = 0: leaf 0 in front holds an outside brush,
        // leaf 1 behind an inside brush (also reaching into the front: not in its
        // leaf, so not seen from there).
        let contents = PointContents {
            nodes: vec![([1.0, 0.0, 0.0], 0.0, [-1, -2])],
            leaves: vec![vec![0], vec![1]],
            brushes: vec![
                block(CONTENTS_OUTSIDE, [0.0, -64.0, -64.0], [100.0, 64.0, 64.0]),
                block(CONTENTS_INSIDE, [-100.0, -64.0, -64.0], [50.0, 64.0, 64.0]),
            ],
        };
        assert_eq!(contents.at([25.0, 0.0, 0.0]), CONTENTS_OUTSIDE);
        assert_eq!(contents.at([-50.0, 0.0, 0.0]), CONTENTS_INSIDE);
        assert_eq!(contents.at([150.0, 0.0, 0.0]), 0);
        // On the node's plane: in front; on the brush's face: inside it.
        assert_eq!(contents.at([0.0, 0.0, 64.0]), CONTENTS_OUTSIDE);
    }
}
