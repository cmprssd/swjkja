//! What single-player NPCs do as time passes while they stand with no enemy: their
//! movement frames (the stance they return to, the idle they play after standing
//! still), the spawnscripts they run (`NPC_Begin` runs a script's first pass,
//! `NPC_Think` the rest, once a server frame), and the map's scripts that run with
//! them (`target_scriptrunner`, such as the academies' intros, which their player
//! starts as it enters the world); `code/icarus/TaskManager.cpp`, `Sequencer.cpp`;
//! the game's side in `Q3_Interface.cpp`.
//!
//! The game is stepped a server frame at a time in the original's order: in each
//! frame, by entity number ([`crate::entity_number`]), each script runner's script
//! and each NPC's movement frame (`Pmove`: its animation timers count down, then
//! its stance returns to the parts whose timers ran out; then `G_CheckClientIdle`)
//! and script; a script runs command after command until one waits. Frames in which nothing can change are skipped.
//!
//! What scripts do to NPCs is modelled: their animations and hold times, what they
//! look at (`SET_LOOK_TARGET`), the way they turn (`SET_DYAW`, `SET_DPITCH`,
//! `SET_VIEWTARGET`, at `SET_YAWSPEED`: `NPC_UpdateAngles` turns their view a step
//! a frame, which a frozen AI's think still does, and the task completes the frame
//! it finds the view there), whether they are drawn with more light
//! (`SET_MORELIGHT`), the tasks scripts wait for (an animation's completes
//! when its timer runs out, a voice line's when it has played), timed waits, loops,
//! conditions, task groups, signals, `use` of other script runners (and the uses
//! a level pins: what its triggers and buttons would use, [`Level::uses`]), `affect`,
//! which hands a block to an NPC (`FLUSH` drops the rest of its script, `INSERT`
//! runs the block first) and runs it at once, and `run`, which runs another script
//! in place (yavin1's opening runs its intro). The brush entities and breakable
//! models scripts name move where `move` sends them (`Lerp2Pos`: to a `ref_tag` or a
//! point, over its time, turning to its angles if it gives them), their task
//! completing on the mover's first turn there, and turn as `rotate` turns them
//! (`Lerp2Angles`), its task completing as the turn's time runs out. An NPC walks to
//! the navgoal a script sets (`SET_NAVGOAL`), its AI running until it gets there
//! ([`Walk`]). Other commands that move an NPC (`SET_ORIGIN`, ...) are left out
//! (those that complete later never do, as with the AI frozen), and so are cameras
//! (but when one is first enabled)
//! and the other commands. A random number is its range's low end
//! (once the client runs, what the shared generator draws depends on frame timing:
//! [`crate::rand`]); a `get` reads nothing (a condition with one is false).

use std::collections::HashMap;

use swjkja_icarus::{Block, Member, Node, id};
use swjkja_model::animation::Animation;

use crate::Vec3;
use crate::brush_entity::{Motion, Slide, slid};
use crate::collision::{CONTENTS_BODY, CONTENTS_BOTCLIP, Collision, MASK_NPCSOLID, Trace};
use crate::nav::{self, Graph, Nearest};
use crate::static_model::{Pose, angle_normalize180, atof, vectoangles};

/// The level time (ms) of the server's first frame on a map: four settling frames
/// from 1000 ms 100 ms apart and one more at 1400 ms leave the clock at 1500, and
/// each frame then runs 50 ms later (`SV_SpawnServer`, `SV_Frame` at `sv_fps` 20).
/// A map's NPCs begin on it (spawners think 350 ms after spawning, `NPC_Begin` 100
/// ms after that).
pub const FIRST_FRAME: i32 = 1550;
/// Milliseconds between server frames.
pub const FRAME_MS: i32 = 50;
/// The milliseconds `NPC_Begin`'s movement frame counts the first pass's timers down
/// by: its command's time is 0, which `ClientThink` raises to the level's time less
/// 1000 (550 ms), against a `commandTime` of 0, and `Pmove` caps it at 200. Later
/// frames count 50 ms each (the AI frozen, as in the captures, its commands' times
/// are 1000 ms behind the level's too).
const BEGIN_MSEC: i32 = 200;
/// How much later than the frame that starts it a voice line plays out, beyond its
/// length (ms). The client plays a line on its own clock and the mixer runs in real
/// time, so the original's lag varies from line to line; 32 to 35 ms ends five of
/// academy4's first six traced lines and academy3's first on the frames the traces
/// have them (DECISIONS.md, 2026-10-30): a server frame less a client frame at the
/// captures' 60 fps. The game sees a line stopped (`G_CheckTasksCompleted`: the
/// mixer's `s_entityWavVol` back to 0) on its first frame after.
const VOICE_LAG: i32 = 33;
/// Milliseconds a part blends from its previous animation into a new one
/// (`SETANIM_BLEND_DEFAULT`, which `NPC_SetAnim` and the stance's `PM_SetAnim`
/// pass).
pub const BLEND_MS: i32 = 100;

/// How long the timeline runs (ms of game time); the last animations then hold.
pub const HORIZON: i32 = 3_600_000;

/// The animation an NPC starts in (`NPC_Begin`).
const STAND: &str = "BOTH_STAND1";

/// One part's change of animation: what it plays from `start` (ms, on the clock the
/// client draws by) and by which clock, and what it blends from, the animation
/// before and its clock, if it changed after the first frame
/// (`G2_Set_Bone_Anim_Index` keeps the old animation's frame for [`BLEND_MS`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub start: i32,
    pub anim: String,
    pub from: Option<(String, Clock)>,
    pub clock: Clock,
}

/// How a bone times its animation (`boneInfo_t`): when it played its first frame
/// (`startTime`, ms: earlier than the change when it carries on from a frame
/// reached at another speed) and its speed in frames per 50 ms (`animSpeed`), or
/// `None` for the animation's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    pub begun: i32,
    pub speed: Option<f32>,
}

impl Clock {
    /// From `start`, at the animation's own speed.
    pub fn from(start: i32) -> Self {
        Self {
            begun: start,
            speed: None,
        }
    }
}

/// The legs' and the torso's animations over time, from the first frame, and what
/// the NPC looks at.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    pub legs: Vec<Change>,
    pub torso: Vec<Change>,
    /// Each change of look target (`SET_LOOK_TARGET`): from when (ms, on the
    /// client's clock, as an animation set in the same frame) and the name of the
    /// entity it looks at, or `None` once cleared ("none", "NULL").
    pub look: Vec<(i32, Option<String>)>,
    /// Each change of `SCF_MORELIGHT` (`SET_MORELIGHT`), which cgame reads as it
    /// draws: from when (ms, on the client's clock) and whether it is set.
    pub more_light: Vec<(i32, bool)>,
    /// Each change of whether the NPC holds no weapon (`SET_WEAPON`: "WP_NONE"
    /// drops every bolted model, `G_RemoveWeaponModels`; another weapon brings one
    /// back): from when (ms, on the client's clock) and whether it is unarmed.
    pub unarmed: Vec<(i32, bool)>,
    /// Its yaw (`ps.viewangles`) after each level frame that turns it: the level
    /// time and the yaw (degrees, -180..180).
    pub yaw: Vec<(i32, f32)>,
    /// How far it has moved from where it began after each level frame that moves
    /// it (walking to a navgoal): the level time and the offset.
    pub moves: Vec<(i32, Vec3)>,
    /// Its velocity (`ps.velocity`) and whether it backs up (`PMF_BACKWARDS_RUN`)
    /// after each level frame that changes either: the level time, the velocity
    /// and the flag. cgame turns its legs toward it ([`Timeline::legs_turns`]).
    pub velocity: Vec<(i32, Vec3, bool)>,
}

impl Timeline {
    /// Whether the NPC is drawn with more light (`RF_MORELIGHT`) at `time`.
    pub fn more_light_at(&self, time: i32) -> bool {
        let after = self.more_light.partition_point(|(start, _)| *start <= time);
        after.checked_sub(1).is_some_and(|i| self.more_light[i].1)
    }

    /// Whether the NPC holds no weapon at `time`, its bolted models dropped.
    pub fn unarmed_at(&self, time: i32) -> bool {
        let after = self.unarmed.partition_point(|(start, _)| *start <= time);
        after.checked_sub(1).is_some_and(|i| self.unarmed[i].1)
    }

    /// Its turns, for an NPC that begins facing `yaw`, as cgame draws them: from
    /// each frame's yaw to the next over the 50 ms between their snapshots
    /// (`CG_InterpolateEntityPosition`'s `LerpAngle`, the shorter way round), as
    /// angles (pitch and roll 0: its legs, `CG_G2PlayerAngles`).
    pub fn turns(&self, yaw: f32) -> Vec<Slide> {
        let mut from = yaw;
        self.yaw
            .iter()
            .map(|&(level, to)| {
                let to = from + angle_subtract(to, from);
                let slide = Slide {
                    start: level - FRAME_MS,
                    from: [0.0, from, 0.0],
                    to: [0.0, to, 0.0],
                    duration: FRAME_MS,
                    linear: true,
                };
                from = to;
                slide
            })
            .collect()
    }

    /// Its legs' turns, for an NPC that begins facing `yaw`, as cgame draws them
    /// once they have caught up: each frame's yaw ([`Timeline::turns`]) turned
    /// toward the velocity of the frame after it, which the server has run as the
    /// client draws the frame ([`legs_offset`]), from frame to frame over the 50 ms
    /// between their snapshots. cgame moves the legs part of the way there each
    /// rendered frame (`CG_PlayerLegsYawFromMovement`, by the frame's
    /// interpolation, 5 degrees at most), at a speed that depends on its frame
    /// rate; once the capture has stopped the game they get there.
    pub fn legs_turns(&self, yaw: f32) -> Vec<Slide> {
        let mut levels: Vec<i32> = self.yaw.iter().map(|y| y.0).collect();
        levels.extend(self.velocity.iter().map(|v| v.0 - FRAME_MS));
        levels.sort_unstable();
        levels.dedup();
        let (mut view, mut from) = (yaw, yaw);
        let mut yaws = self.yaw.iter().peekable();
        let mut turns = Vec::new();
        for level in levels {
            while let Some(&&(l, to)) = yaws.peek()
                && l <= level
            {
                view += angle_subtract(to, view);
                yaws.next();
            }
            let after = self.velocity.partition_point(|v| v.0 <= level + FRAME_MS);
            let (velocity, backwards) = after.checked_sub(1).map_or(([0.0; 3], false), |i| {
                (self.velocity[i].1, self.velocity[i].2)
            });
            let legs = view + legs_offset(view, velocity, backwards);
            let to = from + angle_subtract(legs, from);
            if to != from {
                turns.push(Slide {
                    start: level - FRAME_MS,
                    from: [0.0, from, 0.0],
                    to: [0.0, to, 0.0],
                    duration: FRAME_MS,
                    linear: true,
                });
                from = to;
            }
        }
        turns
    }

    /// Its moves, for an NPC drawn at `origin` as it begins, as cgame draws them:
    /// from each frame's origin to the next over the 50 ms between their snapshots
    /// (`CG_InterpolateEntityPosition`).
    pub fn slides(&self, origin: Vec3) -> Vec<Slide> {
        let mut from = origin;
        self.moves
            .iter()
            .map(|&(level, offset)| {
                let to = [0, 1, 2].map(|k| origin[k] + offset[k]);
                let slide = Slide {
                    start: level - FRAME_MS,
                    from,
                    to,
                    duration: FRAME_MS,
                    linear: true,
                };
                from = to;
                slide
            })
            .collect()
    }

    /// The name of what the NPC looks at at `time`, if anything.
    pub fn look_at(&self, time: i32) -> Option<&str> {
        let after = self.look.partition_point(|(start, _)| *start <= time);
        after.checked_sub(1).and_then(|i| self.look[i].1.as_deref())
    }

    /// What part `part` (0 the legs, 1 the torso) plays at `time`: its last change
    /// started by then (its first before the first frame).
    pub fn at(&self, part: usize, time: i32) -> &Change {
        let changes = if part == 0 { &self.legs } else { &self.torso };
        let after = changes.partition_point(|c| c.start <= time);
        &changes[after.saturating_sub(1)]
    }

    /// What part `part` blends from at `time`, by which clock, and how far the
    /// blend has gone (0 to 1), or `None` outside a blend.
    pub fn blending_at(&self, part: usize, time: i32) -> Option<(&str, Clock, f32)> {
        let c = self.at(part, time);
        let elapsed = time - c.start;
        let (anim, clock) = c.from.as_ref()?;
        (0..BLEND_MS)
            .contains(&elapsed)
            .then(|| (anim.as_str(), *clock, elapsed as f32 / BLEND_MS as f32))
    }
}

/// An animation as the NPC's skeleton has it (its animation file's line), or
/// `None` when the skeleton lacks it (`PM_HasAnimation`).
pub type Holds<'a> = &'a dyn Fn(&str) -> Option<Animation>;

/// Task slots an animation's completion is kept in (`TID_ANIM_BOTH`, `_LOWER`,
/// `_UPPER`).
const BOTH: usize = 0;
const LOWER: usize = 1;
const UPPER: usize = 2;
/// The slot a turn's completion is kept in (`TID_ANGLE_FACE`).
const FACE: usize = 3;
/// The slot a navgoal's task is kept in (`TID_MOVE_NAV`).
const NAV: usize = 4;

/// `MIN_ANGLE_ERROR`: a view this close to where it is turning to is there.
const MIN_ANGLE_ERROR: f32 = 0.01;
/// How far a script may pitch an NPC's view (`Q3_SetDPitch`: one degree inside
/// its head's range, `headPitchRangeUp` and `Down`, 45 unless its block says).
const PITCH_RANGE: f32 = 44.0;

/// `PM_StandingAnim`.
fn standing(anim: &str) -> bool {
    matches!(
        anim,
        "BOTH_STAND1" | "BOTH_STAND2" | "BOTH_STAND3" | "BOTH_STAND4" | "BOTH_ATTACK3"
    )
}

/// `G_CheckClientIdle`'s idle for legs playing `anim` (`BOTH_STAND2` picks one of
/// two at random: the first).
fn idle_for(anim: &str) -> Option<&'static str> {
    match anim {
        "BOTH_STAND1" => Some("BOTH_STAND1IDLE1"),
        "BOTH_STAND2" => Some("BOTH_STAND2IDLE1"),
        "BOTH_STAND3" => Some("BOTH_STAND3IDLE1"),
        "BOTH_STAND5" => Some("BOTH_STAND5IDLE1"),
        _ => None,
    }
}

/// The `set` commands whose task the game completes later (those `Set` returns from
/// without calling back) and that nothing here completes: they move the NPC or
/// wait on its AI.
fn never_completes(name: &str) -> bool {
    matches!(
        name,
        "SET_ANIMFRAME"
            | "SET_BEHAVIOR_STATE"
            | "SET_ENDFRAME"
            | "SET_LOCATION"
            | "SET_TELEPORT_DEST"
            | "SET_TEMP_BSTATE"
            | "SET_WIDTH"
    )
}

/// A member's value as the game reads it: text, a float (`%.3f`, as `Set` and
/// conditions get it), or a random number's low end; `None` for a `get`, a tag or a
/// vector. Advances `at` past it.
fn value(members: &[Member], at: &mut usize) -> Option<Value> {
    let m = members.get(*at)?;
    *at += 1;
    match m.id {
        id::RANDOM => {
            let low = members.get(*at)?.float();
            *at += 2;
            low.map(Value::Float)
        }
        id::FLOAT => m.float().map(Value::Float),
        id::GET => {
            *at += 2;
            None
        }
        id::TAG => {
            *at += 2;
            None
        }
        id::VECTOR => {
            *at += 3;
            None
        }
        _ => m.text().map(|t| Value::Text(t.to_owned())),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Text(String),
    Float(f32),
}

impl Value {
    /// As the game's `Set` gets it: a float printed with three decimals.
    fn text(&self) -> String {
        match self {
            Value::Text(t) => t.clone(),
            Value::Float(f) => format!("{f:.3}"),
        }
    }

    fn float(&self) -> Option<f32> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Text(_) => None,
        }
    }
}

/// `atoi`.
fn atoi(s: &str) -> i32 {
    crate::style::atoi(s) as i32
}

/// `CSequencer::EvaluateConditional` with `Q3_Evaluate`: two operands compared by
/// `>`, `<`, `==` or `!=`, as floats when both are numbers, as text otherwise
/// (equality only). False when an operand is not read here.
fn condition(b: &Block) -> bool {
    let mut at = 0;
    let first = value(&b.members, &mut at);
    let op = b.members.get(at).map(|m| m.id);
    at += 1;
    let second = value(&b.members, &mut at);
    let (Some(a), Some(op), Some(b)) = (first, op, second) else {
        return false;
    };
    match (a.float(), b.float()) {
        (Some(a), Some(b)) => match op {
            id::GREATER_THAN => a > b,
            id::LESS_THAN => a < b,
            id::EQUALS => a == b,
            id::NOT => a != b,
            _ => false,
        },
        _ => match op {
            id::EQUALS => a.text().eq_ignore_ascii_case(&b.text()),
            id::NOT => !a.text().eq_ignore_ascii_case(&b.text()),
            _ => false,
        },
    }
}

/// One part's animation as the game holds it.
struct Part {
    anim: String,
    clock: Clock,
    /// `legsAnimTimer` / `torsoAnimTimer`: ms left to hold, 0 for none, -1 forever.
    timer: i32,
}

/// Where a script is: a body being run, how far, and what it is.
struct Cursor<'a> {
    nodes: &'a [Node],
    at: usize,
    kind: Kind,
}

enum Kind {
    Plain,
    /// A loop with its iterations left (-1 forever).
    Loop(i32),
    /// A task group's body, run by `do`: its commands are the group's tasks.
    Group(usize),
}

/// A `task` block: its name, body, and the tasks its last `do` started, each with
/// whether it has completed.
struct Group<'a> {
    name: &'a str,
    body: &'a [Node],
    tasks: Vec<(u32, bool)>,
}

/// What an NPC's body does: its animations, what it looks at, its idle clock and
/// the animation tasks it holds.
struct Body<'a> {
    stance: &'a Pose,
    holds: Holds<'a>,
    parts: [Part; 2],
    changes: [Vec<Change>; 2],
    look: Vec<(i32, Option<String>)>,
    more_light: Vec<(i32, bool)>,
    unarmed: Vec<(i32, bool)>,
    /// `client->idleTime`.
    idle_time: i32,
    /// The task each slot (the animations', the turn's, the navgoal's) waits on,
    /// if any.
    slots: [Option<u32>; 5],
    /// Its view's pitch and yaw (`ps.viewangles`) and where it turns them to
    /// (`NPCInfo->desiredPitch`, `desiredYaw`).
    view: [f32; 2],
    desired: [f32; 2],
    /// `stats.yawSpeed`.
    yaw_speed: f32,
    yaw: Vec<(i32, f32)>,
    walk: Walk,
    moves: Vec<(i32, Vec3)>,
    velocity: Vec<(i32, Vec3, bool)>,
}

/// Milliseconds between an unfrozen NPC's behavior thinks (`nextBStateThink`,
/// `FRAMETIME`); on the frames between, its last command plays again.
const BSTATE_MS: i32 = 100;
/// How much a think can change the velocity it steers by (`STEER::Activate`'s
/// `mMaxForce`).
const MAX_FORCE: f32 = 150.0;
/// `ps.friction` (`ClientSpawn`), `pm_accelerate` and `pm_stopspeed`.
const FRICTION: f32 = 6.0;
const ACCELERATE: f32 = 12.0;
const STOP_SPEED: f32 = 100.0;
/// `STEPSIZE`: how far above its feet a walk's path trace sweeps its box
/// (`MoveTrace`).
const STEPSIZE: f32 = 18.0;
/// `OVERCLIP`: how much more than touching a velocity is clipped off a plane.
const OVERCLIP: f32 = 1.001;

/// An NPC's walk to the navgoal a script sets (`SET_NAVGOAL`) and its movement.
/// The captures freeze the AI but for an NPC whose navgoal's task is pending
/// (`d_npcfreeze 3`), which thinks as in the game: every [`BSTATE_MS`] its behavior
/// (`NPC_BSDefault`) completes the task once it has reached the goal, or steers it
/// there (`NPC_MoveToGoal`: straight, or by the nav graph); the frames between
/// replay its last command.
#[derive(Debug, Default)]
struct Walk {
    /// Where it is (`currentOrigin`) and its velocity (`ps.velocity`).
    origin: Vec3,
    velocity: Vec3,
    /// The navgoal (`tempGoal->currentOrigin`) and its radius (`goalRadius`), while
    /// one is set.
    goal: Option<(Vec3, f32)>,
    /// `ps.moveDir` and `ps.speed` as its last think left them.
    dir: Vec3,
    speed: f32,
    /// `lastPathAngles`' yaw: its velocity's when it last steered with any speed.
    path_yaw: f32,
    /// How fast its last movement frame moved it (`resultspeed`, units a second).
    result_speed: f32,
    /// When its behavior next thinks (`nextBStateThink`).
    next_think: i32,
    /// `SCF_RUNNING`: it runs to its goals, else walks (`NPC_BSDefault` presses
    /// `BUTTON_WALKING` unless it runs).
    running: bool,
    /// When its path is next traced ("SafeToGoToCHECK") and until when a clear
    /// trace keeps it safe ("SafeToGoToDURATION").
    check: Option<i32>,
    safe_until: Option<i32>,
    /// The command its last think gave (`ucmd`, replayed until the next):
    /// `forwardmove`, `rightmove`, and whether it presses `BUTTON_WALKING`.
    forward: i32,
    right: i32,
    walking: bool,
    /// `PMF_BACKWARDS_RUN`: the last command that moved it forward or back moved
    /// it back.
    backwards: bool,
    /// The nav point it was last found nearest (`waypoint`).
    waypoint: Option<Nearest>,
    /// The side it steers around things by (`avoidSide`), the side it last
    /// steered by (`lastAvoidSteerSide`) and until when that side holds
    /// (`lastAvoidSteerSideDebouncer`).
    avoid_side: Side,
    last_avoid_side: Side,
    avoid_until: i32,
}

/// A side to steer by (`ESide`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Side {
    #[default]
    None,
    Left,
    Right,
}

/// Another NPC as a steering trace meets it (`CONTENTS_BODY`): its actor, its box
/// where it stands, and its velocity.
struct Obstacle {
    actor: usize,
    mins: Vec3,
    maxs: Vec3,
    velocity: Vec3,
}

/// An NPC's steering as one think builds it (`SSteerUser`, from
/// `STEER::Activate`): where it is, its velocity and speed, the speed it steers
/// at most, the steering so far, where it last sought (`mSeekLocation`) and the
/// next point of its path, the points it projects to avoid collisions, and the
/// NPC those leave out (`mIgnoreEntity`).
struct Steer {
    position: Vec3,
    velocity: Vec3,
    speed: f32,
    max_speed: f32,
    steering: Vec3,
    seek_at: Vec3,
    next: Option<Vec3>,
    project_fwd: Vec3,
    project_side: Vec3,
    ignore: Option<usize>,
}

impl Steer {
    /// `STEER::Seek` of `pos`, slowing within `slowing` (none at 0), weighted by
    /// `weight`: the velocity there at its speed (a height under 10 left out) less
    /// its own, added to the steering. How far `pos` is.
    fn seek(&mut self, pos: Vec3, slowing: f32, weight: f32) -> f32 {
        self.seek_at = pos;
        let mut desired = sub(pos, self.position);
        if desired[2].abs() < 10.0 {
            desired[2] = 0.0;
        }
        let dist = length(desired);
        if dist > 1e-10 {
            let mut speed = self.max_speed;
            if slowing != 0.0 && dist < slowing {
                speed *= dist / slowing;
            }
            desired = desired.map(|x| x / dist * speed);
        } else {
            desired = [0.0; 3];
        }
        for k in 0..3 {
            self.steering[k] += (desired[k] - self.velocity[k]) * weight;
        }
        if dist > 1e-10 { dist } else { 0.0 }
    }
}

/// How long an NPC keeps steering around things by the same side: 500 to 2000 ms
/// in the original (`Q_irand(500, STEER::SIDE_LOCKED_TIMER)`), a draw not
/// replayed here; t1_sour's traced stand-in needs at least 1300.
const SIDE_LOCKED_MS: i32 = 2000;

/// `MIN_WALK_NORMAL`: steeper planes cannot be walked on.
const MIN_WALK_NORMAL: f32 = 0.7;

/// `MoveTrace` of a box from `start` to `end` through `world` and `bodies`
/// (`CONTENTS_BODY`), botclip too unless it starts in it: whether it is clear,
/// the trace, and the NPC it hit.
fn move_trace_bodies(
    world: &Collision,
    bodies: &[Obstacle],
    start: Vec3,
    end: Vec3,
    mins: Vec3,
    maxs: Vec3,
) -> (bool, Trace, Option<usize>) {
    let run = |mask: i32| {
        let (tr, hit) = trace_bodies(world, bodies, start, end, mins, maxs, mask);
        (
            !tr.all_solid && !tr.start_solid && tr.fraction == 1.0,
            tr,
            hit,
        )
    };
    let first = run(MASK_NPCSOLID | CONTENTS_BOTCLIP);
    if !first.0 && (first.1.all_solid || first.1.start_solid) {
        return run(MASK_NPCSOLID);
    }
    first
}

/// `SV_Trace`: the world first, then, up to where it stops, each of `bodies` (an
/// entity box, `SV_ClipMoveToEntities`), the nearest kept; the NPC hit.
fn trace_bodies(
    world: &Collision,
    bodies: &[Obstacle],
    start: Vec3,
    end: Vec3,
    mins: Vec3,
    maxs: Vec3,
    mask: i32,
) -> (Trace, Option<usize>) {
    let mut tr = world.trace(start, end, mins, maxs, mask);
    if tr.fraction == 0.0 || mask & CONTENTS_BODY == 0 {
        return (tr, None);
    }
    let world_fraction = tr.fraction;
    let short = tr.end;
    tr.fraction = 1.0;
    let mut hit = None;
    for b in bodies {
        let mut c = Collision::default();
        c.add_box(b.mins, b.maxs, CONTENTS_BODY);
        let t = c.trace(start, short, mins, maxs, mask);
        if t.all_solid {
            if !tr.all_solid {
                hit = Some(b.actor);
            }
            tr.all_solid = true;
        } else if t.start_solid {
            if !tr.start_solid {
                hit = Some(b.actor);
            }
            tr.start_solid = true;
        }
        if t.fraction < tr.fraction {
            let start_solid = tr.start_solid;
            tr = t;
            tr.start_solid |= start_solid;
            hit = Some(b.actor);
        }
    }
    tr.fraction *= world_fraction;
    (tr, hit)
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `CVec3::Norm`.
fn normalize(v: Vec3) -> Vec3 {
    let l = length(v);
    v.map(|x| x / l)
}

/// The node of a nearest point, if not an edge.
fn node(n: Nearest) -> Option<usize> {
    match n {
        Nearest::Node(i) => Some(i),
        Nearest::Edge(_) => None,
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [0, 1, 2].map(|k| a[k] - b[k])
}

fn length(v: Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// `CVec3::Truncate`: `v` cut to `max` long; its length after.
fn truncate(v: &mut Vec3, max: f32) -> f32 {
    let len = length(*v);
    if len > max && len > 1e-10 {
        let scale = max / len;
        *v = v.map(|x| x * scale);
        return max;
    }
    len
}

/// An entity scripts move: where it spawns, its moves and turns, and the tasks
/// waiting on them.
struct Moving {
    origin: Vec3,
    angles: Vec3,
    linear: bool,
    motion: Motion,
    /// The move under way: the level time it ends, and whether it turns too
    /// (`reachedF_moveAndRotateCallback`).
    moving: Option<(i32, bool)>,
    /// When a `rotate`'s `anglerCallback` thinks (`nextthink`).
    think: Option<i32>,
    /// The tasks waiting on it: `TID_MOVE_NAV` and `TID_ANGLE_FACE`.
    tasks: [Option<u32>; 2],
}

/// [`Moving::tasks`]'s slots.
const MOVE_NAV: usize = 0;
const ANGLE_FACE: usize = 1;

impl Moving {
    /// `currentOrigin` at level time `level`.
    fn at(&self, level: i32) -> Vec3 {
        self.motion
            .slides
            .last()
            .map_or(self.origin, |s| s.at(level))
    }

    /// `currentAngles` at level time `level`.
    fn angles_at(&self, level: i32) -> Vec3 {
        slid(&self.motion.turns, level).unwrap_or(self.angles)
    }
}

/// `AngleSubtract`: `a1 - a2` in -180..=180.
fn angle_subtract(a1: f32, a2: f32) -> f32 {
    let mut a = (a1 - a2) % 360.0;
    while a > 180.0 {
        a -= 360.0;
    }
    while a < -180.0 {
        a += 360.0;
    }
    a
}

/// `AngleDelta`: `angle1 - angle2` normalized to -180..=180 in 65536ths of a turn
/// (`AngleNormalize180`).
fn angle_delta(angle1: f32, angle2: f32) -> f32 {
    let a =
        (360.0f32 / 65536.0) * (((angle1 - angle2) * (65536.0f32 / 360.0)) as i32 & 65535) as f32;
    if a > 180.0 { a - 360.0 } else { a }
}

/// How far cgame turns the legs of an NPC facing `yaw` from it toward its
/// `velocity` once they have caught up (`CG_PlayerLegsYawFromMovement`): to the
/// velocity's yaw, within 60 degrees, the other way when it backs up
/// (`PMF_BACKWARDS_RUN`); not at all when it stands still or moves more than 150
/// degrees from where it faces.
pub fn legs_offset(yaw: f32, velocity: Vec3, backwards: bool) -> f32 {
    if velocity[0] == 0.0 && velocity[1] == 0.0 {
        return 0.0;
    }
    let offset = -angle_delta(yaw, vectoangles(velocity)[1]);
    if offset.abs() > 150.0 {
        return 0.0;
    }
    let offset = offset.clamp(-60.0, 60.0);
    if backwards { -offset } else { offset }
}

/// An entity that runs scripts (its `CSequencer` and `CTaskManager`): an NPC, with
/// a body, a script runner, or a mover.
struct Actor<'a> {
    /// The name scripts find it by (`script_targetname`).
    name: Option<&'a str>,
    body: Option<Body<'a>>,
    mover: Option<Moving>,
    groups: Vec<Group<'a>>,
    stack: Vec<Cursor<'a>>,
    /// When the current timed wait started (`CTask` time stamp).
    wait_since: Option<i32>,
    /// The voice line playing (`TID_CHAN_VOICE`): its task and the level time it
    /// has played out by ([`VOICE_LAG`] after its start, plus its length).
    voice: Option<(u32, i32)>,
}

/// When an animation started in level frame `level` starts for the client: one
/// frame earlier, but in the frames before the client draws its first (up to the
/// one after the player begins), where its clock is still at 0 and the level's time
/// is taken (`PM_SetAnimFinal`'s `cg.time ? cg.time : level.time`), and in the two
/// frames after, while the client is still drawing its first: its clock stays at
/// that frame's time.
fn client_time(level: i32) -> i32 {
    let first_drawn = PLAYER_BEGIN + FRAME_MS;
    if level <= first_drawn {
        level
    } else if level <= first_drawn + 2 * FRAME_MS {
        first_drawn
    } else {
        level - FRAME_MS
    }
}

impl<'a> Body<'a> {
    /// Whether its view is where it turns to (`NPC_UpdateAngles`' `exact`).
    fn exact(&self) -> bool {
        (0..2).all(|k| angle_delta(self.view[k], self.desired[k]).abs() <= MIN_ANGLE_ERROR)
    }

    /// `NPC_UpdateAngles` and the movement frame's `PM_UpdateViewAngles` in level
    /// frame `level`: each angle off by more than [`MIN_ANGLE_ERROR`] turns toward
    /// where it is going by `(60 + 3 yawSpeed) / 20` degrees (pitch at the same
    /// speed), the view kept in 65536ths of a turn (the command's angles, a
    /// `short`); a yaw it turns is kept. Returns whether it was already there. With
    /// `apply` false its view stays (an unfrozen NPC between its thinks: the command
    /// it replays keeps the angles of its last).
    fn update_angles(&mut self, level: i32, apply: bool) -> bool {
        let exact = self.exact();
        if !apply {
            return exact;
        }
        let yaw = self.view[1];
        let decay = (60.0 + self.yaw_speed * 3.0) * (50.0 / 1000.0);
        for k in 0..2 {
            let target = self.desired[k];
            let mut error = angle_delta(self.view[k], target);
            if error.abs() > MIN_ANGLE_ERROR {
                error = if error < 0.0 {
                    (error + decay).min(0.0)
                } else {
                    (error - decay).max(0.0)
                };
            }
            // ANGLE2SHORT, then SHORT2ANGLE of the short.
            let short = ((target + error) * 65536.0 / 360.0) as i32 as i16;
            self.view[k] = short as f32 * (360.0 / 65536.0);
        }
        if !exact && self.view[1] != yaw {
            self.yaw.push((level, self.view[1]));
        }
        exact
    }

    /// `Q3_SetDYaw` (no NPC here has an enemy).
    fn set_yaw(&mut self, yaw: f32) {
        self.desired[1] = yaw;
    }

    /// `Q3_SetDPitch`: within [`PITCH_RANGE`] of level.
    fn set_pitch(&mut self, pitch: f32) {
        let pitch = angle_normalize180(pitch);
        self.desired[0] = if pitch < -1.0 {
            pitch.max(-PITCH_RANGE)
        } else if pitch > 1.0 {
            pitch.min(PITCH_RANGE)
        } else {
            pitch
        };
    }

    /// `STEER::Reached`: whether `goal` is within `radius` of its origin or inside
    /// its box (`absmin`, `absmax`: a unit larger all round).
    fn reached(&self, goal: Vec3, radius: f32) -> bool {
        let at = self.walk.origin;
        let (mins, maxs) = (self.stance.mins, self.stance.maxs);
        let d = sub(goal, at);
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < radius * radius
            || (0..3).all(|k| goal[k] > at[k] + mins[k] - 1.0 && goal[k] < at[k] + maxs[k] + 1.0)
    }

    /// `STEER::SafeToGoTo` at level time `level` on the nav graph `nav`: within 110
    /// units (and 50 up or down) of `goal`; within 500 when its nearest point and
    /// the goal's neighbor and each stands within the safe radius of its own
    /// (`NAV::OnNeighboringPoints`, `NAV::InSafeRadius`); or within 400 while a trace
    /// of its path keeps it safe (one every 1.5 s, `MoveTrace` of its box raised by
    /// [`STEPSIZE`] from below, past every NPC: a clear one keeps it safe for the
    /// 2 s after, but not on the think that traced, which takes the graph). Without
    /// a graph (a level the caller gave none) the graph's test is left out and the
    /// think that traces clear goes straight too.
    fn safe_to_go(
        &mut self,
        goal: Vec3,
        level: i32,
        world: &Collision,
        nav: Option<&Graph>,
    ) -> bool {
        let at = self.walk.origin;
        let dist = length(sub(goal, at));
        if dist < 110.0 && (goal[2] - at[2]).abs() < 50.0 {
            return true;
        }
        if dist >= 500.0 {
            return false;
        }
        if let Some(g) = nav {
            let target = g.nearest(world, goal, None, None);
            let near = self.nearest_point(g, world, target.and_then(node));
            let neighbors = match (near, target) {
                (Some(Nearest::Node(a)), Some(Nearest::Node(b))) => {
                    g.neighboring(a, b)
                        && g.in_safe_radius(at, a, b)
                        && g.in_safe_radius(goal, b, a)
                }
                _ => false,
            };
            if neighbors {
                return true;
            }
        }
        if dist >= 400.0 {
            return false;
        }
        let w = &mut self.walk;
        if w.safe_until.is_some_and(|t| t >= level) {
            return true;
        }
        if w.check.is_some_and(|t| t >= level) {
            return false;
        }
        w.check = Some(level + 1500);
        let mut mins = self.stance.mins;
        mins[2] += STEPSIZE;
        let clear = nav::move_trace(world, at, goal, mins, self.stance.maxs, true);
        if clear {
            w.safe_until = Some(level + 2000);
        }
        clear && nav.is_none()
    }

    /// `NAV::GetNearestNode` for it, now (`forceRecalcNow`), toward `goal`: from the
    /// point it was last nearest (an edge's is not passed on), which this becomes.
    fn nearest_point(
        &mut self,
        g: &Graph,
        world: &Collision,
        goal: Option<usize>,
    ) -> Option<Nearest> {
        let previous = self.walk.waypoint.and_then(node);
        self.walk.waypoint = g.nearest(world, self.walk.origin, previous, goal);
        self.walk.waypoint
    }

    /// `NAV::GoTo` toward `goal` on the graph `g` at level time `level`: the
    /// shortest path from its nearest point to the goal's (`NAV::FindPath`; on an
    /// edge, its first point, where the original draws one of the two), its next
    /// point sought (`STEER::Path`, slowing within that point's radius), then
    /// collisions avoided. Whether it has a path. The path is found anew on each
    /// think that takes the graph (`NAV::UpdatePath` is left out): the thinks
    /// between go straight.
    fn nav_go_to(
        &mut self,
        s: &mut Steer,
        g: &Graph,
        goal: Vec3,
        level: i32,
        world: &Collision,
        bodies: &[Obstacle],
    ) -> bool {
        let first = |n: Nearest| match n {
            Nearest::Node(i) => i,
            Nearest::Edge(e) => g.edges[e].a,
        };
        let Some(target) = g.nearest(world, goal, None, None).map(first) else {
            return false;
        };
        let Some(start) = self.nearest_point(g, world, Some(target)).map(first) else {
            return false;
        };
        let (mins, maxs) = (self.stance.mins, self.stance.maxs);
        let path = g.find_path(world, self.walk.origin, mins, maxs, start, target);
        let Some(next) = path.as_ref().and_then(|p| p.last()) else {
            return false;
        };
        s.next = Some(next.point);
        if s.seek(next.point, next.slowing_radius, 1.0) == 0.0 {
            return false;
        }
        self.avoid_collisions(s, level, world, bodies);
        true
    }

    /// `STEER::AvoidCollisions` at level time `level` (none ignored: no NPC here
    /// greets, fights or leads another): its velocity as the steering so far would
    /// leave it, traced a second ahead (`TestCollision`), then 0.3 s ahead and 40
    /// degrees toward the side it avoids by (straight ahead again while it has
    /// none), half as strongly; a side found clear swaps the side. Whether it hit
    /// something (`STEER::Blocked`, left out: it acts once blocked for 2 s).
    fn avoid_collisions(
        &mut self,
        s: &mut Steer,
        level: i32,
        world: &Collision,
        bodies: &[Obstacle],
    ) -> bool {
        s.ignore = None;
        let mut steering = s.steering;
        let mut velocity = s.velocity;
        let mut speed = s.speed;
        if truncate(&mut steering, MAX_FORCE) > 1e-10 {
            velocity = [0, 1, 2].map(|k| velocity[k] + steering[k]);
            speed = truncate(&mut velocity, s.max_speed);
        }
        if speed <= 0.01 {
            return false;
        }
        let side = self.walk.avoid_side;
        let turn = if side == Side::Left { 40.0f32 } else { -40.0 }.to_radians();
        let (sin, cos) = turn.sin_cos();
        let aside = [
            velocity[0] * cos - velocity[1] * sin,
            velocity[0] * sin + velocity[1] * cos,
            velocity[2],
        ];
        s.project_fwd = [0, 1, 2].map(|k| s.position[k] + velocity[k]);
        s.project_side = [0, 1, 2].map(|k| s.position[k] + aside[k] * 0.3);
        let front = self.test_collision(s, velocity, speed, Side::None, 1.0, level, world, bodies);
        let beside = self.test_collision(s, velocity, speed, side, 0.5, level, world, bodies);
        if !beside {
            self.walk.avoid_side = if side == Side::Left {
                Side::Right
            } else {
                Side::Left
            };
        }
        front || beside
    }

    /// `TestCollision` of its move toward `side`'s projected point (`Side::None`:
    /// ahead) at `velocity` and `speed`, weighted by `weight`: whether it would
    /// hit something. Ahead, an NPC that moves slower its way is followed at its
    /// speed and one standing on its next point is stopped behind; a shallow slope
    /// is safe. Else it steers past what it hit, toward the side the plane leans
    /// its way, kept for a while ([`SIDE_LOCKED_MS`]).
    /// Doors are not told apart from the world (the original passes through them).
    #[allow(clippy::too_many_arguments)]
    fn test_collision(
        &mut self,
        s: &mut Steer,
        velocity: Vec3,
        speed: f32,
        side: Side,
        weight: f32,
        level: i32,
        world: &Collision,
        bodies: &[Obstacle],
    ) -> bool {
        let to = if side == Side::None {
            s.project_fwd
        } else {
            s.project_side
        };
        let mut mins = self.stance.mins;
        mins[2] += STEPSIZE;
        let (mut safe, tr, hit) =
            move_trace_bodies(world, bodies, s.position, to, mins, self.stance.maxs);
        if hit.is_some() && hit == s.ignore {
            safe = true;
        }
        if safe || side != Side::None {
            return !safe;
        }
        if let Some(b) = hit.and_then(|h| bodies.iter().find(|b| b.actor == h)) {
            let contact_speed = length(b.velocity);
            if contact_speed > 0.01 {
                if contact_speed < speed {
                    let similar = dot(normalize(velocity), normalize(b.velocity)).abs();
                    if similar > 0.5 {
                        let mut desired = s.velocity;
                        truncate(&mut desired, contact_speed);
                        for k in 0..3 {
                            s.steering[k] += (desired[k] - velocity[k]) * similar;
                        }
                        s.ignore = Some(b.actor);
                        safe = true;
                    }
                }
            } else {
                let next = s.next.unwrap_or(s.seek_at);
                if (0..3).all(|k| next[k] > b.mins[k] - 1.0 && next[k] < b.maxs[k] + 1.0) {
                    for k in 0..3 {
                        s.steering[k] -= velocity[k];
                    }
                    s.ignore = Some(b.actor);
                    safe = true;
                }
            }
        }
        if !safe && tr.normal[2] > 0.0 && tr.normal[2] < MIN_WALK_NORMAL {
            safe = true;
        }
        let w = &mut self.walk;
        if safe {
            w.last_avoid_side = Side::None;
            return false;
        }
        // The plane's normal crossed with up: along the plane.
        let mut along = [tr.normal[1], -tr.normal[0], 0.0];
        if w.last_avoid_side != Side::None && w.avoid_until < level {
            w.last_avoid_side = Side::None;
            w.avoid_until = level + SIDE_LOCKED_MS;
        }
        if w.last_avoid_side == Side::Right
            || w.last_avoid_side == Side::None && dot(along, normalize(velocity)) < 0.0
        {
            along = along.map(|x| -x);
            w.last_avoid_side = Side::Right;
        } else {
            w.last_avoid_side = Side::Left;
        }
        let out = speed * weight * 0.5;
        let contact = [0, 1, 2].map(|k| tr.end[k] + along[k] * out);
        // The original passes the weight as the slowing distance.
        s.seek(contact, weight, 1.0);
        true
    }

    /// `NPC_MoveToGoal` toward `goal` (radius `radius`) at level time `level`:
    /// straight there (`STEER::GoTo`: `Seek`, slowing within four times the radius,
    /// at its walking or running speed, then collisions avoided) when that is safe,
    /// else by the nav graph `nav` (`NAV::GoTo`), else it stops (`STEER::Stop`).
    /// Then `STEER::DeActivate`: the steering, at most [`MAX_FORCE`], added to its
    /// velocity at most that speed, sets its move direction, its speed (none under
    /// 10; three quarters while it backs up, a whole number), the yaw it turns to
    /// and, with any speed, its command (walking under its walking speed and 5).
    /// `bodies` are the other NPCs.
    fn steer(
        &mut self,
        goal: Vec3,
        radius: f32,
        level: i32,
        world: &Collision,
        nav: Option<&Graph>,
        bodies: &[Obstacle],
    ) {
        let mut s = Steer {
            position: self.walk.origin,
            velocity: self.walk.velocity,
            speed: length(self.walk.velocity),
            max_speed: if self.walk.running {
                self.stance.run_speed
            } else {
                self.stance.walk_speed
            },
            steering: [0.0; 3],
            seek_at: self.walk.origin,
            next: None,
            project_fwd: self.walk.origin,
            project_side: self.walk.origin,
            ignore: None,
        };
        if self.safe_to_go(goal, level, world, nav) {
            s.seek(goal, radius * 4.0, 1.0);
            self.avoid_collisions(&mut s, level, world, bodies);
        } else if !nav.is_some_and(|g| self.nav_go_to(&mut s, g, goal, level, world, bodies)) {
            for k in 0..3 {
                s.steering[k] -= s.velocity[k];
            }
        }
        let w = &mut self.walk;
        let mut velocity = s.velocity;
        let mut speed = s.speed;
        if truncate(&mut s.steering, MAX_FORCE) > 1e-10 {
            velocity = [0, 1, 2].map(|k| velocity[k] + s.steering[k]);
            speed = truncate(&mut velocity, s.max_speed);
        }
        if speed < 10.0 {
            speed = 0.0;
        }
        w.dir = velocity;
        if speed > 0.0 && velocity != [0.0; 3] {
            let len = length(velocity);
            w.dir = velocity.map(|x| x / len);
            w.path_yaw = vectoangles(velocity)[1];
        }
        self.desired = [0.0, w.path_yaw];
        // ps.speed is a whole number.
        w.speed = speed as i32 as f32;
        if speed > 0.0 {
            // The command, from its current angles (AngleVectors' forward and right).
            let yaw = self.view[1].to_radians();
            let dot = |x: f32, y: f32| (x * w.dir[0] + y * w.dir[1]) * 127.0;
            w.forward = dot(yaw.cos(), yaw.sin()).clamp(-127.0, 127.0).floor() as i32;
            w.right = dot(yaw.sin(), -yaw.cos()).clamp(-127.0, 127.0).floor() as i32;
            w.walking = speed < self.stance.walk_speed + 5.0;
            if w.forward < 0 {
                w.speed = (w.speed * 0.75) as i32 as f32;
            }
        }
    }

    /// `PM_WalkMove` on level ground for `msec`: friction (`PM_Friction`: at least
    /// [`STOP_SPEED`] slows by [`FRICTION`] a second), then acceleration along its
    /// move direction toward its speed (`PM_Accelerate`, [`ACCELERATE`]); its box
    /// then sweeps as far as it can go (`PM_StepSlideMove`: a wall stops it there,
    /// its velocity clipped along it). Whether it moved.
    fn walk_move(&mut self, msec: i32, world: &Collision) -> bool {
        let w = &mut self.walk;
        let time = msec as f32 * 0.001;
        let v = &mut w.velocity;
        let speed = (v[0] * v[0] + v[1] * v[1]).sqrt();
        if speed < 1.0 {
            v[0] = 0.0;
            v[1] = 0.0;
        } else {
            let drop = speed.max(STOP_SPEED) * FRICTION * time;
            let scale = (speed - drop).max(0.0) / speed;
            *v = v.map(|x| x * scale);
        }
        let current = v[0] * w.dir[0] + v[1] * w.dir[1] + v[2] * w.dir[2];
        let add = w.speed - current;
        if add > 0.0 {
            let accel = (ACCELERATE * time * w.speed).min(add);
            *v = [0, 1, 2].map(|k| v[k] + accel * w.dir[k]);
        }
        if v[0] == 0.0 && v[1] == 0.0 {
            return false;
        }
        let end = [0, 1, 2].map(|k| w.origin[k] + v[k] * time);
        let tr = world.trace(
            w.origin,
            end,
            self.stance.mins,
            self.stance.maxs,
            MASK_NPCSOLID,
        );
        if tr.fraction < 1.0 {
            // PM_ClipVelocity.
            let n = tr.normal;
            let mut backoff = v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
            backoff = if backoff < 0.0 {
                backoff * OVERCLIP
            } else {
                backoff / OVERCLIP
            };
            *v = [0, 1, 2].map(|k| v[k] - n[k] * backoff);
        }
        let moved = tr.end != w.origin;
        w.origin = tr.end;
        moved
    }

    /// `PM_SetAnimFinal` for part `part` (0 the legs, 1 the torso) in level frame
    /// `level`: it plays `anim` at [`Body::anim_speed`] from its first frame, but
    /// for a part already playing it, unless `restart`: that carries on from the
    /// frame it has reached when the speed changes (`G2_Set_Bone_Anim_Index`'s
    /// `setFrame`, which moves its start back), and is left alone otherwise. A part
    /// set without `restart` while the other plays `anim` at another frame takes
    /// up the other's (`g_synchSplitAnims`). A part that carries on does not blend,
    /// and ends a blend in progress. Nothing when the skeleton lacks `anim`.
    fn play(&mut self, part: usize, anim: &str, level: i32, restart: bool) {
        let Some(a) = (self.holds)(anim) else { return };
        let time = client_time(level);
        let speed = self.anim_speed(anim, &a);
        let new_speed = speed.unwrap_or(a.speed());
        let playing = |p: &Part| p.clock.speed.unwrap_or(a.speed());
        let reached =
            |p: &Part| (p.anim == anim).then(|| a.frame_reached(time - p.clock.begun, playing(p)));
        let (mine, other) = (reached(&self.parts[part]), reached(&self.parts[1 - part]));
        let frame = match (restart, mine, other) {
            (false, _, Some(other)) if mine != Some(other) => Some(other),
            (false, Some(_), _) if playing(&self.parts[part]) == new_speed => return,
            (false, mine, _) => mine,
            (true, ..) => None,
        };
        let begun = frame.map_or(time, |f| {
            let back = f64::from(f - a.first as f32) * 50.0 / f64::from(new_speed);
            (f64::from(time) - back) as i32
        });
        let blends = restart || mine.is_none();
        let clock = Clock { begun, speed };
        let p = &mut self.parts[part];
        let from = (blends && level != FIRST_FRAME).then(|| (p.anim.clone(), p.clock));
        p.anim = anim.to_owned();
        p.clock = clock;
        self.changes[part].push(Change {
            start: time,
            anim: anim.to_owned(),
            from,
            clock,
        });
    }

    /// The speed it plays `anim` (`a`) at, `None` for the animation's own: with
    /// `g_noFootSlide`, a walk or run played forward goes as much faster or slower
    /// than its own as the body's last movement frame went than the animation's
    /// stride ([`Pose::strides`]; 75 for a crouched walk), from 0.01 to 1.5. Not for
    /// the classes without strides, nor for dual and staff saber styles' strides
    /// (left out).
    fn anim_speed(&self, anim: &str, a: &Animation) -> Option<f32> {
        let walking = matches!(
            anim,
            "BOTH_WALK1"
                | "BOTH_WALK2"
                | "BOTH_WALK_STAFF"
                | "BOTH_WALK_DUAL"
                | "BOTH_WALK5"
                | "BOTH_WALK6"
                | "BOTH_WALK7"
                | "BOTH_WALKBACK1"
                | "BOTH_WALKBACK2"
                | "BOTH_WALKBACK_STAFF"
                | "BOTH_WALKBACK_DUAL"
        );
        let running = matches!(
            anim,
            "BOTH_RUN1"
                | "BOTH_RUN2"
                | "BOTH_RUN4"
                | "BOTH_RUN_STAFF"
                | "BOTH_RUN_DUAL"
                | "BOTH_RUNBACK1"
                | "BOTH_RUNBACK2"
                | "BOTH_RUNBACK_STAFF"
                | "BOTH_RUN1START"
                | "BOTH_RUN1STOP"
                | "BOTH_RUNSTRAFE_LEFT1"
                | "BOTH_RUNSTRAFE_RIGHT1"
        );
        let crouched = matches!(anim, "BOTH_CROUCH1WALK" | "BOTH_CROUCH1WALKBACK");
        let [walk, run] = self.stance.strides?;
        let stride = match (crouched, walking, running) {
            (true, ..) => 75.0,
            (_, true, _) => walk,
            (_, _, true) => run,
            _ => return None,
        };
        let speed = a.speed();
        (speed >= 0.0).then(|| (speed * (self.walk.result_speed / stride)).clamp(0.01, 1.5))
    }
}

impl<'a> Actor<'a> {
    fn new(name: Option<&'a str>, body: Option<Body<'a>>, script: Option<&'a [Node]>) -> Self {
        let mut actor = Self {
            name,
            body,
            mover: None,
            groups: Vec::new(),
            stack: Vec::new(),
            wait_since: None,
            voice: None,
        };
        if let Some(nodes) = script {
            actor.run(nodes);
        }
        actor
    }

    /// Starts `nodes` as its script, registering their task groups.
    fn run(&mut self, nodes: &'a [Node]) {
        groups(nodes, &mut self.groups);
        self.stack = vec![Cursor {
            nodes,
            at: 0,
            kind: Kind::Plain,
        }];
        self.wait_since = None;
    }

    /// `CSequencer::Affect` with `body`: `TYPE_FLUSH` drops the rest of its script
    /// for it, `TYPE_INSERT` runs it first and then goes on with the script (a wait
    /// it was in starts over).
    fn affect(&mut self, body: &'a [Node], insert: bool) {
        groups(body, &mut self.groups);
        if !insert {
            self.stack.clear();
        }
        self.stack.push(Cursor {
            nodes: body,
            at: 0,
            kind: Kind::Plain,
        });
        self.wait_since = None;
    }

    /// `Q3_TaskIDComplete` for the task `id`: it completes, and every slot waiting on
    /// it is cleared.
    fn complete(&mut self, id: u32) {
        for g in &mut self.groups {
            for t in &mut g.tasks {
                if t.0 == id {
                    t.1 = true;
                }
            }
        }
        if let Some(body) = &mut self.body {
            for s in &mut body.slots {
                if *s == Some(id) {
                    *s = None;
                }
            }
        }
    }

    /// `Q3_TaskIDSet`: a task stomped by another completes.
    fn slot(&mut self, slot: usize, id: u32) {
        let Some(body) = &mut self.body else { return };
        if let Some(old) = body.slots[slot].replace(id) {
            self.complete(old);
            self.body.as_mut().unwrap().slots[slot] = Some(id);
        }
    }

    /// `PM_SetLegsAnimTimer` / `PM_SetTorsoAnimTimer`: a timer that runs out
    /// completes the animation task waiting on its part (both parts' when both wait).
    fn set_timer(&mut self, part: usize, time: i32) {
        let body = self.body.as_mut().unwrap();
        let t = &mut body.parts[part].timer;
        *t = if time < 0 && time != -1 { 0 } else { time };
        let (mine, other) = if part == 0 {
            (LOWER, UPPER)
        } else {
            (UPPER, LOWER)
        };
        if *t != 0 {
            return;
        }
        let Some(task) = body.slots[mine] else { return };
        match body.slots[BOTH] {
            None => self.complete(task),
            Some(both) => {
                body.slots[mine] = None;
                if body.slots[other].is_none() {
                    self.complete(both);
                }
            }
        }
    }

    /// `NPC_SetAnim` with `SETANIM_FLAG_OVERRIDE | SETANIM_FLAG_HOLD` and `restart`
    /// for the parts `parts` (0 legs, 1 torso): their timers drop to 0, then each
    /// part plays `anim` (unless it already does, without `restart`), held for the
    /// animation's hold. False when the skeleton lacks it.
    fn set_held(&mut self, parts: &[usize], anim: &str, restart: bool, level: i32) -> bool {
        let body = self.body.as_ref().unwrap();
        let Some(hold) = (body.holds)(anim).map(|a| a.hold()) else {
            return false;
        };
        let differs: Vec<bool> = parts
            .iter()
            .map(|&p| restart || body.parts[p].anim != anim)
            .collect();
        for (&p, &d) in parts.iter().zip(&differs) {
            if d {
                self.set_timer(p, 0);
            }
        }
        for (&p, &d) in parts.iter().zip(&differs) {
            // An animation already playing is not played again, nor held anew.
            if d {
                self.body.as_mut().unwrap().play(p, anim, level, restart);
                self.set_timer(p, hold);
            }
        }
        true
    }

    /// The movement frame of level frame `level`, `msec` after the last: the timers
    /// count down (`PM_DropTimers`), it walks ([`Body::walk_move`]), parts no longer
    /// held play what its command asks (`PM_Footsteps`, `PM_TorsoAnimation`, which
    /// play an animation the skeleton has, without restarting it), then
    /// `G_CheckClientIdle`. Nothing for an actor without a body.
    ///
    /// A command that moves it walks or runs its legs, forward or back
    /// (`BOTH_WALK1`, `BOTH_RUN1`, `BOTH_WALKBACK1`, `BOTH_RUNBACK1`), its torso
    /// with them, but for an armed NPC backing up (its weapon's animation, not
    /// modelled: it keeps its stance's); otherwise both parts return to the stance.
    /// The classes that walk or run otherwise (the ATST, rancor, wampa, jawa, Galak
    /// mech) and lit sabers are not modelled.
    fn movement(&mut self, level: i32, msec: i32, world: &Collision) {
        if self.body.is_none() {
            return;
        }
        for p in 0..2 {
            let timer = self.body.as_ref().unwrap().parts[p].timer;
            if timer > 0 {
                self.set_timer(p, (timer - msec).max(0));
            }
        }
        let body = self.body.as_mut().unwrap();
        let w = &body.walk;
        let before = w.origin;
        if (w.velocity != [0.0; 3] || w.dir != [0.0; 3]) && body.walk_move(msec, world) {
            let offset = sub(body.walk.origin, body.stance.origin);
            body.moves.push((level, offset));
        }
        let moved = length(sub(body.walk.origin, before));
        body.walk.result_speed = moved / msec as f32 * 1000.0;
        let moving =
            body.walk.velocity != [0.0; 3] || (body.walk.dir != [0.0; 3] && body.walk.speed > 0.0);
        let w = &mut body.walk;
        if w.forward < 0 {
            w.backwards = true;
        } else if w.forward > 0 || w.right != 0 {
            w.backwards = false;
        }
        let heading = (w.velocity, w.backwards);
        if body
            .velocity
            .last()
            .map_or(([0.0; 3], false), |v| (v.1, v.2))
            != heading
        {
            body.velocity.push((level, heading.0, heading.1));
        }
        let w = &mut body.walk;
        let legs = match (w.forward != 0 || w.right != 0, w.backwards, w.walking) {
            (false, ..) => body.stance.legs,
            (true, false, true) => "BOTH_WALK1",
            (true, false, false) => "BOTH_RUN1",
            (true, true, true) => "BOTH_WALKBACK1",
            (true, true, false) => "BOTH_RUNBACK1",
        };
        if body.parts[0].timer == 0 {
            body.play(0, legs, level, false);
        }
        let legs = body.parts[0].anim.as_str();
        let unarmed = body.unarmed.last().map_or(body.stance.unarmed, |u| u.1);
        let torso = match legs {
            "BOTH_WALK1" | "BOTH_RUN1" => legs,
            "BOTH_WALKBACK1" | "BOTH_RUNBACK1" if unarmed => legs,
            _ => body.stance.torso,
        };
        if body.parts[1].timer == 0 {
            let torso = torso.to_owned();
            body.play(1, &torso, level, false);
        }
        let [legs, torso] = &body.parts;
        let mut idle = None;
        if moving || !standing(&legs.anim) || legs.timer != 0 || torso.timer != 0 {
            body.idle_time = body.idle_time.max(level);
        } else if level - body.idle_time > 5000
            && let Some(anim) = idle_for(&legs.anim)
            && let Some(a) = (body.holds)(anim)
        {
            idle = Some((anim, a.length()));
        }
        if let Some((anim, length)) = idle {
            self.set_held(&[0, 1], anim, false, level);
            self.body.as_mut().unwrap().idle_time = level + length;
        }
    }

    /// One `set` command, as task `task`, on map `map`: whether its task stays
    /// pending. An actor without a body completes every one.
    fn set(
        &mut self,
        name: &str,
        value: Option<&Value>,
        task: u32,
        level: i32,
        map: &Level,
    ) -> bool {
        if self.body.is_none() {
            return false;
        }
        let text = value.map(Value::text).unwrap_or_default();
        match name {
            "SET_ANIM_BOTH" => {
                // Q3_SetAnimUpper, then Q3_SetAnimLower: each restarts its part.
                let upper = self.set_held(&[1], &text, true, level);
                if upper {
                    self.slot(UPPER, task);
                }
                let lower = self.set_held(&[0], &text, true, level);
                if lower {
                    self.slot(LOWER, task);
                }
                if upper && lower {
                    self.slot(BOTH, task);
                }
                upper || lower
            }
            "SET_ANIM_UPPER" | "SET_ANIM_LOWER" => {
                let (part, slot) = if name == "SET_ANIM_UPPER" {
                    (1, UPPER)
                } else {
                    (0, LOWER)
                };
                let set = self.set_held(&[part], &text, true, level);
                if set {
                    self.body.as_mut().unwrap().slots[BOTH] = None;
                    self.slot(slot, task);
                }
                set
            }
            "SET_ANIM_HOLDTIME_BOTH" => {
                let time = atoi(&text);
                self.set_timer(1, time);
                self.set_timer(0, time);
                for s in [BOTH, UPPER, LOWER] {
                    self.slot(s, task);
                }
                true
            }
            "SET_ANIM_HOLDTIME_UPPER" | "SET_ANIM_HOLDTIME_LOWER" => {
                let (part, slot) = if name == "SET_ANIM_HOLDTIME_UPPER" {
                    (1, UPPER)
                } else {
                    (0, LOWER)
                };
                self.set_timer(part, atoi(&text));
                self.body.as_mut().unwrap().slots[BOTH] = None;
                self.slot(slot, task);
                true
            }
            "SET_LOOK_TARGET" => {
                // Q3_LookTarget: "none" or "NULL" clears it.
                let cleared =
                    text.eq_ignore_ascii_case("none") || text.eq_ignore_ascii_case("NULL");
                let target = (!cleared).then_some(text);
                let body = self.body.as_mut().unwrap();
                if body.look.last().map(|(_, t)| t) != Some(&target) {
                    body.look.push((client_time(level), target));
                }
                false
            }
            "SET_MORELIGHT" => {
                // Q3_SetMoreLight: anything but "true" clears it.
                let on = text.eq_ignore_ascii_case("true");
                let body = self.body.as_mut().unwrap();
                if body.more_light.last().map(|(_, m)| *m).unwrap_or(false) != on {
                    body.more_light.push((client_time(level), on));
                }
                false
            }
            "SET_WEAPON" => {
                // Q3_SetWeapon: "WP_NONE" (G_SetWeapon) and "drop" remove the
                // weapon's models.
                let off = text.eq_ignore_ascii_case("WP_NONE") || text.eq_ignore_ascii_case("drop");
                let body = self.body.as_mut().unwrap();
                if body.unarmed.last().map(|(_, u)| *u).unwrap_or(false) != off {
                    body.unarmed.push((client_time(level), off));
                }
                false
            }
            "SET_DYAW" | "SET_DPITCH" => {
                let angle = atof(&text);
                let body = self.body.as_mut().unwrap();
                if name == "SET_DYAW" {
                    body.set_yaw(angle);
                } else {
                    body.set_pitch(angle);
                }
                self.slot(FACE, task);
                true
            }
            "SET_YAWSPEED" => {
                self.body.as_mut().unwrap().yaw_speed = atof(&text);
                false
            }
            "SET_RUNNING" => {
                // Q3_SetRunning: anything but "true" clears it.
                self.body.as_mut().unwrap().walk.running = text.eq_ignore_ascii_case("true");
                false
            }
            "SET_NAVGOAL" => self.set_nav_goal(&text, map, task),
            _ => never_completes(name),
        }
    }

    /// `Q3_SetNavGoal`, as task `task`: the goal is where the tag `name` names and
    /// its radius ([`Level::radii`]), the task completing as it gets there; "null"
    /// clears the goal, completing its task. Whether the task stays pending (a tag
    /// not found completes it; entities that are not tags are not found here).
    fn set_nav_goal(&mut self, name: &str, map: &Level, task: u32) -> bool {
        if name.eq_ignore_ascii_case("null") {
            self.body.as_mut().unwrap().walk.goal = None;
            if let Some(old) = self.body.as_mut().unwrap().slots[NAV].take() {
                self.complete(old);
            }
            return false;
        }
        let key = name.to_ascii_lowercase();
        let Some(&goal) = map.tags.get(&key) else {
            return false;
        };
        let radius = map.radii.get(&key).copied().unwrap_or(0.0);
        self.body.as_mut().unwrap().walk.goal = Some((goal, radius));
        self.slot(NAV, task);
        true
    }

    /// The behavior think of level frame `level` of an NPC walking to a navgoal
    /// (its task pending: the AI then runs, [`Walk`]), every [`BSTATE_MS`]: once
    /// it has reached the goal (`UpdateGoal`) the goal clears and the task
    /// completes, else it steers there ([`Body::steer`]). Returns whether it
    /// thought, and whether it was unfrozen; a frozen NPC's move direction and
    /// command clear (`NPC_Think`).
    fn think(
        &mut self,
        level: i32,
        world: &Collision,
        nav: Option<&Graph>,
        bodies: &[Obstacle],
    ) -> (bool, bool) {
        let Some(body) = &mut self.body else {
            return (false, false);
        };
        let Some(task) = body.slots[NAV] else {
            body.walk.dir = [0.0; 3];
            (body.walk.forward, body.walk.right) = (0, 0);
            return (false, false);
        };
        if body.walk.next_think > level {
            return (false, true);
        }
        body.walk.next_think = level + BSTATE_MS;
        body.walk.dir = [0.0; 3];
        (body.walk.forward, body.walk.right) = (0, 0);
        let (goal, radius) = body.walk.goal.expect("a pending navgoal's goal");
        if body.reached(goal, radius) {
            body.walk.goal = None;
            body.slots[NAV] = None;
            self.complete(task);
        } else {
            body.steer(goal, radius, level, world, nav, bodies);
        }
        (true, true)
    }

    /// `Q3_SetViewTarget`, as task `task`: the view turns toward `target`'s eyes
    /// from its own (yaw and pitch), if found; the task waits for the turn either
    /// way. An actor without a body completes it.
    fn view_target(&mut self, target: Option<Vec3>, task: u32) -> bool {
        let Some(body) = &mut self.body else {
            return false;
        };
        if let Some(target) = target {
            let to = vectoangles([0, 1, 2].map(|k| target[k] - body.stance.eyes[k]));
            body.set_yaw(to[1]);
            body.set_pitch(to[0]);
        }
        self.slot(FACE, task);
        true
    }

    /// The turn of level frame `level` (`NPC_UpdateAngles`, before the movement
    /// frame; `apply` as [`Body::update_angles`]): a view already where it turns to
    /// completes the turn's task.
    fn turn(&mut self, level: i32, apply: bool) {
        let Some(body) = &mut self.body else { return };
        if body.update_angles(level, apply)
            && let Some(task) = body.slots[FACE]
        {
            self.complete(task);
        }
    }

    /// `Lerp2Pos` for `move` command `b`, as task `task` at level time `level`: it
    /// moves from where it is to the tag or point `b` names, over `b`'s time (1 ms
    /// at least), slowing at the end unless "linear", turning by the way to its
    /// angles if `b` gives them (`AngleDelta`); its task (in the current group)
    /// completes as it gets there, and those it stomps complete now. A tag not
    /// found fails the task: nothing moves.
    fn move_to(&mut self, b: &Block, tags: &HashMap<String, Vec3>, task: u32, level: i32) {
        let float = |n: usize| b.members.get(n).and_then(Member::float).unwrap_or(0.0);
        let (to, mut at) = match b.members.first().map(|m| m.id) {
            Some(id::TAG) => (tags.get(&text(b, 1).to_ascii_lowercase()).copied(), 3),
            Some(id::VECTOR) => (Some([float(1), float(2), float(3)]), 4),
            _ => (None, 0),
        };
        let angles = b
            .members
            .get(at)
            .is_some_and(|m| m.id == id::VECTOR)
            .then(|| [float(at + 1), float(at + 2), float(at + 3)]);
        if angles.is_some() {
            at += 4;
        }
        let Some(to) = to else { return };
        let duration = match float(at) as i32 {
            0 => 1,
            d => d,
        };
        if let Some(g) = self.current_group() {
            self.groups[g].tasks.push((task, false));
        }
        let m = self.mover.as_mut().unwrap();
        let slide = Slide {
            start: level,
            from: m.at(level),
            to,
            duration,
            linear: m.linear,
        };
        m.motion.slides.push(slide);
        m.moving = Some((level + duration, angles.is_some()));
        if let Some(angles) = angles {
            let from = m.angles_at(level);
            let turn = Slide {
                start: level,
                from,
                to: [0, 1, 2].map(|k| from[k] + angle_delta(angles[k], from[k])),
                duration,
                linear: m.linear,
            };
            m.motion.turns.push(turn);
            self.set_task(ANGLE_FACE, task);
        }
        self.set_task(MOVE_NAV, task);
    }

    /// `Lerp2Angles` for `rotate` command `b`, as task `task` at level time `level`:
    /// it turns from its angles to those `b` gives (`AngleSubtract`), over `b`'s time
    /// (1 ms at least), slowing at the end unless "linear"; its task (in the current
    /// group) completes as its time runs out (`anglerCallback`), and one it stomps
    /// completes now.
    fn rotate(&mut self, b: &Block, task: u32, level: i32) {
        let float = |n: usize| b.members.get(n).and_then(Member::float).unwrap_or(0.0);
        if b.members.first().map(|m| m.id) != Some(id::VECTOR) {
            return;
        }
        let angles = [float(1), float(2), float(3)];
        let time = float(4);
        if let Some(g) = self.current_group() {
            self.groups[g].tasks.push((task, false));
        }
        let m = self.mover.as_mut().unwrap();
        let from = m.angles_at(level);
        let turn = Slide {
            start: level,
            from,
            to: [0, 1, 2].map(|k| from[k] + angle_subtract(angles[k], from[k])),
            duration: if time > 0.0 { time as i32 } else { 1 },
            linear: m.linear,
        };
        m.motion.turns.push(turn);
        m.think = Some(level + time as i32);
        self.set_task(ANGLE_FACE, task);
    }

    /// `Q3_TaskIDSet` on a mover: a task stomped by another completes.
    fn set_task(&mut self, slot: usize, id: u32) {
        let m = self.mover.as_mut().unwrap();
        if let Some(old) = m.tasks[slot].replace(id) {
            self.complete(old);
        }
    }

    /// `Q3_TaskIDComplete` on a mover: the task in `slot`, if any, completes.
    fn complete_slot(&mut self, slot: usize) {
        if let Some(task) = self.mover.as_mut().unwrap().tasks[slot].take() {
            self.complete(task);
        }
    }

    /// A mover's turn at `level`: `G_MoverTeam`'s check, a move that has got there
    /// completing its tasks (its turn's too, if it turned), then its think, a
    /// `rotate` whose time has run out completing its task.
    fn reach(&mut self, level: i32) {
        let Some(m) = &mut self.mover else { return };
        if let Some((end, turned)) = m.moving
            && end <= level
        {
            m.moving = None;
            if turned {
                self.complete_slot(ANGLE_FACE);
            }
            self.complete_slot(MOVE_NAV);
        }
        let m = self.mover.as_mut().unwrap();
        if let Some(think) = m.think
            && think <= level
        {
            m.think = None;
            self.complete_slot(ANGLE_FACE);
        }
    }

    /// The group whose body the innermost `do` runs, if any.
    fn current_group(&self) -> Option<usize> {
        self.stack.iter().rev().find_map(|c| match c.kind {
            Kind::Group(g) => Some(g),
            _ => None,
        })
    }

    /// The first level frame after `level` at which something of its own can
    /// change, if any: a timer running out, a timed wait ending, a voice line
    /// playing out, or the idle starting.
    fn next_event(&self, level: i32) -> Option<i32> {
        let frame_from = |t: i32| level + ((t - level).max(1) + FRAME_MS - 1) / FRAME_MS * FRAME_MS;
        let mut next: Option<i32> = None;
        let mut at = |t: i32| next = Some(next.map_or(t, |n| n.min(t)));
        if let Some(since) = self.wait_since
            && let Some(top) = self.stack.last()
            && let Some(Node::Command(b)) = top.nodes.get(top.at)
        {
            let mut i = 0;
            if let Some(Value::Float(ms)) = value(&b.members, &mut i) {
                // The first frame later than since + ms.
                let end = (since as f32 + ms).floor() as i32 + 1;
                at(frame_from(end));
            }
        }
        if let Some((_, end)) = self.voice {
            at(frame_from(end));
        }
        if let Some(m) = &self.mover {
            for end in [m.moving.map(|(end, _)| end), m.think]
                .into_iter()
                .flatten()
            {
                at(frame_from(end));
            }
        }
        let Some(body) = &self.body else { return next };
        if body.slots[FACE].is_some() || !body.exact() {
            at(level + FRAME_MS);
        }
        if body.slots[NAV].is_some() || body.walk.velocity != [0.0; 3] {
            at(level + FRAME_MS);
        }
        for p in &body.parts {
            if p.timer > 0 {
                at(frame_from(level + p.timer));
            }
        }
        let [legs, torso] = &body.parts;
        let idle = idle_for(&legs.anim).and_then(|a| (body.holds)(a));
        if standing(&legs.anim) && legs.timer == 0 && torso.timer == 0 && idle.is_some() {
            at(frame_from(body.idle_time + 5001));
        }
        next
    }
}

/// Every `task` block in `nodes` and the bodies under them (`ParseTask` registers
/// each as the script is routed, before it runs), the first of a name kept; not
/// those under `affect`, which go to the entity it affects.
fn groups<'a>(nodes: &'a [Node], out: &mut Vec<Group<'a>>) {
    for n in nodes {
        if let Node::Body(b, body) = n {
            if b.id == id::AFFECT {
                continue;
            }
            if b.id == id::TASK
                && let Some(name) = b.members.first().and_then(Member::text)
                && !out.iter().any(|g| g.name.eq_ignore_ascii_case(name))
            {
                out.push(Group {
                    name,
                    body,
                    tasks: Vec::new(),
                });
            }
            groups(body, out);
        }
    }
}

/// Member `n` of `b` as text ("" if it is not).
fn text(b: &Block, n: usize) -> &str {
    b.members.get(n).and_then(Member::text).unwrap_or("")
}

/// `affect`'s type that runs the block before the rest of the entity's script
/// (`TYPE_INSERT`); the other (`TYPE_FLUSH`, 56) drops the rest.
const AFFECT_INSERT: f32 = 55.0;

/// `camera`'s type that hands the view to the camera (`TYPE_ENABLE`).
const CAMERA_ENABLE: f32 = 62.0;

/// Whether a `sound` command's channel is a voice's, whose task completes when the
/// line has played (`PlayIcarusSound`).
fn voice_channel(channel: &str) -> bool {
    ["CHAN_VOICE", "CHAN_VOICE_ATTEN", "CHAN_VOICE_GLOBAL"]
        .iter()
        .any(|c| c.eq_ignore_ascii_case(channel))
}

/// The level time (ms) the player enters the world on a map: the first frame
/// after the NPCs have begun that a fast load gives (the reference captures hold
/// the server there until the client enters, `sv_swjkjaBegin`). The spawn point's
/// targets are used then (`ClientSpawn`).
pub const PLAYER_BEGIN: i32 = 2000;

/// A map's `target_scriptrunner`: the name it is used by, its script (`usescript`),
/// how long after it is used the script starts (`delay`, ms), and its entity
/// number ([`crate::entity_number`]).
#[derive(Debug, Clone)]
pub struct Runner {
    pub targetname: Option<String>,
    pub script: std::sync::Arc<Vec<Node>>,
    pub delay: i32,
    pub number: i32,
}

/// An entity scripts can move (a brush entity, a breakable model): its name
/// (`script_targetname`), where it stands and its angles, and whether it moves at
/// one speed ("linear").
#[derive(Debug, Clone, PartialEq)]
pub struct Mover {
    pub name: String,
    pub origin: Vec3,
    pub angles: Vec3,
    pub linear: bool,
}

/// The scripts a map runs besides its NPCs' spawnscripts: its script runners, the
/// names its player's spawn point uses as the player begins, and the scripts the
/// runners' scripts `run` (by name, in lower case; those runs reach too); the
/// places its `ref_tag`s name (in lower case), and the movers scripts can name.
#[derive(Debug, Clone, Default)]
pub struct Level {
    pub runners: Vec<Runner>,
    pub begin: Vec<String>,
    pub runs: HashMap<String, std::sync::Arc<Vec<Node>>>,
    pub tags: HashMap<String, Vec3>,
    /// The radius of each tag (`TAG_GetRadius`): 16 for a `ref_tag`, a navgoal's
    /// "radius" (12 without one).
    pub radii: HashMap<String, f32>,
    /// The brushes NPCs walk among; the caller fills them.
    pub collision: Collision,
    /// The nav graph NPCs find their way by ([`Graph::build`]), if the caller
    /// built one.
    pub nav: Option<Graph>,
    /// In spawn order; the caller fills them ([`Timelines::movers`]).
    pub movers: Vec<Mover>,
    /// What is used from outside the scripts, each at the first level frame at or
    /// after its time (after the first), before that frame's entities run (the `use` command, as the
    /// reference captures pin it: `g_swjkjaUse`); the caller fills them.
    pub uses: Vec<Use>,
}

/// A name used (`G_UseTargets2`: every entity with that "targetname") at a level
/// time (ms).
#[derive(Debug, Clone, PartialEq)]
pub struct Use {
    pub name: String,
    pub at: i32,
}

/// The level frame a time falls in: the first at or after it.
pub fn frame_at(ms: i32) -> i32 {
    FIRST_FRAME + ((ms - FIRST_FRAME).max(0) + FRAME_MS - 1) / FRAME_MS * FRAME_MS
}

impl Level {
    /// The level scripts of map `bsp` at skill `skill`, `scripts` reading a script by name: its
    /// script runners (`SP_target_scriptrunner`) but those that start inactive or
    /// run on what uses them (spawnflags 128 and 1; the player is not run here) or
    /// whose script does not load; and what the player uses as it begins: the
    /// targets of its first player start (`SelectSpawnPoint` picks one at random:
    /// the first; `ClientSpawn` uses its `target` and `target2`), then those of the
    /// triggers its box stands in there (`trigger_once`, `trigger_multiple`; its
    /// first move touches them), but those that need a button, a facing or an NPC,
    /// or start inactive (spawnflags 2, 4, 8, 16, 128).
    pub fn of(
        bsp: &swjkja_bsp::Bsp,
        skill: u32,
        scripts: &dyn Fn(&str) -> Option<Vec<Node>>,
    ) -> Self {
        let entities = &bsp.entities;
        let numbers = crate::entity_number::entity_numbers(entities, skill);
        let class = |e: &swjkja_bsp::Entity, c: &str| {
            e.get("classname")
                .is_some_and(|v| v.eq_ignore_ascii_case(c))
        };
        let runners = entities
            .iter()
            .zip(&numbers.entity)
            .filter(|(e, _)| class(e, "target_scriptrunner"))
            .filter(|(e, _)| e.get("spawnflags").map_or(0, crate::style::atoi) & (1 | 128) == 0)
            .filter_map(|(e, number)| {
                let script = scripts(e.get("usescript")?)?;
                let delay: f32 = e.get("delay").map_or(0.0, |d| {
                    d.split_ascii_whitespace()
                        .next()
                        .and_then(|d| d.parse().ok())
                        .unwrap_or(0.0)
                });
                Some(Runner {
                    targetname: e.get("targetname").map(str::to_owned),
                    script: std::sync::Arc::new(script),
                    delay: (delay * 1000.0) as i32,
                    number: (*number)?,
                })
            })
            .collect::<Vec<_>>();
        let mut runs = HashMap::new();
        let mut todo: Vec<String> = Vec::new();
        for r in &runners {
            run_names(&r.script, &mut todo);
        }
        while let Some(name) = todo.pop() {
            if runs.contains_key(&name) {
                continue;
            }
            if let Some(script) = scripts(&name) {
                run_names(&script, &mut todo);
                runs.insert(name, std::sync::Arc::new(script));
            }
        }
        let start = entities
            .iter()
            .find(|e| class(e, "info_player_start") || class(e, "info_player_deathmatch"));
        let mut begin: Vec<String> = start
            .into_iter()
            .flat_map(|e| [e.get("target"), e.get("target2")])
            .flatten()
            .map(str::to_owned)
            .collect();
        if let Some(start) = start {
            // The player's box (playerMins, playerMaxs) at the spawn point.
            let (origin, _) = crate::static_model::origin_and_angles(start);
            let mins = [origin[0] - 15.0, origin[1] - 15.0, origin[2] - 24.0];
            let maxs = [origin[0] + 15.0, origin[1] + 15.0, origin[2] + 40.0];
            for e in entities {
                if !(class(e, "trigger_once") || class(e, "trigger_multiple"))
                    || e.get("spawnflags").map_or(0, crate::style::atoi) & (2 | 4 | 8 | 16 | 128)
                        != 0
                {
                    continue;
                }
                let model = e
                    .get("model")
                    .and_then(|m| m.strip_prefix('*')?.parse::<usize>().ok());
                let Some(m) = model.and_then(|m| bsp.models.get(m)) else {
                    continue;
                };
                let touches = (0..3).all(|k| mins[k] <= m.maxs[k] && maxs[k] >= m.mins[k]);
                if touches {
                    begin.extend(
                        [e.get("target"), e.get("target2")]
                            .into_iter()
                            .flatten()
                            .map(str::to_owned),
                    );
                }
            }
        }
        // TAG_Add: a tag's name in lower case, the first of a name kept; those with
        // an "ownername" belong to it, and the movers here have none. A navgoal
        // (SP_waypoint_navgoal) is a tag 0.125 above its origin, its radius a whole
        // number.
        let mut tags = HashMap::new();
        let mut radii = HashMap::new();
        for e in entities {
            let navgoal = class(e, "waypoint_navgoal");
            if (navgoal || class(e, "ref_tag"))
                && e.get("ownername").is_none()
                && let Some(name) = e.get("targetname")
            {
                let (mut origin, _) = crate::static_model::origin_and_angles(e);
                let mut radius = 16.0;
                if navgoal {
                    origin[2] += 0.125;
                    radius = match e.get("radius").map_or(0.0, atof) as i32 {
                        0 => 12.0,
                        r => r as f32,
                    };
                }
                let name = name.to_ascii_lowercase();
                if !tags.contains_key(&name) {
                    tags.insert(name.clone(), origin);
                    radii.insert(name, radius);
                }
            }
        }
        Self {
            runners,
            begin,
            runs,
            tags,
            radii,
            collision: Collision::default(),
            nav: None,
            movers: Vec::new(),
            uses: Vec::new(),
        }
    }

    /// The script `run` names (`CSequencer::ParseRun` strips its extension).
    fn run(&self, name: &str) -> Option<&[Node]> {
        let name = name.to_ascii_lowercase();
        let name = name.strip_suffix(".ibi").unwrap_or(&name);
        self.runs.get(name).map(|s| s.as_slice())
    }
}

/// The scripts `nodes` `run`, anywhere in them, in lower case without extension.
fn run_names(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        match n {
            Node::Command(b) if b.id == id::RUN => {
                let name = text(b, 0).to_ascii_lowercase();
                out.push(name.strip_suffix(".ibi").unwrap_or(&name).to_owned());
            }
            Node::Body(_, body) => run_names(body, out),
            _ => {}
        }
    }
}

/// An NPC as its scripts see it: how it stands, its spawnscript, and its
/// skeleton's animations' holds and lengths.
pub struct Npc<'a> {
    pub stance: &'a Pose,
    pub script: Option<&'a [Node]>,
    pub holds: Holds<'a>,
}

/// Every actor's scripts stepped together: the map's script runners and its NPCs by
/// entity number in each frame, as `G_RunFrame` updates them, then its movers.
struct World<'a> {
    /// The runners, then the NPCs, then the movers.
    actors: Vec<Actor<'a>>,
    /// How many of the actors are runners.
    runners: usize,
    /// The order actors update in each frame.
    order: Vec<usize>,
    level: &'a Level,
    /// When each runner's script starts (level time), if it has been used.
    starts: Vec<Option<i32>>,
    /// Signals sent and not yet received (`CIcarus::m_signals`).
    signals: Vec<String>,
    voice: &'a dyn Fn(&str) -> Option<i32>,
    next_task: u32,
    /// When a script first enabled the camera (ms, on the client's clock).
    camera: Option<i32>,
    /// Every name used, at the level frame it was used in.
    used: Vec<Use>,
}

impl<'a> World<'a> {
    /// `target_scriptrunner_use` of the runners named `name` at `level`: each
    /// starts its script `delay` later, on its first think from then (`G_RunThink`
    /// runs its scripts right after), or with no delay at its next update: in this
    /// frame if its turn is still to come (by entity number), else the next; the
    /// next if `after_frame` (used after the frame's entities ran).
    fn use_targets(&mut self, name: &str, level: i32, after_frame: bool) {
        self.used.push(Use {
            name: name.to_owned(),
            at: level,
        });
        for (r, runner) in self.level.runners.iter().enumerate() {
            if runner
                .targetname
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case(name))
                && self.starts[r].is_none()
            {
                let start = if runner.delay > 0 {
                    level + runner.delay
                } else {
                    level + i32::from(after_frame)
                };
                self.starts[r] = Some(start);
            }
        }
    }

    /// The script's turn of actor `i` in level frame `level` (`CTaskManager::Update`):
    /// commands run one after another until one waits. Returns whether it still has
    /// commands. An `affect` runs the entity it affects right away (`CheckAffect`),
    /// `depth` deep.
    fn script(&mut self, i: usize, level: i32, depth: usize) -> bool {
        // RUNAWAY_LIMIT, per update.
        for _ in 0..10_000 {
            let a = &mut self.actors[i];
            let Some(top) = a.stack.last_mut() else {
                return false;
            };
            let Some(node) = top.nodes.get(top.at) else {
                match &mut top.kind {
                    Kind::Loop(n) if *n == -1 || *n > 1 => {
                        if *n > 1 {
                            *n -= 1;
                        }
                        top.at = 0;
                    }
                    _ => {
                        a.stack.pop();
                    }
                }
                continue;
            };
            let nodes = top.nodes;
            match node {
                Node::Command(b) => match b.id {
                    id::WAIT => {
                        let mut at = 0;
                        let done = match value(&b.members, &mut at) {
                            Some(Value::Text(name)) => a
                                .groups
                                .iter()
                                .find(|g| g.name.eq_ignore_ascii_case(&name))
                                .is_some_and(|g| g.tasks.iter().all(|t| t.1)),
                            Some(Value::Float(ms)) => {
                                let since = *a.wait_since.get_or_insert(level);
                                (since as f32 + ms) < level as f32
                            }
                            None => false,
                        };
                        if !done {
                            return true;
                        }
                        a.wait_since = None;
                    }
                    id::WAITSIGNAL => {
                        let name = text(b, 0);
                        let Some(s) = self.signals.iter().position(|s| s == name) else {
                            return true;
                        };
                        self.signals.remove(s);
                    }
                    id::SIGNAL => {
                        let name = text(b, 0);
                        if !self.signals.iter().any(|s| s == name) {
                            self.signals.push(name.to_owned());
                        }
                    }
                    id::USE => {
                        let name = text(b, 0).to_owned();
                        self.use_targets(&name, level, false);
                    }
                    id::MOVE if a.mover.is_some() => {
                        let task = self.next_task;
                        self.next_task += 1;
                        a.move_to(b, &self.level.tags, task, level);
                    }
                    id::ROTATE if a.mover.is_some() => {
                        let task = self.next_task;
                        self.next_task += 1;
                        a.rotate(b, task, level);
                    }
                    id::CAMERA => {
                        if b.members.first().and_then(Member::float) == Some(CAMERA_ENABLE) {
                            self.camera.get_or_insert(client_time(level));
                        }
                    }
                    id::SET | id::SOUND => {
                        let task = self.next_task;
                        self.next_task += 1;
                        let a = &mut self.actors[i];
                        if let Some(g) = a.current_group() {
                            a.groups[g].tasks.push((task, false));
                        }
                        let pending = if b.id == id::SOUND {
                            let (channel, name) = (text(b, 0), text(b, 1));
                            voice_channel(channel) && {
                                let length = (self.voice)(name).unwrap_or(0);
                                let a = &mut self.actors[i];
                                if let Some((old, _)) =
                                    a.voice.replace((task, level + VOICE_LAG + length))
                                {
                                    a.complete(old);
                                }
                                true
                            }
                        } else {
                            let mut at = 1;
                            let v = value(&b.members, &mut at);
                            if text(b, 0) == "SET_VIEWTARGET" {
                                let name = v.as_ref().map(Value::text).unwrap_or_default();
                                let eyes = self.eyes_of(&name);
                                self.actors[i].view_target(eyes, task)
                            } else {
                                let map = self.level;
                                self.actors[i].set(text(b, 0), v.as_ref(), task, level, map)
                            }
                        };
                        if !pending {
                            self.actors[i].complete(task);
                        }
                    }
                    // `CSequencer::CheckRun`: the script runs in place, in this
                    // update, then the rest of this one (its task groups were
                    // routed with it; registered here).
                    id::RUN => {
                        top.at += 1;
                        let level_scripts: &'a Level = self.level;
                        if let Some(nodes) = level_scripts.run(text(b, 0)) {
                            groups(nodes, &mut a.groups);
                            a.stack.push(Cursor {
                                nodes,
                                at: 0,
                                kind: Kind::Plain,
                            });
                        }
                        continue;
                    }
                    id::DO => {
                        let name = text(b, 0);
                        let found = a
                            .groups
                            .iter()
                            .position(|g| g.name.eq_ignore_ascii_case(name));
                        top.at += 1;
                        if let Some(g) = found {
                            a.groups[g].tasks.clear();
                            a.stack.push(Cursor {
                                nodes: a.groups[g].body,
                                at: 0,
                                kind: Kind::Group(g),
                            });
                        }
                        continue;
                    }
                    _ => {}
                },
                Node::Body(b, body) => match b.id {
                    id::LOOP => {
                        let mut at = 0;
                        let n = value(&b.members, &mut at).and_then(|v| v.float());
                        top.at += 1;
                        // `CheckLoop` enters the body before it reads the count, so a
                        // loop of 0 (`random(0, 3)` at its low end) runs once.
                        let n = n.map_or(0, |n| n as i32);
                        a.stack.push(Cursor {
                            nodes: body,
                            at: 0,
                            kind: Kind::Loop(n),
                        });
                        continue;
                    }
                    id::IF => {
                        top.at += 1;
                        let otherwise = match nodes.get(top.at) {
                            Some(Node::Body(e, body)) if e.id == id::ELSE => {
                                top.at += 1;
                                Some(body)
                            }
                            _ => None,
                        };
                        let chosen = if condition(b) { Some(body) } else { otherwise };
                        if let Some(nodes) = chosen {
                            a.stack.push(Cursor {
                                nodes,
                                at: 0,
                                kind: Kind::Plain,
                            });
                        }
                        continue;
                    }
                    id::AFFECT => {
                        top.at += 1;
                        let name = text(b, 0);
                        let insert =
                            b.members.get(1).and_then(Member::float) == Some(AFFECT_INSERT);
                        // An NPC by that name, or the last mover spawned with it (the
                        // name map keeps the last one registered).
                        let named =
                            |t: &Actor| t.name.is_some_and(|n| n.eq_ignore_ascii_case(name));
                        let target = self
                            .actors
                            .iter()
                            .position(|t| t.body.is_some() && named(t))
                            .or_else(|| {
                                self.actors
                                    .iter()
                                    .rposition(|t| t.mover.is_some() && named(t))
                            });
                        if let Some(t) = target {
                            self.actors[t].affect(body, insert);
                            if depth < 8 {
                                self.script(t, level, depth + 1);
                            }
                        }
                        continue;
                    }
                    // Task bodies are run by `do`.
                    _ => {}
                },
            }
            let a = &mut self.actors[i];
            a.stack.last_mut().unwrap().at += 1;
        }
        true
    }

    /// The eyes of the first NPC named `name` (`G_Find` by targetname, ignoring
    /// case), as it stands; other entities are not found here.
    fn eyes_of(&self, name: &str) -> Option<Vec3> {
        self.actors
            .iter()
            .filter(|a| a.name.is_some_and(|n| n.eq_ignore_ascii_case(name)))
            .find_map(|a| Some(a.body.as_ref()?.stance.eyes))
    }

    /// `G_CheckTasksCompleted` for actor `i` at `level`: a voice line played out
    /// completes its task.
    fn check_voice(&mut self, i: usize, level: i32) {
        let a = &mut self.actors[i];
        if let Some((task, end)) = a.voice
            && end <= level
        {
            a.voice = None;
            a.complete(task);
        }
    }

    /// One level frame, each actor in [`World::order`]: a runner's script, started
    /// if due; an NPC's (or a mover's) movement frame (`msec` after the last) and
    /// script.
    fn frame(&mut self, level: i32, msec: i32) {
        let world = &self.level.collision;
        let nav = self.level.nav.as_ref();
        for k in 0..self.order.len() {
            let i = self.order[k];
            if i < self.runners {
                if self.starts[i].is_some_and(|s| s <= level) && self.actors[i].stack.is_empty() {
                    self.starts[i] = Some(i32::MAX);
                    let script = self.level.runners[i].script.as_slice();
                    self.actors[i].run(script);
                }
                self.check_voice(i, level);
                self.script(i, level, 0);
                continue;
            }
            self.check_voice(i, level);
            // The other NPCs where they stand now: those before it have moved.
            let bodies: Vec<Obstacle> = (self.runners..self.actors.len())
                .filter(|&j| j != i)
                .filter_map(|j| {
                    let b = self.actors[j]
                        .body
                        .as_ref()
                        .filter(|b| !b.stance.not_solid)?;
                    let at = b.walk.origin;
                    Some(Obstacle {
                        actor: j,
                        mins: [0, 1, 2].map(|k| at[k] + b.stance.mins[k]),
                        maxs: [0, 1, 2].map(|k| at[k] + b.stance.maxs[k]),
                        velocity: b.walk.velocity,
                    })
                })
                .collect();
            let (thought, unfrozen) = self.actors[i].think(level, world, nav, &bodies);
            self.actors[i].turn(level, thought || !unfrozen);
            self.actors[i].movement(level, msec, world);
            self.actors[i].reach(level);
            self.script(i, level, 0);
        }
    }

    /// The first level frame after `level` at which something can change.
    fn next_event(&self, level: i32) -> Option<i32> {
        let mut next = self.actors.iter().filter_map(|a| a.next_event(level)).min();
        let mut at = |t: i32| next = Some(next.map_or(t, |n: i32| n.min(t)));
        for s in self.starts.iter().flatten() {
            if *s != i32::MAX {
                at(level + ((*s - level).max(1) + FRAME_MS - 1) / FRAME_MS * FRAME_MS);
            }
        }
        // A signal someone waits for is received on its next update.
        let waiting = self.actors.iter().any(|a| {
            a.stack
                .last()
                .and_then(|c| c.nodes.get(c.at))
                .is_some_and(|n| {
                    matches!(n, Node::Command(b) if b.id == id::WAITSIGNAL
                    && self.signals.iter().any(|s| s == text(b, 0)))
                })
        });
        if waiting {
            at(level + FRAME_MS);
        }
        next
    }
}

/// What a map's scripts do up to [`HORIZON`] ([`timelines`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Timelines {
    /// One per NPC, in order.
    pub npcs: Vec<Timeline>,
    /// When a script first enabled the camera (ms, on the client's clock): the
    /// camera then draws the view (`CGCam_RenderScene`), its field of view
    /// [`CAMERA_DEFAULT_FOV`] until a zoom; cameras are not modelled further.
    pub camera: Option<i32>,
    /// One per mover of the level ([`Level::movers`]), in order: its moves and
    /// turns.
    pub movers: Vec<Motion>,
    /// Every name the scripts and [`Level::uses`] used, in the order used, at the
    /// level frame of each (what else answers them, such as doors, is the caller's:
    /// [`crate::brush_entity::use_doors`]).
    pub used: Vec<Use>,
}

/// A camera's field of view as it is enabled (`CAMERA_DEFAULT_FOV`).
pub const CAMERA_DEFAULT_FOV: f32 = 90.0;

/// What NPCs standing in their stances do up to [`HORIZON`], each running its
/// spawnscript, and the map's `level` scripts with them; `voice` gives how long a
/// voice line plays (ms; `None` for one that does not load, which plays out at
/// once).
///
/// Each NPC starts in `BOTH_STAND1` (`NPC_Begin`). On the first frame its script
/// runs its first pass, then its first movement frame (counting no time down) puts
/// it in its stance; each later frame runs the runners' scripts, then each NPC's
/// movement frame and script. The player begins at [`PLAYER_BEGIN`], using its
/// spawn point's targets.
pub fn timelines(
    npcs: &[Npc<'_>],
    level: &Level,
    voice: &dyn Fn(&str) -> Option<i32>,
) -> Timelines {
    let part = || Part {
        anim: STAND.to_owned(),
        clock: Clock::from(FIRST_FRAME),
        timer: 0,
    };
    let mut actors: Vec<Actor> = level
        .runners
        .iter()
        .map(|r| Actor::new(r.targetname.as_deref(), None, None))
        .collect();
    for n in npcs {
        let body = Body {
            stance: n.stance,
            holds: n.holds,
            parts: [part(), part()],
            changes: [Vec::new(), Vec::new()],
            look: Vec::new(),
            more_light: Vec::new(),
            unarmed: Vec::new(),
            idle_time: n.stance.still_since,
            slots: [None; 5],
            view: [0.0, n.stance.yaw],
            desired: [0.0, n.stance.yaw],
            yaw_speed: n.stance.yaw_speed,
            yaw: Vec::new(),
            walk: Walk {
                origin: n.stance.origin,
                ..Walk::default()
            },
            moves: Vec::new(),
            velocity: Vec::new(),
        };
        actors.push(Actor::new(n.stance.name.as_deref(), Some(body), n.script));
    }
    let first_mover = actors.len();
    for m in &level.movers {
        let mut actor = Actor::new(Some(&m.name), None, None);
        actor.mover = Some(Moving {
            origin: m.origin,
            angles: m.angles,
            linear: m.linear,
            motion: Motion::default(),
            moving: None,
            think: None,
            tasks: [None; 2],
        });
        actors.push(actor);
    }
    // By entity number, a runner before an NPC of the same (made-up levels number
    // neither).
    let mut order: Vec<usize> = (0..first_mover).collect();
    order.sort_by_key(|&i| match &actors[i].body {
        Some(b) => (b.stance.number, 1),
        None => (level.runners[i].number, 0),
    });
    order.extend(first_mover..actors.len());
    let mut world = World {
        actors,
        runners: level.runners.len(),
        order,
        level,
        starts: vec![None; level.runners.len()],
        signals: Vec::new(),
        voice,
        next_task: 0,
        camera: None,
        used: Vec::new(),
    };
    // The pinned uses, by frame.
    let mut pinned: Vec<(i32, &str)> = level
        .uses
        .iter()
        .map(|u| (frame_at(u.at).max(FIRST_FRAME + FRAME_MS), u.name.as_str()))
        .collect();
    pinned.sort_by_key(|&(frame, _)| frame);
    let mut pinned = pinned.into_iter().peekable();
    let mut level_time = FIRST_FRAME;
    for i in world.order.clone() {
        if i < world.runners {
            continue;
        }
        world.script(i, level_time, 0);
        world.actors[i].movement(level_time, BEGIN_MSEC, &level.collision);
    }
    let mut begun = false;
    loop {
        let mut next = world.next_event(level_time);
        if !begun && !level.begin.is_empty() {
            let begin_frame = PLAYER_BEGIN + FRAME_MS;
            if next.is_none_or(|n| n >= begin_frame) && level_time < begin_frame {
                // The player enters the world after frame PLAYER_BEGIN.
                for name in &level.begin {
                    world.use_targets(name, PLAYER_BEGIN, true);
                }
                begun = true;
                next = world.next_event(level_time);
            }
        }
        if let Some(&(frame, _)) = pinned.peek() {
            next = Some(next.map_or(frame, |n| n.min(frame)));
        }
        let Some(next) = next else { break };
        if next > HORIZON {
            break;
        }
        // The frames in between count the timers down and keep the idle clock as
        // the frame before the event would leave them.
        let skipped = (next - level_time) / FRAME_MS - 1;
        if skipped > 0 {
            for a in &mut world.actors {
                a.movement(next - FRAME_MS, skipped * FRAME_MS, &level.collision);
            }
        }
        level_time = next;
        while let Some((_, name)) = pinned.next_if(|&(frame, _)| frame == level_time) {
            world.use_targets(name, level_time, false);
        }
        world.frame(level_time, FRAME_MS);
    }
    let first = |anim: &str| Change {
        start: FIRST_FRAME,
        anim: anim.to_owned(),
        from: None,
        clock: Clock::from(FIRST_FRAME),
    };
    let start = |mut c: Vec<Change>| {
        if c.first().is_none_or(|c| c.start != FIRST_FRAME) {
            c.insert(0, first(STAND));
        }
        c
    };
    let movers = world.actors[first_mover..]
        .iter_mut()
        .map(|a| std::mem::take(&mut a.mover.as_mut().unwrap().motion))
        .collect();
    let npcs = world
        .actors
        .into_iter()
        .filter_map(|a| a.body)
        .map(|b| {
            let [legs, torso] = b.changes;
            Timeline {
                legs: start(legs),
                torso: start(torso),
                look: b.look,
                more_light: b.more_light,
                unarmed: b.unarmed,
                yaw: b.yaw,
                moves: b.moves,
                velocity: b.velocity,
            }
        })
        .collect();
    Timelines {
        npcs,
        camera: world.camera,
        movers,
        used: world.used,
    }
}

/// What an NPC standing in `stance` does up to [`HORIZON`], running `script` (its
/// spawnscript, if any), with `holds` its skeleton's animations' holds and lengths
/// ([`timelines`] for it alone).
pub fn timeline(stance: &Pose, script: Option<&[Node]>, holds: Holds<'_>) -> Timeline {
    let npc = Npc {
        stance,
        script,
        holds,
    };
    timelines(&[npc], &Level::default(), &|_| None)
        .npcs
        .remove(0)
}

/// What a spawnscript's first pass leaves before its first command that may wait
/// (`wait`, `do`, a loop, a condition): whether it puts the NPC's weapon away
/// (`SET_WEAPON WP_NONE`: `G_RemoveWeaponModels` drops every bolted model) and
/// whether it makes it invisible (`SET_INVISIBLE`, `EF_NODRAW`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FirstPass {
    pub unarmed: bool,
    pub invisible: bool,
}

pub fn first_pass(script: &[Node]) -> FirstPass {
    let mut out = FirstPass::default();
    let mut names = HashMap::new();
    for n in script {
        let Node::Command(b) = n else {
            if matches!(n, Node::Body(b, _) if b.id == id::TASK) {
                continue;
            }
            break;
        };
        if b.id != id::SET {
            if matches!(b.id, id::WAIT | id::DO | id::WAITSIGNAL) {
                break;
            }
            continue;
        }
        let mut at = 1;
        if let (Some(name), Some(v)) = (
            b.members.first().and_then(Member::text),
            value(&b.members, &mut at),
        ) {
            names.insert(name, v.text());
        }
    }
    if let Some(w) = names.get("SET_WEAPON") {
        out.unarmed = w.eq_ignore_ascii_case("WP_NONE");
    }
    if let Some(v) = names.get("SET_INVISIBLE") {
        out.invisible = v.eq_ignore_ascii_case("true");
    }
    out
}

/// The file a script named `name` is read from (`RegisterScript`): under "scripts/"
/// unless the name starts with it, with ".IBI" added; `None` for "NULL" and
/// "default", which name no script.
pub fn script_path(name: &str) -> Option<String> {
    if name.eq_ignore_ascii_case("NULL") || name.eq_ignore_ascii_case("default") {
        return None;
    }
    let prefixed = name
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("scripts"));
    Some(if prefixed {
        format!("{name}.IBI")
    } else {
        format!("scripts/{name}.IBI")
    })
}
