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
//!
//! In single player a door that spawns a trigger opens when an NPC stands in it
//! ([`touch_doors`]), and stays open while the NPC does.

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
    /// A func_rotating that turns: its angles' change, in degrees a second (pitch,
    /// yaw, roll), from zero at time 0 (`TR_LINEAR` with `trTime` 0). It is drawn at
    /// [`BrushEntity::angles_at`], not at `axes`.
    pub spin: Option<Vec3>,
    /// A single-player door: how it opens.
    pub door: Option<Door>,
    /// A door opening: it is drawn from [`BrushEntity::origin_at`], not `origin`.
    pub slide: Option<Slide>,
}

/// How a single-player door opens (`SP_func_door`, `InitMoverTrData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Door {
    /// Where it opens to (`pos2`; for one that starts open, where it was placed).
    pub pos2: Vec3,
    /// Its travel time, ms (`trDuration`: its distance at "speed", 400 when 0, in
    /// whole ms; at least 1).
    pub duration: i32,
    /// "linear": it moves at one speed (`TR_LINEAR_STOP`), not slowing at the end
    /// (`TR_NONLINEAR_STOP`).
    pub linear: bool,
    /// "delay" (whole seconds), in ms between being used and starting.
    pub delay: i32,
    /// "team": the doors that open with it, the first in spawn order leading
    /// (`G_FindTeams`).
    pub team: Option<String>,
    /// It spawns a trigger an NPC opens by standing in it (`Think_SpawnNewDoorTrigger`,
    /// `Touch_DoorTrigger`): no "targetname" or "health", not locked (16), and not
    /// force-activated (2), used by the player (64), inactive (128), opened by a key
    /// NPCs lack (32) or a toggle (8: touched on, it would close again a frame after
    /// opening).
    pub touched: bool,
}

/// A door moving from `from` to `to` over `duration` ms from game time `start`.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    pub start: i32,
    pub from: Vec3,
    pub to: Vec3,
    pub duration: i32,
    pub linear: bool,
}

/// `MOVER_LOCKED`: a door locked until used, its shader animation frame-controlled.
const MOVER_LOCKED: i64 = 16;
/// The door flags that keep a touch from opening it: `MOVER_FORCE_ACTIVATE`,
/// `MOVER_TOGGLE`, `MOVER_LOCKED`, `MOVER_GOODIE`, `MOVER_PLAYER_USE`,
/// `MOVER_INACTIVE`.
const UNTOUCHED: i64 = 2 | 8 | MOVER_LOCKED | 32 | 64 | 128;
/// How far a door's trigger reaches past it along its thinnest axis
/// (`Think_SpawnNewDoorTrigger`).
const TRIGGER_REACH: f32 = 120.0;
/// A door used starts moving 50 ms later (`Use_BinaryMover_Go`).
const USE_LAG: i32 = 50;

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
    // CMod_LoadSubmodels spreads the model's bounds by a unit each way; the game sees
    // those (SetBrushModel).
    let (mins, maxs) = (m.mins.map(|x| x - 1.0), m.maxs.map(|x| x + 1.0));
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
    let (mut spin, mut door) = (None, None);
    match class.as_str() {
        "func_door" => {
            // pos2 = pos1 moved along movedir by the model's size there, less the
            // lip; "start_open" (spawnflag 1) starts at pos2.
            let dir = movedir(angles);
            let size = [0, 1, 2].map(|i| maxs[i] - mins[i]);
            let distance = (0..3).map(|i| dir[i].abs() * size[i]).sum::<f32>() - lip();
            let mut pos2 = [0, 1, 2].map(|i| origin[i] + distance * dir[i]);
            if spawnflags & 1 != 0 {
                (origin, pos2) = (pos2, origin);
            }
            shader_anim = spawnflags & MOVER_LOCKED != 0;
            if let Rules::SinglePlayer { .. } = rules {
                let speed = match e.get("speed").map_or(0.0, atof) {
                    0.0 => 400.0,
                    s => s,
                };
                let length = (0..3)
                    .map(|i| (pos2[i] - origin[i]) * (pos2[i] - origin[i]))
                    .sum::<f32>()
                    .sqrt();
                door = Some(Door {
                    pos2,
                    duration: ((length * 1000.0 / speed) as i32).max(1),
                    linear: e.get("linear").map_or(0, atoi) != 0,
                    delay: e.get("delay").map_or(0, atoi).saturating_mul(1000) as i32,
                    team: e.get("team").map(str::to_owned),
                    touched: spawnflags & UNTOUCHED == 0
                        && e.get("targetname").is_none()
                        && e.get("health").map_or(0, atoi) == 0,
                });
            }
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
        "func_rotating" => spin = rotating_spin(e, spawnflags, rules),
        "func_glass" => {}
        "func_breakable" => broadcast = force_visible,
        "func_wall" if spawnflags & 1 == 0 => {}
        "func_usable" if spawnflags & 1 == 0 => {
            broadcast = force_visible;
            shader_anim = e.get("endframe").map_or(0, atoi) > 0;
        }
        _ => return None,
    }
    // SV_LinkEntity: the model's box at the origin, or a cube of the model's radius
    // when it is turned (a turning one is relinked at its angles every frame);
    // an epsilon larger.
    let entity_box = if drawn_angles == [0.0; 3] && spin.is_none() {
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
        spin,
        door,
        slide: None,
    })
}

/// Opens the single-player doors `entities` holds whose trigger one of `bodies` (an
/// NPC's box where it stands) touches at game time `at`, its first think; they stay
/// open while it does.
///
/// A team's first door spawns the trigger, around the whole team's linked boxes
/// and reaching [`TRIGGER_REACH`] further each way along their thinnest axis
/// (`Think_SpawnNewDoorTrigger`); a touch (`G_TouchTriggersLerped`,
/// `SV_EntityContact`) uses its leader ("delay" later), and the team moves together,
/// each door over its own time, from 50 ms later (`Use_BinaryMover_Go`,
/// `MatchTeam`). Each touch of an open door puts off its closing, so it stays
/// open. Moving, a door is linked over its whole way.
pub fn touch_doors(entities: &mut [BrushEntity], bodies: &[[Vec3; 2]], at: i32) {
    for lead in 0..entities.len() {
        let Some(door) = &entities[lead].door else {
            continue;
        };
        let in_team = |e: &BrushEntity| {
            door.team.is_some() && e.door.as_ref().is_some_and(|d| d.team == door.team)
        };
        if entities[..lead].iter().any(in_team) || !door.touched {
            continue;
        }
        let team: Vec<usize> = (lead..entities.len())
            .filter(|&i| i == lead || in_team(&entities[i]))
            .collect();
        let start = at + door.delay + USE_LAG;
        let mut trigger = entities[lead].entity_box;
        for &i in &team {
            for k in 0..3 {
                trigger[0][k] = trigger[0][k].min(entities[i].entity_box[0][k]);
                trigger[1][k] = trigger[1][k].max(entities[i].entity_box[1][k]);
            }
        }
        let size = |k: usize| trigger[1][k] - trigger[0][k];
        let thinnest = (1..3).fold(0, |best, k| if size(k) < size(best) { k } else { best });
        trigger[0][thinnest] -= TRIGGER_REACH;
        trigger[1][thinnest] += TRIGGER_REACH;
        let touches = |[mins, maxs]: &[Vec3; 2]| {
            (0..3).all(|k| mins[k] <= trigger[1][k] && maxs[k] >= trigger[0][k])
        };
        if !bodies.iter().any(touches) {
            continue;
        }
        for i in team {
            let e = &mut entities[i];
            let Some(door) = &e.door else { continue };
            let moved = [0, 1, 2].map(|k| door.pos2[k] - e.origin[k]);
            for k in 0..3 {
                e.entity_box[0][k] += moved[k].min(0.0);
                e.entity_box[1][k] += moved[k].max(0.0);
            }
            e.slide = Some(Slide {
                start,
                from: e.origin,
                to: door.pos2,
                duration: door.duration,
                linear: door.linear,
            });
        }
    }
}

/// `SP_func_rotating`: "speed" degrees a second (100 when 0) about yaw, or roll
/// (spawnflag 4), or pitch (spawnflag 8). Single player turns only those that start
/// on (spawnflag 1); multiplayer turns them all, by "spinangles" when given.
fn rotating_spin(e: &Entity, spawnflags: i64, rules: Rules) -> Option<Vec3> {
    if let Rules::SinglePlayer { .. } = rules
        && spawnflags & 1 == 0
    {
        return None;
    }
    if let (Rules::Multiplayer { .. }, Some(spin)) = (rules, e.get("spinangles")) {
        return Some(vector_prefix(spin));
    }
    let speed = match e.get("speed").map_or(0.0, atof) {
        0.0 => 100.0,
        s => s,
    };
    Some(if spawnflags & 4 != 0 {
        [0.0, 0.0, speed]
    } else if spawnflags & 8 != 0 {
        [speed, 0.0, 0.0]
    } else {
        [0.0, speed, 0.0]
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
            color: [0; 4],
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
    /// Its angles at game time `ms` (`BG_EvaluateTrajectory`'s `TR_LINEAR`, in
    /// floats as the original), or `None` if it does not turn.
    pub fn angles_at(&self, ms: i32) -> Option<Vec3> {
        let seconds = ms as f32 * 0.001;
        self.spin.map(|d| d.map(|x| seconds * x))
    }

    /// Where it is drawn from at game time `ms` (`EvaluateTrajectory`'s
    /// `TR_NONLINEAR_STOP` or `TR_LINEAR_STOP`, in floats as the original); once
    /// there, it stands at its end (`Reached_BinaryMover`).
    pub fn origin_at(&self, ms: i32) -> Vec3 {
        let Some(s) = &self.slide else {
            return self.origin;
        };
        if ms >= s.start + s.duration {
            return s.to;
        }
        let elapsed = ms - s.start;
        if elapsed <= 0 {
            return s.from;
        }
        let duration = s.duration as f32;
        let seconds = if s.linear {
            elapsed as f32 * 0.001
        } else {
            let degrees = 90.0 - 90.0 * elapsed as f32 / duration;
            duration * 0.001 * (degrees * (std::f32::consts::PI / 180.0)).cos()
        };
        // trDelta: the way, a second (SetMoverState).
        let per_second = (1000.0 / f64::from(s.duration)) as f32;
        [0, 1, 2].map(|k| s.from[k] + seconds * ((s.to[k] - s.from[k]) * per_second))
    }

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
