//! The brush entities a map spawns (doors, plats, walls, breakables: the map's
//! models after the world, `bsp.models[1..]`), where they stand when it starts, as
//! the game spawns them (OpenJK `code/game/g_mover.cpp`: `SP_func_door`,
//! `SP_func_plat`, `SP_func_static`, `SP_func_rotating`, `SP_func_wall`,
//! `InitMover`; `g_usable.cpp`: `SP_func_usable`; `g_breakable.cpp`:
//! `SP_func_breakable`, `SP_func_glass`). The server sends each to a view by the
//! PVS of its linked box (`SV_LinkEntity`) and cgame draws it at its origin and
//! angles (`code/cgame/cg_ents.cpp`: `CG_Mover`).
//!
//! The brushes' vertices are stored relative to the entity's origin (the compiler
//! moves an entity with an "origin" key by it), so the entity's place is where they
//! are drawn from.
//!
//! An entity with a "model2" key also draws that model where it stands (`InitMover`,
//! `InitBBrush`; cgame's `CG_Mover` adds it after the brushes).

use swjkja_bsp::{Entity, Model};

use crate::static_model::{
    Ghoul2, Rules, StaticModel, angles_to_axis, atof, spawns_at, spawns_in, vector_prefix,
};
use crate::style::atoi;
use crate::{Geometry, Vec3};

/// A brush entity as the map starts.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushEntity {
    /// Its model in `bsp.models` (1 or more).
    pub model: usize,
    /// Where it is drawn from (cgame's `lerpOrigin`).
    pub origin: Vec3,
    /// `AnglesToAxis` of its angles (forward, left, up): only a `func_static` keeps
    /// the angles it spawns with (`G_SetAngles`); the others stand at zero.
    pub axes: [Vec3; 3],
    /// Its linked box (`absmin`, `absmax`), for the PVS test.
    pub entity_box: [Vec3; 2],
    /// `SVF_BROADCAST`: sent to every view.
    pub broadcast: bool,
    /// `EF_SHADER_ANIM`: animated images hold the entity's frame (0 at spawn)
    /// instead of playing (`R_BindAnimatedImage` with `RF_SETANIMINDEX`).
    pub shader_anim: bool,
    /// Its secondary model ("model2"), drawn at its origin and sent with its box.
    pub model2: Option<StaticModel>,
}

/// `MOVER_LOCKED`: a door locked until used, its shader animation frame-controlled.
const MOVER_LOCKED: i64 = 16;

/// The brush entities `entities` spawn under `rules` and cgame draws, in spawn order,
/// with the map's `models`. Left out: those spawned hidden (a `func_wall` or `func_usable` started off,
/// spawnflag 1: `SVF_NOCLIENT`; a `rail_mover`, `EF_NODRAW` until its rail starts
/// it), the classes no retail map spawns with surfaces, and the ones whose model the
/// map lacks.
pub fn brush_entities(entities: &[Entity], models: &[Model], rules: Rules) -> Vec<BrushEntity> {
    entities
        .iter()
        .filter(|e| match rules {
            Rules::SinglePlayer { skill } => spawns_at(e, skill),
            Rules::Multiplayer { gametype } => spawns_in(e, gametype),
        })
        .filter_map(|e| spawn(models, e, rules))
        .collect()
}

fn spawn(models: &[Model], e: &Entity, rules: Rules) -> Option<BrushEntity> {
    let model = atoi(e.get("model")?.strip_prefix('*')?);
    let model = usize::try_from(model).ok().filter(|&m| m >= 1)?;
    let m = models.get(model)?;
    let (mins, maxs) = (m.mins, m.maxs);
    let class = e.get("classname").unwrap_or("").to_ascii_lowercase();
    let spawnflags = e.get("spawnflags").map_or(0, atoi);
    let mut origin = e.get("origin").map_or([0.0; 3], vector_prefix);
    // G_ParseField: "angles" as three numbers, "angle" as a yaw.
    let angles = match (e.get("angles"), e.get("angle")) {
        (Some(a), _) => vector_prefix(a),
        (None, Some(yaw)) => [0.0, atof(yaw), 0.0],
        (None, None) => [0.0; 3],
    };
    let lip = || e.get("lip").map_or(8.0, atof);
    // "forcevisible" makes one with no origin brush broadcast (force sight sees it
    // through walls).
    let force_visible = e.get("forcevisible").map_or(0, atoi) != 0 && origin == [0.0; 3];
    let (mut drawn_angles, mut broadcast, mut shader_anim) = ([0.0; 3], false, false);
    match class.as_str() {
        "func_door" => {
            // pos2 = pos1 moved along movedir by the model's size there, less the
            // lip; "start_open" (spawnflag 1) starts at pos2.
            if spawnflags & 1 != 0 {
                let dir = movedir(angles);
                let size = [0, 1, 2].map(|i| maxs[i] - mins[i]);
                let distance = (0..3).map(|i| dir[i].abs() * size[i]).sum::<f32>() - lip();
                origin = [0, 1, 2].map(|i| origin[i] + distance * dir[i]);
            }
            shader_anim = spawnflags & MOVER_LOCKED != 0;
        }
        // pos1, where it starts, is the bottom: "height" below the origin, or the
        // model's height less the lip.
        "func_plat" => {
            let height = match e.get("height") {
                Some(h) => atof(h),
                None => maxs[2] - mins[2] - lip(),
            };
            origin[2] -= height;
        }
        "func_static" => {
            drawn_angles = angles;
            broadcast = spawnflags & 2048 != 0;
            shader_anim = spawnflags & 4 != 0;
        }
        // Rotating ones turn from zero as time passes (left out: drawn at zero).
        "func_rotating" | "func_glass" => {}
        "func_breakable" => broadcast = force_visible,
        "func_wall" if spawnflags & 1 == 0 => {}
        "func_usable" if spawnflags & 1 == 0 => {
            broadcast = force_visible;
            shader_anim = e.get("endframe").map_or(0, atoi) > 0;
        }
        _ => return None,
    }
    // SV_LinkEntity: the model's box at the origin, or a cube of the model's radius
    // when it is turned; an epsilon larger.
    let entity_box = if drawn_angles == [0.0; 3] {
        [
            [0, 1, 2].map(|i| origin[i] + mins[i] - 1.0),
            [0, 1, 2].map(|i| origin[i] + maxs[i] + 1.0),
        ]
    } else {
        let corner = [0, 1, 2].map(|i| mins[i].abs().max(maxs[i].abs()));
        let radius = corner.iter().map(|c| c * c).sum::<f32>().sqrt();
        [
            origin.map(|o| o - radius - 1.0),
            origin.map(|o| o + radius + 1.0),
        ]
    };
    let model2 = e
        .get("model2")
        .and_then(|name| secondary(e, &class, rules, name, origin, drawn_angles, entity_box));
    Some(BrushEntity {
        model,
        origin,
        axes: angles_to_axis(drawn_angles),
        entity_box,
        broadcast,
        shader_anim,
        model2,
    })
}

/// The "model2" an entity of `class` draws, from its `origin` at its `angles`.
///
/// Single player (`code/game/g_mover.cpp`: `InitMover`; `code/cgame/cg_ents.cpp`:
/// `CG_Mover`) turns it further by "modelAngles" when they are not zero, and makes a
/// `.glm` a Ghoul2 model of radius 120 at frame 0 of its skeleton. Multiplayer
/// (`codemp`) has no "modelAngles", draws no `.glm`, and scales the model of a
/// `func_static` or `func_rotating` by "model2scale" percent (0, the default, for
/// none; clamped to 1023: `ScaleModelAxis`).
fn secondary(
    e: &Entity,
    class: &str,
    rules: Rules,
    name: &str,
    origin: Vec3,
    angles: Vec3,
    entity_box: [Vec3; 2],
) -> Option<StaticModel> {
    let ghoul2 = name.contains(".glm");
    let (mut axes, mut scale, mut normalized) = (angles_to_axis(angles), [1.0; 3], true);
    match rules {
        Rules::SinglePlayer { .. } => {
            let model_angles = e.get("modelAngles").map_or([0.0; 3], vector_prefix);
            if model_angles != [0.0; 3] {
                axes = angles_to_axis([0, 1, 2].map(|i| angles[i] + model_angles[i]));
            }
        }
        Rules::Multiplayer { .. } => {
            if ghoul2 {
                return None;
            }
            let percent = match class {
                "func_static" | "func_rotating" => e.get("model2scale").map_or(0, atoi),
                _ => 0,
            }
            .clamp(0, 1023);
            if percent != 0 {
                let s = percent as f32 / 100.0;
                scale = [s; 3];
                axes = axes.map(|a| a.map(|x| x * s));
                normalized = false;
            }
        }
    }
    Some(StaticModel {
        model: name.to_owned(),
        origin,
        axes,
        scale,
        normalized,
        zoffset: 0.0,
        entity_box: Some(entity_box),
        ghoul2: ghoul2.then(|| Ghoul2 {
            skin: None,
            switches: Vec::new(),
            radius: 120.0,
            pose: None,
            attached: Vec::new(),
        }),
    })
}

/// `G_SetMovedir`: straight up for yaw -1, down for -2, else the angles' forward.
fn movedir(angles: Vec3) -> Vec3 {
    match angles {
        [0.0, -1.0, 0.0] => [0.0, 0.0, 1.0],
        [0.0, -2.0, 0.0] => [0.0, 0.0, -1.0],
        _ => angles_to_axis(angles)[0],
    }
}

impl BrushEntity {
    /// A point of the model's space in the world.
    pub fn place(&self, v: Vec3) -> Vec3 {
        [0, 1, 2].map(|k| self.origin[k] + (0..3).map(|i| v[i] * self.axes[i][k]).sum::<f32>())
    }

    /// A direction of the model's space in the world.
    pub fn turn(&self, v: Vec3) -> Vec3 {
        [0, 1, 2].map(|k| (0..3).map(|i| v[i] * self.axes[i][k]).sum::<f32>())
    }

    /// The eight corners of a box of the model's space, in the world
    /// (`R_CullLocalBox`'s).
    pub fn corners(&self, [mins, maxs]: [Vec3; 2]) -> [Vec3; 8] {
        std::array::from_fn(|i| {
            self.place([
                if i & 1 == 0 { mins[0] } else { maxs[0] },
                if i & 2 == 0 { mins[1] } else { maxs[1] },
                if i & 4 == 0 { mins[2] } else { maxs[2] },
            ])
        })
    }

    /// A surface of its model, placed in the world: vertices, normals and planes
    /// turned and moved, bounds around the moved corners.
    pub fn placed(&self, geometry: &Geometry) -> Geometry {
        let verts = |verts: &[swjkja_bsp::DrawVert]| -> Vec<swjkja_bsp::DrawVert> {
            verts
                .iter()
                .map(|v| swjkja_bsp::DrawVert {
                    xyz: self.place(v.xyz),
                    normal: self.turn(v.normal),
                    ..*v
                })
                .collect()
        };
        let bounds = |b: [Vec3; 2]| {
            let c = self.corners(b);
            [0, 1].map(|s| {
                [0, 1, 2].map(|k| {
                    let it = c.iter().map(|p| p[k]);
                    if s == 0 {
                        it.fold(f32::INFINITY, f32::min)
                    } else {
                        it.fold(f32::NEG_INFINITY, f32::max)
                    }
                })
            })
        };
        match geometry {
            Geometry::Face {
                verts: v,
                indexes,
                normal,
                dist,
            } => {
                let normal = self.turn(*normal);
                let moved = (0..3).map(|k| normal[k] * self.origin[k]).sum::<f32>();
                Geometry::Face {
                    verts: verts(v),
                    indexes: indexes.clone(),
                    normal,
                    dist: dist + moved,
                }
            }
            Geometry::Triangles {
                verts: v,
                indexes,
                bounds: b,
            } => Geometry::Triangles {
                verts: verts(v),
                indexes: indexes.clone(),
                bounds: bounds(*b),
            },
            Geometry::Grid(g) => {
                let mut g = g.clone();
                g.verts = verts(&g.verts);
                g.mesh_bounds = bounds(g.mesh_bounds);
                g.local_origin = self.place(g.local_origin);
                g.lod_origin = self.place(g.lod_origin);
                Geometry::Grid(g)
            }
            Geometry::Flare {
                origin,
                color,
                normal,
            } => Geometry::Flare {
                origin: self.place(*origin),
                color: *color,
                normal: self.turn(*normal),
            },
            Geometry::Skip => Geometry::Skip,
        }
    }
}
