//! What single-player NPCs do as time passes while they stand with no enemy: their
//! movement frames (the stance they return to, the idle they play after standing
//! still), the spawnscripts they run (`NPC_Begin` runs a script's first pass,
//! `NPC_Think` the rest, once a server frame), and the map's scripts that run with
//! them (`target_scriptrunner`, such as the academies' intros, which their player
//! starts as it enters the world); `code/icarus/TaskManager.cpp`, `Sequencer.cpp`;
//! the game's side in `Q3_Interface.cpp`.
//!
//! The game is stepped a server frame at a time in the original's order: in each
//! frame each script runner's script, then each NPC's movement frame (`Pmove`: its
//! animation timers count down, then its stance returns to the parts whose timers ran
//! out; then `G_CheckClientIdle`) and script; a script runs command after command
//! until one waits. Frames in which nothing can change are skipped.
//!
//! What scripts do to NPCs is modelled: their animations and hold times, what they
//! look at (`SET_LOOK_TARGET`), whether they are drawn with more light
//! (`SET_MORELIGHT`), the tasks scripts wait for (an animation's completes
//! when its timer runs out, a voice line's when it has played), timed waits, loops,
//! conditions, task groups, signals, `use` of other script runners, and `affect`,
//! which hands a block to an NPC (`FLUSH` drops the rest of its script, `INSERT`
//! runs the block first) and runs it at once. Commands that move an NPC
//! (`SET_NAVGOAL`, `SET_ORIGIN`, ...) are left out (those that complete later never
//! do, as with the AI frozen), and so are cameras, `run` (no script reached here
//! runs another) and the other commands. A random number is its range's low end
//! (once the client runs, what the shared generator draws depends on frame timing:
//! [`crate::rand`]); a `get` reads nothing (a condition with one is false).

use std::collections::HashMap;

use swjkja_icarus::{Block, Member, Node, id};

use crate::static_model::Pose;

/// The level time (ms) of the server's first frame on a map: four settling frames
/// from 1000 ms 100 ms apart and one more at 1400 ms leave the clock at 1500, and
/// each frame then runs 50 ms later (`SV_SpawnServer`, `SV_Frame` at `sv_fps` 20).
/// A map's NPCs begin on it (spawners think 350 ms after spawning, `NPC_Begin` 100
/// ms after that).
pub const FIRST_FRAME: i32 = 1550;
/// Milliseconds between server frames.
pub const FRAME_MS: i32 = 50;
/// Milliseconds a part blends from its previous animation into a new one
/// (`SETANIM_BLEND_DEFAULT`, which `NPC_SetAnim` and the stance's `PM_SetAnim`
/// pass).
pub const BLEND_MS: i32 = 100;

/// How long the timeline runs (ms of game time); the last animations then hold.
pub const HORIZON: i32 = 3_600_000;

/// The animation an NPC starts in (`NPC_Begin`).
const STAND: &str = "BOTH_STAND1";

/// One part's change of animation: what it plays from `start` (ms, on the clock the
/// client draws by), and what it blends from, the animation before and when that
/// started, if it changed after the first frame (`G2_Set_Bone_Anim_Index` keeps the
/// old animation's frame for [`BLEND_MS`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub start: i32,
    pub anim: String,
    pub from: Option<(String, i32)>,
}

/// The legs' and the torso's animations over time, from the first frame, and what
/// the NPC looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

impl Timeline {
    /// Whether the NPC is drawn with more light (`RF_MORELIGHT`) at `time`.
    pub fn more_light_at(&self, time: i32) -> bool {
        let after = self.more_light.partition_point(|(start, _)| *start <= time);
        after.checked_sub(1).is_some_and(|i| self.more_light[i].1)
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

    /// What part `part` blends from at `time` and how far the blend has gone (0 to
    /// 1), or `None` outside a blend.
    pub fn blending_at(&self, part: usize, time: i32) -> Option<(&str, i32, f32)> {
        let c = self.at(part, time);
        let elapsed = time - c.start;
        let (anim, start) = c.from.as_ref()?;
        (0..BLEND_MS)
            .contains(&elapsed)
            .then(|| (anim.as_str(), *start, elapsed as f32 / BLEND_MS as f32))
    }
}

/// An animation's hold and full length (ms) as the game times it
/// (`PM_SetAnimFinal`'s `animDurMSec`, `PM_AnimLength`), or `None` when the
/// skeleton lacks it (`PM_HasAnimation`).
pub type Holds<'a> = &'a dyn Fn(&str) -> Option<(i32, i32)>;

/// Task slots an animation's completion is kept in (`TID_ANIM_BOTH`, `_LOWER`,
/// `_UPPER`).
const BOTH: usize = 0;
const LOWER: usize = 1;
const UPPER: usize = 2;

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
/// without calling back) and that nothing here completes: they move or turn the NPC.
fn never_completes(name: &str) -> bool {
    matches!(
        name,
        "SET_ANIMFRAME"
            | "SET_BEHAVIOR_STATE"
            | "SET_DPITCH"
            | "SET_DYAW"
            | "SET_ENDFRAME"
            | "SET_LOCATION"
            | "SET_NAVGOAL"
            | "SET_TELEPORT_DEST"
            | "SET_TEMP_BSTATE"
            | "SET_VIEWTARGET"
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
    start: i32,
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
    /// `client->idleTime`.
    idle_time: i32,
    /// The task each animation slot waits on, if any.
    slots: [Option<u32>; 3],
}

/// An entity that runs scripts (its `CSequencer` and `CTaskManager`): an NPC, with
/// a body, or a script runner.
struct Actor<'a> {
    /// The name scripts find it by (`script_targetname`).
    name: Option<&'a str>,
    body: Option<Body<'a>>,
    groups: Vec<Group<'a>>,
    stack: Vec<Cursor<'a>>,
    /// When the current timed wait started (`CTask` time stamp).
    wait_since: Option<i32>,
    /// The voice line playing (`TID_CHAN_VOICE`): its task and the level time it
    /// has played out by.
    voice: Option<(u32, i32)>,
}

/// When an animation started in level frame `level` starts for the client: one
/// frame earlier, except on the first frame (the client's clock still at 0, the
/// level's time is taken).
fn client_time(level: i32) -> i32 {
    if level == FIRST_FRAME {
        level
    } else {
        level - FRAME_MS
    }
}

impl<'a> Body<'a> {
    fn play(&mut self, part: usize, anim: &str, level: i32) {
        let start = client_time(level);
        let p = &mut self.parts[part];
        let from = (level != FIRST_FRAME).then(|| (p.anim.clone(), p.start));
        p.anim = anim.to_owned();
        p.start = start;
        self.changes[part].push(Change {
            start,
            anim: anim.to_owned(),
            from,
        });
    }
}

impl<'a> Actor<'a> {
    fn new(name: Option<&'a str>, body: Option<Body<'a>>, script: Option<&'a [Node]>) -> Self {
        let mut actor = Self {
            name,
            body,
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
        let Some((hold, _)) = (body.holds)(anim) else {
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
                self.body.as_mut().unwrap().play(p, anim, level);
                self.set_timer(p, hold);
            }
        }
        true
    }

    /// The movement frame of level frame `level`, `msec` after the last: the timers
    /// count down (`PM_DropTimers`), the stance returns to parts no longer held
    /// (`PM_Footsteps`, `PM_TorsoAnimation`, which play an animation the skeleton
    /// has, without restarting it), then `G_CheckClientIdle`. Nothing for an actor
    /// without a body.
    fn movement(&mut self, level: i32, msec: i32) {
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
        for (p, anim) in [(0, body.stance.legs), (1, body.stance.torso)] {
            let has = (body.holds)(anim).is_some();
            if body.parts[p].timer == 0 && has && body.parts[p].anim != anim {
                body.play(p, anim, level);
            }
        }
        let [legs, torso] = &body.parts;
        let mut idle = None;
        if !standing(&legs.anim) || legs.timer != 0 || torso.timer != 0 {
            body.idle_time = body.idle_time.max(level);
        } else if level - body.idle_time > 5000
            && let Some(anim) = idle_for(&legs.anim)
            && let Some((_, length)) = (body.holds)(anim)
        {
            idle = Some((anim, length));
        }
        if let Some((anim, length)) = idle {
            self.set_held(&[0, 1], anim, false, level);
            self.body.as_mut().unwrap().idle_time = level + length;
        }
    }

    /// One `set` command, as task `task`: whether its task stays pending. An actor
    /// without a body completes every one.
    fn set(&mut self, name: &str, value: Option<&Value>, task: u32, level: i32) -> bool {
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
            _ => never_completes(name),
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
        let Some(body) = &self.body else { return next };
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

/// A map's `target_scriptrunner`: the name it is used by, its script (`usescript`)
/// and how long after it is used the script starts (`delay`, ms).
#[derive(Debug, Clone)]
pub struct Runner {
    pub targetname: Option<String>,
    pub script: std::sync::Arc<Vec<Node>>,
    pub delay: i32,
}

/// The scripts a map runs besides its NPCs' spawnscripts: its script runners, and
/// the names its player's spawn point uses as the player begins.
#[derive(Debug, Clone, Default)]
pub struct Level {
    pub runners: Vec<Runner>,
    pub begin: Vec<String>,
}

impl Level {
    /// The level scripts of map `bsp`, `scripts` reading a script by name: its
    /// script runners (`SP_target_scriptrunner`) but those that start inactive or
    /// run on what uses them (spawnflags 128 and 1; the player is not run here) or
    /// whose script does not load; and what the player uses as it begins: the
    /// targets of its first player start (`SelectSpawnPoint` picks one at random:
    /// the first; `ClientSpawn` uses its `target` and `target2`), then those of the
    /// triggers its box stands in there (`trigger_once`, `trigger_multiple`; its
    /// first move touches them), but those that need a button, a facing or an NPC,
    /// or start inactive (spawnflags 2, 4, 8, 16, 128).
    pub fn of(bsp: &swjkja_bsp::Bsp, scripts: &dyn Fn(&str) -> Option<Vec<Node>>) -> Self {
        let entities = &bsp.entities;
        let class = |e: &swjkja_bsp::Entity, c: &str| {
            e.get("classname")
                .is_some_and(|v| v.eq_ignore_ascii_case(c))
        };
        let runners = entities
            .iter()
            .filter(|e| class(e, "target_scriptrunner"))
            .filter(|e| e.get("spawnflags").map_or(0, crate::style::atoi) & (1 | 128) == 0)
            .filter_map(|e| {
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
                })
            })
            .collect();
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
        Self { runners, begin }
    }
}

/// An NPC as its scripts see it: how it stands, its spawnscript, and its
/// skeleton's animations' holds and lengths.
pub struct Npc<'a> {
    pub stance: &'a Pose,
    pub script: Option<&'a [Node]>,
    pub holds: Holds<'a>,
}

/// Every actor's scripts stepped together: the map's script runners, then its
/// NPCs, in each frame (the original's entity order puts most runners first).
struct World<'a> {
    actors: Vec<Actor<'a>>,
    /// How many of the actors are runners (they come first).
    runners: usize,
    level: &'a Level,
    /// When each runner's script starts (level time), if it has been used.
    starts: Vec<Option<i32>>,
    /// Signals sent and not yet received (`CIcarus::m_signals`).
    signals: Vec<String>,
    voice: &'a dyn Fn(&str) -> Option<i32>,
    next_task: u32,
}

impl<'a> World<'a> {
    /// `target_scriptrunner_use` of the runners named `name` at `level`: each
    /// starts its script `delay` later, on its first think from then (`G_RunThink`
    /// runs its scripts right after), or with no delay at its next update.
    fn use_targets(&mut self, name: &str, level: i32) {
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
                    level + 1
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
                        self.use_targets(&name, level);
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
                                if let Some((old, _)) = a.voice.replace((task, level + length)) {
                                    a.complete(old);
                                }
                                true
                            }
                        } else {
                            let mut at = 1;
                            let v = value(&b.members, &mut at);
                            a.set(text(b, 0), v.as_ref(), task, level)
                        };
                        if !pending {
                            self.actors[i].complete(task);
                        }
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
                        let n = n.map_or(0, |n| n as i32);
                        if n != 0 {
                            a.stack.push(Cursor {
                                nodes: body,
                                at: 0,
                                kind: Kind::Loop(n),
                            });
                        }
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
                        let target = self.actors.iter().position(|t| {
                            t.body.is_some() && t.name.is_some_and(|n| n.eq_ignore_ascii_case(name))
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

    /// One level frame: each runner's script, starting those due; then each NPC's
    /// movement frame (`msec` after the last) and script.
    fn frame(&mut self, level: i32, msec: i32) {
        for r in 0..self.runners {
            if self.starts[r].is_some_and(|s| s <= level) && self.actors[r].stack.is_empty() {
                self.starts[r] = Some(i32::MAX);
                let script = self.level.runners[r].script.as_slice();
                self.actors[r].run(script);
            }
            self.check_voice(r, level);
            self.script(r, level, 0);
        }
        for i in self.runners..self.actors.len() {
            self.check_voice(i, level);
            self.actors[i].movement(level, msec);
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

/// What NPCs standing in their stances do up to [`HORIZON`], each running its
/// spawnscript, and the map's `level` scripts with them; `voice` gives how long a
/// voice line plays (ms; `None` for one that does not load, which plays out at
/// once). One timeline per NPC, in order.
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
) -> Vec<Timeline> {
    let part = || Part {
        anim: STAND.to_owned(),
        start: FIRST_FRAME,
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
            idle_time: n.stance.still_since,
            slots: [None; 3],
        };
        actors.push(Actor::new(n.stance.name.as_deref(), Some(body), n.script));
    }
    let mut world = World {
        actors,
        runners: level.runners.len(),
        level,
        starts: vec![None; level.runners.len()],
        signals: Vec::new(),
        voice,
        next_task: 0,
    };
    let mut level_time = FIRST_FRAME;
    for i in world.runners..world.actors.len() {
        world.script(i, level_time, 0);
        world.actors[i].movement(level_time, 0);
    }
    let mut begun = false;
    loop {
        let mut next = world.next_event(level_time);
        if !begun && !level.begin.is_empty() {
            let begin_frame = PLAYER_BEGIN + FRAME_MS;
            if next.is_none_or(|n| n >= begin_frame) && level_time < begin_frame {
                // The player enters the world after frame PLAYER_BEGIN.
                for name in &level.begin {
                    world.use_targets(name, PLAYER_BEGIN);
                }
                begun = true;
                next = world.next_event(level_time);
            }
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
                a.movement(next - FRAME_MS, skipped * FRAME_MS);
            }
        }
        level_time = next;
        world.frame(level_time, FRAME_MS);
    }
    let first = |anim: &str| Change {
        start: FIRST_FRAME,
        anim: anim.to_owned(),
        from: None,
    };
    let start = |mut c: Vec<Change>| {
        if c.first().is_none_or(|c| c.start != FIRST_FRAME) {
            c.insert(0, first(STAND));
        }
        c
    };
    world
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
            }
        })
        .collect()
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
    timelines(&[npc], &Level::default(), &|_| None).remove(0)
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
