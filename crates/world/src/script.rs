//! What a single-player NPC does as time passes while it stands with no enemy: its
//! movement frames (the stance it returns to, the idle it plays after standing still)
//! and the spawnscript it runs (`NPC_Begin` runs the script's first pass, `NPC_Think`
//! the rest, once a server frame; `code/icarus/TaskManager.cpp`, `Sequencer.cpp`;
//! the game's side in `Q3_Interface.cpp`).
//!
//! The game is stepped a server frame at a time in the original's order: in each
//! frame the NPC's movement frame (`Pmove`: its animation timers count down, then its
//! stance returns to the parts whose timers ran out; then `G_CheckClientIdle`), then
//! its script, which runs command after command until one waits. Frames in which
//! nothing can change are skipped.
//!
//! Only what a script does to the NPC itself is modelled: its animations and their
//! hold times, the tasks a script waits for (an animation's completes when its timer
//! runs out), timed waits, loops, conditions and task groups. Commands that move it
//! (`SET_NAVGOAL`, ...) never complete, as with the AI frozen; `affect` (another
//! entity), `run` (no spawnscript of a map-spawned NPC runs another) and the other
//! commands do nothing here. A random number is its range's low end, as other random
//! choices here take their first branch; a `get` reads nothing (a condition with one
//! is false).

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

/// The legs' and the torso's animations over time, from the first frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    pub legs: Vec<Change>,
    pub torso: Vec<Change>,
}

impl Timeline {
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

/// Where the script is: a body being run, how far, and what it is.
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

struct Npc<'a> {
    stance: &'a Pose,
    holds: Holds<'a>,
    parts: [Part; 2],
    changes: [Vec<Change>; 2],
    /// `client->idleTime`.
    idle_time: i32,
    /// The task each animation slot waits on, if any.
    slots: [Option<u32>; 3],
    groups: Vec<Group<'a>>,
    stack: Vec<Cursor<'a>>,
    /// When the current timed wait started (`CTask` time stamp).
    wait_since: Option<i32>,
    next_task: u32,
}

impl<'a> Npc<'a> {
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

    fn play(&mut self, part: usize, anim: &str, level: i32) {
        let start = Self::client_time(level);
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
        for s in &mut self.slots {
            if *s == Some(id) {
                *s = None;
            }
        }
    }

    /// `Q3_TaskIDSet`: a task stomped by another completes.
    fn slot(&mut self, slot: usize, id: u32) {
        if let Some(old) = self.slots[slot] {
            self.complete(old);
        }
        self.slots[slot] = Some(id);
    }

    /// `PM_SetLegsAnimTimer` / `PM_SetTorsoAnimTimer`: a timer that runs out
    /// completes the animation task waiting on its part (both parts' when both wait).
    fn set_timer(&mut self, part: usize, time: i32) {
        let t = &mut self.parts[part].timer;
        *t = if time < 0 && time != -1 { 0 } else { time };
        let (mine, other) = if part == 0 {
            (LOWER, UPPER)
        } else {
            (UPPER, LOWER)
        };
        if *t != 0 {
            return;
        }
        let Some(task) = self.slots[mine] else { return };
        match self.slots[BOTH] {
            None => self.complete(task),
            Some(both) => {
                self.slots[mine] = None;
                if self.slots[other].is_none() {
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
        let Some((hold, _)) = (self.holds)(anim) else {
            return false;
        };
        for &p in parts {
            if restart || self.parts[p].anim != anim {
                self.set_timer(p, 0);
            }
        }
        for &p in parts {
            // An animation already playing is not played again, nor held anew.
            if restart || self.parts[p].anim != anim {
                self.play(p, anim, level);
                self.set_timer(p, hold);
            }
        }
        true
    }

    /// The movement frame of level frame `level`, `msec` after the last: the timers
    /// count down (`PM_DropTimers`), the stance returns to parts no longer held
    /// (`PM_Footsteps`, `PM_TorsoAnimation`, which play an animation the skeleton
    /// has, without restarting it), then `G_CheckClientIdle`.
    fn movement(&mut self, level: i32, msec: i32) {
        for p in 0..2 {
            if self.parts[p].timer > 0 {
                let left = (self.parts[p].timer - msec).max(0);
                self.set_timer(p, left);
            }
        }
        for (p, anim) in [(0, self.stance.legs), (1, self.stance.torso)] {
            let has = (self.holds)(anim).is_some();
            if self.parts[p].timer == 0 && has && self.parts[p].anim != anim {
                self.play(p, anim, level);
            }
        }
        let [legs, torso] = &self.parts;
        if !standing(&legs.anim) || legs.timer != 0 || torso.timer != 0 {
            self.idle_time = self.idle_time.max(level);
        } else if level - self.idle_time > 5000
            && let Some(idle) = idle_for(&legs.anim)
            && let Some((_, length)) = (self.holds)(idle)
        {
            self.set_held(&[0, 1], idle, false, level);
            self.idle_time = level + length;
        }
    }

    /// One `set` command, as task `task`: whether its task stays pending.
    fn set(&mut self, name: &str, value: Option<&Value>, task: u32, level: i32) -> bool {
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
                    self.slots[BOTH] = None;
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
                self.slots[BOTH] = None;
                self.slot(slot, task);
                true
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

    /// The script's turn in level frame `level` (`CTaskManager::Update`): commands run
    /// one after another until one waits. Returns whether it still has commands.
    fn script(&mut self, level: i32) -> bool {
        // RUNAWAY_LIMIT, per update.
        for _ in 0..10_000 {
            let Some(top) = self.stack.last_mut() else {
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
                        self.stack.pop();
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
                            Some(Value::Text(name)) => self
                                .groups
                                .iter()
                                .find(|g| g.name.eq_ignore_ascii_case(&name))
                                .is_some_and(|g| g.tasks.iter().all(|t| t.1)),
                            Some(Value::Float(ms)) => {
                                let since = *self.wait_since.get_or_insert(level);
                                (since as f32 + ms) < level as f32
                            }
                            None => false,
                        };
                        if !done {
                            return true;
                        }
                        self.wait_since = None;
                    }
                    id::WAITSIGNAL => return true,
                    id::SET => {
                        let task = self.next_task;
                        self.next_task += 1;
                        let group = self.current_group();
                        if let Some(g) = group {
                            self.groups[g].tasks.push((task, false));
                        }
                        let mut at = 1;
                        let name = b.members.first().and_then(Member::text).unwrap_or("");
                        let v = value(&b.members, &mut at);
                        let name = name.to_owned();
                        if !self.set(&name, v.as_ref(), task, level) {
                            self.complete(task);
                        }
                    }
                    id::DO => {
                        let name = b.members.first().and_then(Member::text).unwrap_or("");
                        let found = self
                            .groups
                            .iter()
                            .position(|g| g.name.eq_ignore_ascii_case(name));
                        self.stack.last_mut().unwrap().at += 1;
                        if let Some(g) = found {
                            self.groups[g].tasks.clear();
                            self.stack.push(Cursor {
                                nodes: self.groups[g].body,
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
                        self.stack.last_mut().unwrap().at += 1;
                        let n = n.map_or(0, |n| n as i32);
                        if n != 0 {
                            self.stack.push(Cursor {
                                nodes: body,
                                at: 0,
                                kind: Kind::Loop(n),
                            });
                        }
                        continue;
                    }
                    id::IF => {
                        let top = self.stack.last_mut().unwrap();
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
                            self.stack.push(Cursor {
                                nodes,
                                at: 0,
                                kind: Kind::Plain,
                            });
                        }
                        continue;
                    }
                    // Task bodies run by `do`; `affect` runs on another entity.
                    _ => {}
                },
            }
            self.stack.last_mut().unwrap().at += 1;
        }
        true
    }

    /// The first level frame after `level` at which something can change, if any:
    /// a timer running out, a timed wait ending, or the idle starting.
    fn next_event(&self, level: i32, script_waits: bool) -> Option<i32> {
        let frame_from = |t: i32| level + ((t - level).max(1) + FRAME_MS - 1) / FRAME_MS * FRAME_MS;
        let mut next: Option<i32> = None;
        let mut at = |t: i32| next = Some(next.map_or(t, |n| n.min(t)));
        for p in &self.parts {
            if p.timer > 0 {
                at(frame_from(level + p.timer));
            }
        }
        if script_waits && let Some(since) = self.wait_since {
            let top = self.stack.last().unwrap();
            if let Some(Node::Command(b)) = top.nodes.get(top.at) {
                let mut i = 0;
                if let Some(Value::Float(ms)) = value(&b.members, &mut i) {
                    // The first frame later than since + ms.
                    let end = (since as f32 + ms).floor() as i32 + 1;
                    at(frame_from(end));
                }
            }
        }
        let [legs, torso] = &self.parts;
        let idle = idle_for(&legs.anim).and_then(|a| (self.holds)(a));
        if standing(&legs.anim) && legs.timer == 0 && torso.timer == 0 && idle.is_some() {
            at(frame_from(self.idle_time + 5001));
        }
        next
    }
}

/// Every `task` block in `nodes` and the bodies under them (`ParseTask` registers
/// each as the script is routed, before it runs), the first of a name kept.
fn groups<'a>(nodes: &'a [Node], out: &mut Vec<Group<'a>>) {
    for n in nodes {
        if let Node::Body(b, body) = n {
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

/// What an NPC standing in `stance` does up to [`HORIZON`], running `script` (its
/// spawnscript, if any), with `holds` its skeleton's animations' holds and lengths.
///
/// It starts in `BOTH_STAND1` (`NPC_Begin`). On the first frame its script runs its
/// first pass, then its first movement frame (counting no time down) puts it in its
/// stance; each later frame runs its movement frame, then its script.
pub fn timeline(stance: &Pose, script: Option<&[Node]>, holds: Holds<'_>) -> Timeline {
    let mut all = Vec::new();
    if let Some(nodes) = script {
        groups(nodes, &mut all);
    }
    let part = || Part {
        anim: STAND.to_owned(),
        start: FIRST_FRAME,
        timer: 0,
    };
    let mut npc = Npc {
        stance,
        holds,
        parts: [part(), part()],
        changes: [Vec::new(), Vec::new()],
        idle_time: stance.still_since,
        slots: [None; 3],
        groups: all,
        stack: script
            .map(|nodes| Cursor {
                nodes,
                at: 0,
                kind: Kind::Plain,
            })
            .into_iter()
            .collect(),
        wait_since: None,
        next_task: 0,
    };
    let mut level = FIRST_FRAME;
    let mut running = npc.script(level);
    npc.movement(level, 0);
    while let Some(next) = npc.next_event(level, running) {
        if next > HORIZON {
            break;
        }
        // The frames in between count the timers down and keep the idle clock as
        // the frame before the event would leave them.
        let skipped = (next - level) / FRAME_MS - 1;
        if skipped > 0 {
            npc.movement(next - FRAME_MS, skipped * FRAME_MS);
        }
        level = next;
        npc.movement(level, FRAME_MS);
        if running {
            running = npc.script(level);
        }
    }
    let [legs, torso] = npc.changes;
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
    Timeline {
        legs: start(legs),
        torso: start(torso),
    }
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
