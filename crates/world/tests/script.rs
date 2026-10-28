use std::sync::Arc;
use swjkja_icarus::{Block, Member, Node, id};

use swjkja_world::brush_entity::{BrushEntity, Slide};
use swjkja_world::script::{
    Change, FirstPass, Level, Mover, Npc, Runner, Use, first_pass, frame_at, script_path, timeline,
    timelines,
};
use swjkja_world::static_model::Pose;

fn text(t: &str) -> Member {
    Member {
        id: id::STRING,
        data: format!("{t}\0").into_bytes(),
    }
}

fn float(x: f32) -> Member {
    Member {
        id: id::FLOAT,
        data: x.to_le_bytes().to_vec(),
    }
}

/// `random(low, high)` as the compiler writes it: the marker, then both ends.
fn random(low: f32, high: f32) -> [Member; 3] {
    let marker = Member {
        id: id::RANDOM,
        data: f32::MAX.to_le_bytes().to_vec(),
    };
    [marker, float(low), float(high)]
}

fn command(id: i32, members: Vec<Member>) -> Node {
    Node::Command(Block {
        id,
        flags: 0,
        members,
    })
}

fn body(id: i32, members: Vec<Member>, nodes: Vec<Node>) -> Node {
    Node::Body(
        Block {
            id,
            flags: 0,
            members,
        },
        nodes,
    )
}

fn set(name: &str, value: &str) -> Node {
    command(id::SET, vec![text(name), text(value)])
}

fn hold_forever() -> Node {
    command(id::SET, vec![text("SET_ANIM_HOLDTIME_BOTH"), float(-1.0)])
}

fn stance(legs: &'static str, torso: &'static str) -> Pose {
    Pose {
        legs,
        torso,
        still_since: 0,
        script: None,
        name: None,
        bad_bones: false,
        ..Pose::default()
    }
}

/// Animations' holds and lengths (ms): (frames - 1) and frames at 20 per second.
fn holds(name: &str) -> Option<(i32, i32)> {
    let frames = match name {
        "BOTH_STAND1" | "BOTH_STAND3" | "TORSO_WEAPONIDLE3" | "BOTH_SIT5" | "BOTH_CIN_1" => 21,
        "BOTH_WALK1" | "BOTH_RUN1" | "BOTH_WALKBACK1" | "BOTH_RUNBACK1" => 21,
        "BOTH_STAND1IDLE1" => 40,
        "BOTH_STAND3IDLE1" => 151,
        "BOTH_SIT5_TALK1" => 41,
        "BOTH_SIT5_TALK3" => 61,
        _ => return None,
    };
    Some(((frames - 1) * 50, frames * 50))
}

fn changes(c: &[Change]) -> Vec<(&str, i32)> {
    c.iter().map(|c| (c.anim.as_str(), c.start)).collect()
}

/// The legs' and torso's animations at `time` and when the legs' started.
fn at(t: &swjkja_world::script::Timeline, time: i32) -> (&str, &str, i32) {
    let (legs, torso) = (t.at(0, time), t.at(1, time));
    (&legs.anim, &torso.anim, legs.start)
}

#[test]
fn npcs_standing_still_play_their_idle_after_five_seconds() {
    let still = stance("BOTH_STAND3", "TORSO_WEAPONIDLE3");
    let t = timeline(&still, None, &holds);
    // The stance from the first frame; the idle from the first frame past 5 s of
    // level time (5050), on the client's clock a frame behind.
    assert_eq!(at(&t, 4999), ("BOTH_STAND3", "TORSO_WEAPONIDLE3", 1550));
    assert_eq!(at(&t, 5000), ("BOTH_STAND3IDLE1", "BOTH_STAND3IDLE1", 5000));
    assert_eq!(
        at(&t, 12345),
        ("BOTH_STAND3IDLE1", "BOTH_STAND3IDLE1", 5000)
    );
    // Held until 12550, then the stance until 5 s after the idle's full length.
    assert_eq!(at(&t, 12500), ("BOTH_STAND3", "TORSO_WEAPONIDLE3", 12500));
    assert_eq!(at(&t, 17599), ("BOTH_STAND3", "TORSO_WEAPONIDLE3", 12500));
    assert_eq!(
        at(&t, 17600),
        ("BOTH_STAND3IDLE1", "BOTH_STAND3IDLE1", 17600)
    );
    // One that fell moved on the first frame: its idle comes 1550 ms later.
    let fell = Pose {
        still_since: 1550,
        ..still.clone()
    };
    let t = timeline(&fell, None, &holds);
    assert_eq!(at(&t, 6549).2, 1550);
    assert_eq!(at(&t, 6550), ("BOTH_STAND3IDLE1", "BOTH_STAND3IDLE1", 6550));
    // No idle animation (the stance has none, or the skeleton lacks it).
    let t = timeline(&stance("BOTH_STAND4", "BOTH_STAND4"), None, &|n| {
        (n == "BOTH_STAND4").then_some((1000, 1050))
    });
    assert_eq!(at(&t, 12345), ("BOTH_STAND4", "BOTH_STAND4", 1550));
    let t = timeline(&still, None, &|n| {
        holds(n).filter(|_| !n.ends_with("IDLE1"))
    });
    assert_eq!(at(&t, 12345).2, 1550);
}

#[test]
fn npcs_blend_into_their_idle_and_back_over_100_ms() {
    let t = timeline(&stance("BOTH_STAND3", "TORSO_WEAPONIDLE3"), None, &holds);
    // The idle starts at 5000, blending from the stance that started at 1550.
    assert_eq!(t.blending_at(0, 4999), None);
    assert_eq!(t.blending_at(0, 5000), Some(("BOTH_STAND3", 1550, 0.0)));
    assert_eq!(
        t.blending_at(1, 5050),
        Some(("TORSO_WEAPONIDLE3", 1550, 0.5))
    );
    assert_eq!(t.blending_at(0, 5100), None);
    // The stance comes back at 12500, blending from the idle.
    assert_eq!(
        t.blending_at(1, 12525),
        Some(("BOTH_STAND3IDLE1", 5000, 0.25))
    );
    assert_eq!(t.blending_at(0, 12345), None);
    // The stance on the first frame does not (its BOTH_STAND1 is left out).
    assert_eq!(t.blending_at(0, 1600), None);
}

#[test]
fn a_script_holds_its_npc_in_an_animation() {
    // academy1/studentSpawn: seated for good, from the first frame.
    let script = vec![
        set("SET_WEAPON", "WP_NONE"),
        set("SET_ANIM_BOTH", "BOTH_SIT5"),
        hold_forever(),
    ];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    assert_eq!(changes(&t.legs), [("BOTH_SIT5", 1550)]);
    assert_eq!(changes(&t.torso), [("BOTH_SIT5", 1550)]);
    assert_eq!(at(&t, 3_000_000), ("BOTH_SIT5", "BOTH_SIT5", 1550));
    assert_eq!(
        first_pass(&script),
        FirstPass {
            unarmed: true,
            invisible: false
        }
    );
}

#[test]
fn a_held_animation_gives_way_to_the_stance_then_the_idle() {
    // Held for its 1000 ms, of which `NPC_Begin`'s movement frame counts down 200:
    // the stance returns on the frame its timer runs out (2350, drawn from 2300),
    // and the idle comes 5 s after the last frame that held it (2300 + 5000, the
    // first frame past it 7350).
    let script = vec![set("SET_ANIM_BOTH", "BOTH_CIN_1")];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    assert_eq!(
        changes(&t.legs)[..5],
        [
            ("BOTH_CIN_1", 1550),
            ("BOTH_STAND1", 2300),
            ("BOTH_STAND1IDLE1", 7300),
            ("BOTH_STAND1", 9250),
            ("BOTH_STAND1IDLE1", 14350),
        ]
    );
    assert_eq!(t.blending_at(0, 2350), Some(("BOTH_CIN_1", 1550, 0.5)));
}

#[test]
fn a_looping_script_waits_on_times_and_on_its_animations() {
    // yavin1/jedi_stud_sitting1, with its random waits at their low end.
    let talk =
        |name: &str, anim: &str| body(id::TASK, vec![text(name)], vec![set("SET_ANIM_BOTH", anim)]);
    let turn = |low: f32, high: f32, task: &str| {
        vec![
            set("SET_ANIM_BOTH", "BOTH_SIT5"),
            hold_forever(),
            command(id::WAIT, random(low, high).to_vec()),
            command(id::DO, vec![text(task)]),
            command(id::WAIT, vec![text(task)]),
        ]
    };
    let mut forever = turn(2500.0, 7000.0, "talk1");
    forever.extend(turn(3500.0, 9000.0, "talk3"));
    forever.extend(turn(1500.0, 9000.0, "talk2"));
    let script = vec![
        set("SET_WEAPON", "WP_NONE"),
        talk("sit", "BOTH_SIT5"),
        talk("talk1", "BOTH_SIT5_TALK1"),
        talk("talk2", "BOTH_SIT5_TALK2"),
        talk("talk3", "BOTH_SIT5_TALK3"),
        body(id::LOOP, vec![float(-1.0)], forever),
    ];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    // Seated from the first frame; the 2500 ms wait stamped then ends on the first
    // frame past 4050 (4100, drawn from 4050). The talk is held its 2000 ms, its
    // task completing on frame 6100, where the stance comes back for that frame
    // before the script seats it again. The skeleton lacks BOTH_SIT5_TALK2, so its
    // task completes at once and the loop starts over on frame 14200.
    assert_eq!(
        changes(&t.legs)[..9],
        [
            ("BOTH_SIT5", 1550),
            ("BOTH_SIT5_TALK1", 4050),
            ("BOTH_STAND1", 6050),
            ("BOTH_SIT5", 6050),
            ("BOTH_SIT5_TALK3", 9600),
            ("BOTH_STAND1", 12600),
            ("BOTH_SIT5", 12600),
            ("BOTH_SIT5", 14150),
            ("BOTH_SIT5_TALK1", 16700),
        ]
    );
    assert_eq!(
        changes(&t.torso)[..2],
        [("BOTH_SIT5", 1550), ("BOTH_SIT5_TALK1", 4050)]
    );
    assert_eq!(at(&t, 12345), ("BOTH_SIT5_TALK3", "BOTH_SIT5_TALK3", 9600));
    // Seated again, it blends from the stance of that same frame.
    assert_eq!(t.blending_at(0, 6075), Some(("BOTH_STAND1", 6050, 0.25)));
    // An hour on, it still runs.
    assert!(t.legs.last().unwrap().start > 3_590_000);
}

#[test]
fn a_loop_of_none_runs_once() {
    // yavin1/jedi_stud_leaning's shape: its inner loops' random(0, 3) at the low end
    // run their body once (`CheckLoop` reads the count only at the body's end).
    let task =
        |name: &str, anim: &str| body(id::TASK, vec![text(name)], vec![set("SET_ANIM_BOTH", anim)]);
    let play = |name: &str| {
        vec![
            command(id::DO, vec![text(name)]),
            command(id::WAIT, vec![text(name)]),
        ]
    };
    let mut forever = vec![body(id::LOOP, random(0.0, 3.0).to_vec(), play("stand"))];
    forever.extend(play("talk1"));
    let script = vec![
        task("stand", "BOTH_SIT5"),
        task("talk1", "BOTH_SIT5_TALK1"),
        body(id::LOOP, vec![float(-1.0)], forever),
    ];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    // Each held its length less a frame (the first 200 ms less: `NPC_Begin`'s
    // movement frame), the stance coming back on the frame its task completes; then
    // the talk, and the stand once more.
    assert_eq!(
        changes(&t.legs)[..6],
        [
            ("BOTH_SIT5", 1550),
            ("BOTH_STAND1", 2300),
            ("BOTH_SIT5_TALK1", 2300),
            ("BOTH_STAND1", 4300),
            ("BOTH_SIT5", 4300),
            ("BOTH_STAND1", 5300),
        ]
    );
}

#[test]
fn a_script_draws_its_npc_with_more_light() {
    // Set in the first pass, set again (no change), then cleared by anything but
    // "true" after a 1000 ms wait.
    let script = vec![
        set("SET_MORELIGHT", "true"),
        set("SET_MORELIGHT", "TRUE"),
        command(id::WAIT, vec![float(1000.0)]),
        set("SET_MORELIGHT", "false"),
    ];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    assert_eq!(t.more_light, [(1550, true), (2550, false)]);
    assert!(!t.more_light_at(1549));
    assert!(t.more_light_at(1550));
    assert!(t.more_light_at(2549));
    assert!(!t.more_light_at(2550));
    // Without a script, never.
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), None, &holds);
    assert!(t.more_light.is_empty());
    assert!(!t.more_light_at(12345));
}

#[test]
fn a_script_puts_its_npcs_weapon_away() {
    // Put away in the first pass, again (no change), then armed after a 1000 ms
    // wait.
    let script = vec![
        set("SET_WEAPON", "WP_NONE"),
        set("SET_WEAPON", "wp_none"),
        command(id::WAIT, vec![float(1000.0)]),
        set("SET_WEAPON", "WP_SABER"),
    ];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    assert_eq!(t.unarmed, [(1550, true), (2550, false)]);
    assert!(!t.unarmed_at(1549));
    assert!(t.unarmed_at(1550));
    assert!(t.unarmed_at(2549));
    assert!(!t.unarmed_at(2550));
    // "drop" puts it away too.
    let script = vec![set("SET_WEAPON", "drop")];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    assert_eq!(t.unarmed, [(1550, true)]);
    // Without a script, armed.
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), None, &holds);
    assert!(!t.unarmed_at(12345));
}

#[test]
fn a_script_sets_and_clears_what_its_npc_looks_at() {
    // The look target is set in the first pass, set again (no change), then
    // cleared after a 1000 ms wait and set anew after another.
    let script = vec![
        set("SET_LOOK_TARGET", "npc_luke1"),
        set("SET_LOOK_TARGET", "npc_luke1"),
        command(id::WAIT, vec![float(1000.0)]),
        set("SET_LOOK_TARGET", "NULL"),
        command(id::WAIT, vec![float(1000.0)]),
        set("SET_LOOK_TARGET", "kyle"),
    ];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    // The waits end on the first frames past 2550 and 3600, drawn a frame earlier.
    assert_eq!(
        t.look,
        [
            (1550, Some("npc_luke1".to_owned())),
            (2550, None),
            (3600, Some("kyle".to_owned())),
        ]
    );
    assert_eq!(t.look_at(1549), None);
    assert_eq!(t.look_at(1550), Some("npc_luke1"));
    assert_eq!(t.look_at(3000), None);
    assert_eq!(t.look_at(100_000), Some("kyle"));
    // Without a script, nothing.
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), None, &holds);
    assert!(t.look.is_empty());
}

#[test]
fn conditions_compare_random_numbers_at_their_low_end() {
    // t3_byss/gonk_wander_1's choice: random(0, 1) > 0.8 is false.
    let script = vec![
        body(
            id::IF,
            [
                random(0.0, 1.0).to_vec(),
                vec![
                    Member {
                        id: id::GREATER_THAN,
                        data: Vec::new(),
                    },
                    float(0.8),
                ],
            ]
            .concat(),
            vec![set("SET_ANIM_BOTH", "BOTH_SIT5")],
        ),
        body(
            id::ELSE,
            Vec::new(),
            vec![set("SET_ANIM_BOTH", "BOTH_CIN_1")],
        ),
        hold_forever(),
    ];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    assert_eq!(changes(&t.legs), [("BOTH_CIN_1", 1550)]);
}

#[test]
fn a_first_pass_ends_at_its_first_wait() {
    let script = vec![
        set("SET_SOLID", "false"),
        set("SET_INVISIBLE", "true"),
        command(id::WAIT, vec![float(1000.0)]),
        set("SET_WEAPON", "WP_NONE"),
    ];
    assert_eq!(
        first_pass(&script),
        FirstPass {
            unarmed: false,
            invisible: true
        }
    );
}

#[test]
fn scripts_are_read_from_the_scripts_folder() {
    assert_eq!(
        script_path("academy2/studentSpawn5").as_deref(),
        Some("scripts/academy2/studentSpawn5.IBI")
    );
    assert_eq!(
        script_path("scripts/kor2/immortal").as_deref(),
        Some("scripts/kor2/immortal.IBI")
    );
    assert_eq!(script_path("NULL"), None);
}

fn named(name: &str) -> Pose {
    Pose {
        name: Some(name.to_owned()),
        ..stance("BOTH_STAND1", "BOTH_STAND1")
    }
}

/// `affect(name, FLUSH)` (56) or `INSERT` (55) with `nodes`.
fn affect(name: &str, insert: bool, nodes: Vec<Node>) -> Node {
    body(
        id::AFFECT,
        vec![text(name), float(if insert { 55.0 } else { 56.0 })],
        nodes,
    )
}

fn wait(ms: f32) -> Node {
    command(id::WAIT, vec![float(ms)])
}

/// A map whose player's spawn point uses one runner, named "intro", running `script`.
fn intro(script: Vec<Node>) -> Level {
    Level {
        runners: vec![Runner {
            targetname: Some("intro".to_owned()),
            script: Arc::new(script),
            delay: 0,
            number: 0,
        }],
        begin: vec!["intro".to_owned()],
        ..Default::default()
    }
}

/// The legs' animations before 7000 ms of the NPC named "luke", running
/// `spawnscript`, on a map running `level`, voice lines lasting 1234 ms.
fn luke(spawnscript: Vec<Node>, level: &Level) -> Vec<(String, i32)> {
    let pose = named("luke");
    let npc = Npc {
        stance: &pose,
        script: Some(&spawnscript),
        holds: &holds,
    };
    let t = timelines(&[npc], level, &|_| Some(1234)).npcs.remove(0);
    let early = t.legs.iter().filter(|c| c.start < 7000);
    early.map(|c| (c.anim.clone(), c.start)).collect()
}

fn owned(list: &[(&str, i32)]) -> Vec<(String, i32)> {
    list.iter().map(|&(a, t)| (a.to_owned(), t)).collect()
}

#[test]
fn an_intro_takes_over_an_npc_when_the_player_begins() {
    // The player begins after the 2000 ms frame; the intro runs from the next, its
    // affect running Luke's block at once (animations start then on the level's
    // clock, the client's still at 0; later ones a frame earlier for the client).
    // Its 1000 ms wait ends on the first frame past 3050; Luke, updated after the
    // runner, gets the signal in that frame.
    let level = intro(vec![
        affect(
            "LUKE",
            false,
            vec![
                set("SET_ANIM_BOTH", "BOTH_CIN_1"),
                command(id::WAITSIGNAL, vec![text("go")]),
                set("SET_ANIM_BOTH", "BOTH_SIT5"),
            ],
        ),
        wait(1000.0),
        command(id::SIGNAL, vec![text("go")]),
    ]);
    // The flush drops the rest of Luke's spawnscript. Each animation gives way to
    // the stance when its 1000 ms hold runs out; the one the affect set in the
    // runner's turn is counted down by Luke's movement later in the same frame, so
    // it gives way a frame sooner.
    let spawnscript = vec![
        hold_forever(),
        wait(5000.0),
        set("SET_ANIM_BOTH", "BOTH_STAND3"),
    ];
    assert_eq!(
        luke(spawnscript.clone(), &level),
        owned(&[
            ("BOTH_STAND1", 1550),
            ("BOTH_CIN_1", 2050),
            ("BOTH_STAND1", 2950),
            ("BOTH_SIT5", 3050),
            ("BOTH_STAND1", 4050)
        ])
    );
    // Without the intro, the spawnscript runs on.
    assert_eq!(
        luke(spawnscript, &Level::default()),
        owned(&[("BOTH_STAND1", 1550), ("BOTH_STAND3", 6550)])
    );
}

#[test]
fn the_first_camera_enable_is_kept() {
    // As t2_dpred's intro: it enables the camera (type 62) after a wait, from the
    // frame after the player begins (2050; a frame earlier for the client); a
    // zoom (58) and a second enable change nothing.
    let level = intro(vec![
        wait(100.0),
        command(id::CAMERA, vec![float(62.0)]),
        command(id::CAMERA, vec![float(58.0), float(80.0), float(0.0)]),
        wait(500.0),
        command(id::CAMERA, vec![float(62.0)]),
    ]);
    assert_eq!(timelines(&[], &level, &|_| None).camera, Some(2150));
    let zoom_only = intro(vec![command(id::CAMERA, vec![float(58.0)])]);
    assert_eq!(timelines(&[], &zoom_only, &|_| None).camera, None);
}

#[test]
fn a_run_script_runs_in_place_then_the_rest() {
    // As yavin1's opening: the intro waits 150 ms (from 2050, ending on the first
    // frame past 2200), then runs another script, whose affect sets Luke's
    // animation in that frame; the runner's own last affect comes after it.
    let mut level = intro(vec![
        wait(150.0),
        command(id::RUN, vec![text("Yavin1/Intro.ibi")]),
        affect("luke", false, vec![set("SET_ANIM_BOTH", "BOTH_SIT5")]),
    ]);
    level.runs.insert(
        "yavin1/intro".to_owned(),
        Arc::new(vec![
            affect("luke", false, vec![set("SET_ANIM_BOTH", "BOTH_CIN_1")]),
            wait(1000.0),
        ]),
    );
    // BOTH_CIN_1 from the 2250 frame (2200 for the client), then the run script's
    // 1000 ms wait (ending on the first frame past 3250) before the runner's own;
    // each 1000 ms hold, set in the runner's turn, gives way a frame sooner.
    assert_eq!(
        luke(vec![hold_forever()], &level),
        owned(&[
            ("BOTH_STAND1", 1550),
            ("BOTH_CIN_1", 2200),
            ("BOTH_STAND1", 3150),
            ("BOTH_SIT5", 3250),
            ("BOTH_STAND1", 4200)
        ])
    );
}

#[test]
fn an_inserted_block_runs_first_and_a_wait_it_cut_starts_over() {
    let level = intro(vec![affect(
        "luke",
        true,
        vec![set("SET_ANIM_BOTH", "BOTH_SIT5")],
    )]);
    // The spawnscript's 3000 ms wait from 1550 starts over after the block, at 2050:
    // it ends on the first frame past 5050.
    let spawnscript = vec![wait(3000.0), set("SET_ANIM_BOTH", "BOTH_STAND3")];
    assert_eq!(
        luke(spawnscript, &level),
        owned(&[
            ("BOTH_STAND1", 1550),
            ("BOTH_SIT5", 2050),
            ("BOTH_STAND1", 2950),
            ("BOTH_STAND3", 5050),
            ("BOTH_STAND1", 6050)
        ])
    );
}

#[test]
fn a_voice_line_holds_its_task_group_until_it_has_played() {
    // The line starts at 2050, plays from a frame later (2100) for 1234 ms: its
    // task completes on the first frame from 3334 (3350), before Luke's script
    // runs.
    let line = body(
        id::TASK,
        vec![text("line")],
        vec![command(
            id::SOUND,
            vec![text("CHAN_VOICE"), text("sound/luke.mp3")],
        )],
    );
    let level = intro(vec![affect(
        "luke",
        false,
        vec![
            line,
            command(id::DO, vec![text("line")]),
            command(id::WAIT, vec![text("line")]),
            set("SET_ANIM_BOTH", "BOTH_SIT5"),
        ],
    )]);
    assert_eq!(
        luke(vec![hold_forever()], &level),
        owned(&[
            ("BOTH_STAND1", 1550),
            ("BOTH_SIT5", 3300),
            ("BOTH_STAND1", 4300)
        ])
    );
}

#[test]
fn a_signal_waits_for_its_receiver_and_a_runner_needs_its_user() {
    // Luke waits for a signal the intro sends when it starts: sent before Luke's
    // update in that frame, received in it.
    let spawnscript = vec![
        hold_forever(),
        command(id::WAITSIGNAL, vec![text("go")]),
        set("SET_ANIM_BOTH", "BOTH_SIT5"),
    ];
    let level = intro(vec![command(id::SIGNAL, vec![text("go")])]);
    assert_eq!(
        luke(spawnscript.clone(), &level),
        owned(&[
            ("BOTH_STAND1", 1550),
            ("BOTH_SIT5", 2050),
            ("BOTH_STAND1", 3000)
        ])
    );
    // A runner nothing uses never runs; a delayed one starts on the first frame
    // from its use plus its delay.
    let mut unused = intro(vec![command(id::SIGNAL, vec![text("go")])]);
    unused.begin.clear();
    assert_eq!(
        luke(spawnscript.clone(), &unused),
        owned(&[("BOTH_STAND1", 1550)])
    );
    let mut delayed = intro(vec![command(id::SIGNAL, vec![text("go")])]);
    delayed.runners[0].delay = 520;
    assert_eq!(
        luke(spawnscript, &delayed),
        owned(&[
            ("BOTH_STAND1", 1550),
            ("BOTH_SIT5", 2500),
            ("BOTH_STAND1", 3500)
        ])
    );
}

/// `tag(name, ORIGIN)` as the compiler writes it.
fn move_to_tag_members(name: &str) -> [Member; 3] {
    let marker = Member {
        id: id::TAG,
        data: f32::MAX.to_le_bytes().to_vec(),
    };
    [marker, text(name), float(54.0)]
}

/// `move(tag(name, ORIGIN), ms)` as the compiler writes it.
fn move_to_tag(name: &str, ms: f32) -> Node {
    command(
        id::MOVE,
        [&move_to_tag_members(name)[..], &[float(ms)]].concat(),
    )
}

/// A task group `name` running `nodes`.
fn task(name: &str, nodes: Vec<Node>) -> Node {
    body(id::TASK, vec![text(name)], nodes)
}

/// `do(name)` then `wait(name)`.
fn do_and_wait(name: &str) -> [Node; 2] {
    [
        command(id::DO, vec![text(name)]),
        command(id::WAIT, vec![text(name)]),
    ]
}

/// t3_stamp's cranes (t3_stamp/crane_move): the intro waits 7 s, then has a
/// crane shuttle between two tags for good: an instant move back, 28 s forward.
fn crane_level(movers: &[(&str, [f32; 3], bool)]) -> Level {
    let shuttle = [
        vec![
            task("move1", vec![move_to_tag("CP1", 0.0)]),
            task("move2", vec![move_to_tag("cp2", 28000.0)]),
        ],
        do_and_wait("move1").into(),
        do_and_wait("move2").into(),
    ]
    .concat();
    let mut level = intro(vec![
        wait(7000.0),
        affect(
            "crane",
            false,
            vec![body(id::LOOP, vec![float(-1.0)], shuttle)],
        ),
    ]);
    level.tags.insert("cp1".to_owned(), [0.0, -1000.0, 0.0]);
    level.tags.insert("cp2".to_owned(), [0.0, 5000.0, 0.0]);
    level.movers = movers
        .iter()
        .map(|&(name, origin, linear)| Mover {
            name: name.to_owned(),
            origin,
            angles: [0.0; 3],
            linear,
        })
        .collect();
    level
}

#[test]
fn a_script_moves_a_mover_to_its_tags_each_move_waiting_for_the_last() {
    let level = crane_level(&[("crane", [0.0, 200.0, 0.0], false)]);
    let movers = timelines(&[], &level, &|_| None).movers;
    let slides = |m: &[Slide], n: usize| -> Vec<(i32, [f32; 3], [f32; 3], i32)> {
        m.iter()
            .take(n)
            .map(|s| (s.start, s.from, s.to, s.duration))
            .collect()
    };
    // The intro starts at 2050 and waits until 9100. An instant move takes 1 ms and
    // completes on the mover's next turn; the 28 s one on its turn at its end, when
    // the loop moves it back at once.
    let (cp1, cp2) = ([0.0, -1000.0, 0.0], [0.0, 5000.0, 0.0]);
    assert_eq!(
        slides(&movers[0].slides, 4),
        [
            (9100, [0.0, 200.0, 0.0], cp1, 1),
            (9150, cp1, cp2, 28000),
            (37150, cp2, cp1, 1),
            (37200, cp1, cp2, 28000),
        ]
    );
    assert!(!movers[0].slides[1].linear);
    assert_eq!(movers[0].turns, []);
    // Halfway in time, slowing to a stop: sin(45 degrees) of the way.
    let e = BrushEntity {
        slides: movers[0].slides.clone(),
        ..brush_at([0.0, 200.0, 0.0])
    };
    assert_eq!(e.origin_at(9000), [0.0, 200.0, 0.0]);
    let halfway = e.origin_at(9150 + 14000)[1];
    assert!((halfway - (-1000.0 + 6000.0 * std::f32::consts::FRAC_1_SQRT_2)).abs() < 0.01);
    assert_eq!(e.origin_at(37150), cp2);
    assert_eq!(e.origin_at(37151), cp1);
    // Of two movers with one name, the last spawned moves; a "linear" one at one
    // speed.
    let level = crane_level(&[("crane", [0.0; 3], false), ("CRANE", [0.0; 3], true)]);
    let movers = timelines(&[], &level, &|_| None).movers;
    assert!(movers[0].is_empty());
    assert!(movers[1].slides[1].linear);
}

/// A brush entity standing at `origin`, for its moves.
fn brush_at(origin: [f32; 3]) -> BrushEntity {
    BrushEntity {
        model: 1,
        origin,
        angles: [0.0; 3],
        axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        entity_box: [origin, origin],
        broadcast: false,
        shader_anim: false,
        model2: None,
        spin: None,
        door: None,
        script_name: None,
        linear: false,
        slides: Vec::new(),
        turns: Vec::new(),
    }
}

/// A vector as the compiler writes it: the marker, then its three numbers.
fn vector(v: [f32; 3]) -> [Member; 4] {
    let marker = Member {
        id: id::VECTOR,
        data: f32::MAX.to_le_bytes().to_vec(),
    };
    [marker, float(v[0]), float(v[1]), float(v[2])]
}

/// `rotate(<angles>, ms)`.
fn rotate(angles: [f32; 3], ms: f32) -> Node {
    command(id::ROTATE, [&vector(angles)[..], &[float(ms)]].concat())
}

/// A level whose intro has the mover "ship" (spawned at yaw 360) run `nodes`.
fn ship_level(nodes: Vec<Node>) -> Level {
    let mut level = intro(vec![affect("ship", false, nodes)]);
    level.tags.insert("land".to_owned(), [0.0, 0.0, -704.0]);
    level.movers = vec![Mover {
        name: "ship".to_owned(),
        origin: [0.0, 0.0, 1760.0],
        angles: [0.0, 360.0, 0.0],
        linear: false,
    }];
    level
}

/// Each move's or turn's start, ends and time.
fn spans(m: &[Slide]) -> Vec<(i32, [f32; 3], [f32; 3], i32)> {
    m.iter()
        .map(|s| (s.start, s.from, s.to, s.duration))
        .collect()
}

#[test]
fn t3_hevils_intro_ship_turns_as_it_lands() {
    // t3_hevil/playerSpawn: after 1500 ms the ship turns to yaw 135 over 4 s and
    // lands over 5 s, at once; the group then waits 5 s more.
    let level = ship_level(vec![
        task(
            "land",
            vec![
                wait(1500.0),
                rotate([0.0, 135.0, 0.0], 4000.0),
                move_to_tag("land", 5000.0),
                wait(5000.0),
            ],
        ),
        command(id::DO, vec![text("land")]),
        command(id::WAIT, vec![text("land")]),
        move_to_tag("land", 0.0),
    ]);
    let motion = &timelines(&[], &level, &|_| None).movers[0];
    // From the first frame after 2050 (the intro's first) + 1500. AngleSubtract
    // turns the short way: 135 - 360 is -225, so +135.
    assert_eq!(
        spans(&motion.turns),
        [(3600, [0.0, 360.0, 0.0], [0.0, 495.0, 0.0], 4000)]
    );
    // The group's tasks (the turn, the move, the wait) are done at 8650, the last
    // wait's first frame after 3600 + 5000; the next move starts then.
    assert_eq!(
        spans(&motion.slides),
        [
            (3600, [0.0, 0.0, 1760.0], [0.0, 0.0, -704.0], 5000),
            (8650, [0.0, 0.0, -704.0], [0.0, 0.0, -704.0], 1),
        ]
    );
}

#[test]
fn a_rotate_completes_as_its_time_runs_out_and_a_move_turns_by_its_angles() {
    let level = ship_level(vec![
        task("turn", vec![rotate([0.0, 90.0, 0.0], 1000.0)]),
        command(id::DO, vec![text("turn")]),
        command(id::WAIT, vec![text("turn")]),
        // move(tag, <0 0 0>, 2000): AngleDelta from 450 (-450, in 65536ths of a
        // turn: 270, so -90).
        task(
            "go",
            vec![command(
                id::MOVE,
                [
                    &move_to_tag_members("land")[..],
                    &vector([0.0; 3]),
                    &[float(2000.0)],
                ]
                .concat(),
            )],
        ),
        command(id::DO, vec![text("go")]),
        command(id::WAIT, vec![text("go")]),
        task("again", vec![rotate([0.0, 0.0, 10.0], 0.0)]),
        command(id::DO, vec![text("again")]),
        command(id::WAIT, vec![text("again")]),
        rotate([0.0, 0.0, 0.0], 500.0),
    ]);
    let motion = &timelines(&[], &level, &|_| None).movers[0];
    // The rotate's think at 2050 + 1000 completes it on the ship's turn then, and
    // the move starts at once; its turn completes with it, at 5050. A rotate of
    // no time takes 1 ms and completes on the next frame. Each turns from where
    // the last left it: yaw 360, which AngleSubtract takes as 0.
    assert_eq!(
        spans(&motion.turns),
        [
            (2050, [0.0, 360.0, 0.0], [0.0, 450.0, 0.0], 1000),
            (3050, [0.0, 450.0, 0.0], [0.0, 360.0, 0.0], 2000),
            (5050, [0.0, 360.0, 0.0], [0.0, 360.0, 10.0], 1),
            (5100, [0.0, 360.0, 10.0], [0.0, 360.0, 0.0], 500),
        ]
    );
    assert_eq!(spans(&motion.slides)[0].0, 3050);
}

/// hoth2's first elevator (Hoth2/Elevators/elevator1_up), its runner "t810" used
/// at `at`: it uses "el1_deact", then has the elevator wait a second and rise to
/// "tag_up1" over 4 s.
fn elevator_level(at: i32) -> Level {
    let mut level = intro(vec![
        command(id::USE, vec![text("el1_deact")]),
        affect(
            "elevator1",
            false,
            vec![wait(1000.0), move_to_tag("tag_up1", 4000.0)],
        ),
    ]);
    level.runners[0].targetname = Some("t810".to_owned());
    level.begin.clear();
    level.tags.insert("tag_up1".to_owned(), [0.0, 0.0, 928.0]);
    level.movers = vec![Mover {
        name: "elevator1".to_owned(),
        origin: [0.0, 0.0, 490.0],
        angles: [0.0; 3],
        linear: false,
    }];
    level.uses = vec![Use {
        name: "T810".to_owned(),
        at,
    }];
    level
}

#[test]
fn a_pinned_use_runs_its_runner_in_its_frame() {
    assert_eq!(
        (
            frame_at(1550),
            frame_at(2980),
            frame_at(3000),
            frame_at(3001)
        ),
        (1550, 3000, 3000, 3050)
    );
    // Used in the frame at 3000, before the runner's update: its script runs then,
    // its use in the same frame; the elevator's wait ends on the first frame past
    // 4000.
    let t = timelines(&[], &elevator_level(2980), &|_| None);
    let used: Vec<(&str, i32)> = t.used.iter().map(|u| (u.name.as_str(), u.at)).collect();
    assert_eq!(used, [("T810", 3000), ("el1_deact", 3000)]);
    assert_eq!(
        spans(&t.movers[0].slides),
        [(4050, [0.0, 0.0, 490.0], [0.0, 0.0, 928.0], 4000)]
    );
    // Nothing is used before the frame after the first.
    let t = timelines(&[], &elevator_level(0), &|_| None);
    assert_eq!(t.used[0].at, 1600);
}

fn turner(name: &str, yaw: f32, eyes: [f32; 3]) -> Pose {
    Pose {
        yaw,
        yaw_speed: 90.0,
        eyes,
        ..named(name)
    }
}

fn set_float(name: &str, value: f32) -> Node {
    command(id::SET, vec![text(name), float(value)])
}

/// An NPC's yaw after each frame that turns it ([`swjkja_world::script::Timeline::yaw`]).
type Yaws = Vec<(i32, f32)>;

/// The legs' animation starts of an NPC named "jaden" facing `yaw`, running
/// `script`, with another NPC named "luke" whose eyes are at 0 100 0; and the
/// yaws its turns leave it at.
fn turned(yaw: f32, script: Vec<Node>) -> (Vec<(String, i32)>, Yaws) {
    let jaden = turner("jaden", yaw, [0.0; 3]);
    let luke = turner("luke", 0.0, [0.0, 100.0, 0.0]);
    let npcs = [&jaden, &luke].map(|pose| Npc {
        stance: pose,
        script: pose
            .name
            .as_deref()
            .filter(|n| *n == "jaden")
            .map(|_| &script[..]),
        holds: &holds,
    });
    let t = timelines(&npcs, &Level::default(), &|_| None)
        .npcs
        .remove(0);
    let early = t.legs.iter().filter(|c| c.start < 5000);
    (early.map(|c| (c.anim.clone(), c.start)).collect(), t.yaw)
}

/// A script that does `turn` in a task group, waits for it, then plays BOTH_CIN_1.
fn turn_then_play(turn: Vec<Node>) -> Vec<Node> {
    let mut script = vec![task("turn", turn)];
    script.extend(do_and_wait("turn"));
    script.push(set("SET_ANIM_BOTH", "BOTH_CIN_1"));
    script
}

fn yaws(turns: &[(i32, f32)]) -> Vec<(i32, f32)> {
    turns
        .iter()
        .map(|&(t, y)| (t, (y * 1000.0).round() / 1000.0))
        .collect()
}

#[test]
fn npcs_turn_to_a_scripts_yaw_a_step_a_frame_and_go_on_once_there() {
    // From 325 to 270 at yaw speed 90: (60 + 3 * 90) / 20 = 16.5 degrees a frame,
    // the view in 65536ths of a turn, from the frame after the script set it.
    let (anims, turns) = turned(325.0, turn_then_play(vec![set_float("SET_DYAW", 270.0)]));
    assert_eq!(
        yaws(&turns),
        [
            (1600, -51.504),
            (1650, -68.005),
            (1700, -84.507),
            (1750, -90.0)
        ]
    );
    // The frame after it got there finds it exact and completes the turn: the
    // script goes on at 1800 (before the player begins, the client's clock is
    // the level's).
    assert_eq!(
        anims[..2],
        owned(&[("BOTH_STAND1", 1550), ("BOTH_CIN_1", 1800)])
    );
    // The shorter way round, at the speed a script sets: (60 + 3 * 10) / 20, each
    // step cut to the 65536th of a turn below (350 lands at -10.003, within
    // 0.01).
    let (_, turns) = turned(
        10.0,
        vec![
            set_float("SET_YAWSPEED", 10.0),
            set_float("SET_DYAW", 350.0),
        ],
    );
    assert_eq!(
        yaws(&turns),
        [
            (1600, 5.499),
            (1650, 1.0),
            (1700, -3.499),
            (1750, -7.998),
            (1800, -10.003)
        ]
    );
    // Already facing it: done the next frame, nothing turned.
    let (anims, turns) = turned(270.0, turn_then_play(vec![set_float("SET_DYAW", -90.0)]));
    assert_eq!(turns, []);
    assert_eq!(anims[1], ("BOTH_CIN_1".to_owned(), 1600));
}

#[test]
fn npcs_turn_to_face_a_view_target_from_their_eyes() {
    // Luke's eyes are straight up the y axis from Jaden's: yaw 90, in six frames.
    let (anims, turns) = turned(0.0, turn_then_play(vec![set("SET_VIEWTARGET", "LUKE")]));
    assert_eq!(turns.len(), 6);
    assert_eq!(turns.last(), Some(&(1850, 90.0)));
    assert_eq!(anims[1], ("BOTH_CIN_1".to_owned(), 1900));
    // A target no NPC is named: nothing to turn to, done the next frame.
    let (anims, turns) = turned(0.0, turn_then_play(vec![set("SET_VIEWTARGET", "nobody")]));
    assert_eq!(turns, []);
    assert_eq!(anims[1], ("BOTH_CIN_1".to_owned(), 1600));
}

#[test]
fn turns_are_drawn_from_frame_to_frame_the_shorter_way_round() {
    let t = swjkja_world::script::Timeline {
        legs: Vec::new(),
        torso: Vec::new(),
        look: Vec::new(),
        more_light: Vec::new(),
        unarmed: Vec::new(),
        yaw: vec![(1600, -170.0), (1650, 175.0)],
        moves: Vec::new(),
    };
    let slide = |start, from: f32, to: f32| Slide {
        start,
        from: [0.0, from, 0.0],
        to: [0.0, to, 0.0],
        duration: 50,
        linear: true,
    };
    // From 170 to -170 is 20 degrees on, then 15 back.
    assert_eq!(
        t.turns(170.0),
        [slide(1550, 170.0, 190.0), slide(1600, 190.0, 175.0)]
    );
}

/// academy3's Luke (`npc_luke1`): where he stands, facing 50, his box, yaw speed
/// 140, walking at 55.
fn walker() -> Pose {
    Pose {
        yaw: 50.0,
        yaw_speed: 140.0,
        origin: [-1344.0, -1338.0, 744.125],
        mins: [-15.0, -15.0, -24.0],
        maxs: [15.0, 15.0, 40.0],
        walk_speed: 55.0,
        run_speed: 200.0,
        ..named("luke")
    }
}

/// A map with academy3's "lukeNav1" (radius 12, raised 0.125) and a "far" tag 500
/// units east of Luke, nothing to bump into.
fn navgoals() -> Level {
    let mut level = Level::default();
    for (name, at) in [
        ("lukenav1", [-1292.0, -1510.0, 744.125]),
        ("far", [-844.0, -1338.0, 744.125]),
    ] {
        level.tags.insert(name.to_owned(), at);
        level.radii.insert(name.to_owned(), 12.0);
    }
    level
}

/// A script that sets the navgoal `name` in a task group, waits for it, then plays
/// BOTH_CIN_1 (as academy3's intro: "task lukeNav1").
fn walk_then_play(name: &str) -> Vec<Node> {
    let mut script = vec![task("walk", vec![set("SET_NAVGOAL", name)])];
    script.extend(do_and_wait("walk"));
    script.push(set("SET_ANIM_BOTH", "BOTH_CIN_1"));
    script
}

/// Luke's timeline, running `script` on [`navgoals`].
fn walked(script: Vec<Node>) -> swjkja_world::script::Timeline {
    walked_as(walker(), script)
}

/// The timeline of `pose`, running `script` on [`navgoals`].
fn walked_as(pose: Pose, script: Vec<Node>) -> swjkja_world::script::Timeline {
    let npc = Npc {
        stance: &pose,
        script: Some(&script),
        holds: &holds,
    };
    timelines(&[npc], &navgoals(), &|_| None).npcs.remove(0)
}

/// Where Luke is after the frame at `level`, or `None` if he did not move in it.
fn walked_to(t: &swjkja_world::script::Timeline, level: i32) -> Option<[f32; 3]> {
    let start = walker().origin;
    t.moves
        .iter()
        .find(|(l, _)| *l == level)
        .map(|(_, o)| [0, 1, 2].map(|k| start[k] + o[k]))
}

/// When Luke's legs start BOTH_CIN_1, if they do.
fn cin(t: &swjkja_world::script::Timeline) -> Option<i32> {
    t.legs
        .iter()
        .find(|c| c.anim == "BOTH_CIN_1")
        .map(|c| c.start)
}

#[test]
fn a_script_walks_its_npc_straight_to_a_navgoal() {
    // academy3's intro, traced in the original (`d_npcfreeze 3`): Luke's navgoal
    // set at 14,800, his first think at 14,850, here 13,250 ms earlier. He backs
    // off at three quarters of 55 (the goal lies behind him) and speeds up by 3 a
    // frame from 33 (friction takes 30, acceleration adds 0.6 of his speed).
    let t = walked(walk_then_play("lukeNav1"));
    let traced = [
        (1600, [-1343.644, -1339.1774, 744.125]),
        (1650, [-1343.2881, -1340.3547, 744.125]),
        (1700, [-1342.9408, -1341.5034, 744.125]),
        (1750, [-1342.5935, -1342.6521, 744.125]),
        (1800, [-1342.116, -1344.2314, 744.125]),
        (1850, [-1341.5951, -1345.9545, 744.125]),
        // The last frames, slowing within 48 of the goal.
        (5400, [-1296.9801, -1493.5278, 744.125]),
        (5450, [-1296.8065, -1494.1022, 744.125]),
    ];
    // The trace logs four decimals.
    for (level, traced) in traced {
        let at = walked_to(&t, level).unwrap_or_else(|| panic!("no move at {level}"));
        assert!(
            (0..3).all(|k| (at[k] - traced[k]).abs() < 0.001),
            "at {level}: {at:?}"
        );
    }
    // 5500 finds the goal inside his box (a unit larger), 16.7 away: the task
    // completes there (the script goes on: BOTH_CIN_1, a frame earlier on the
    // client's clock) and friction stops him where he was.
    assert_eq!(t.moves.len(), 78);
    assert_eq!(walked_to(&t, 5500), None);
    assert_eq!(cin(&t), Some(5450));
    // His view turns 24 degrees ((60 + 3 * 140) / 20) toward where he heads, only
    // on the frames his behavior thinks.
    assert_eq!(
        yaws(&t.yaw),
        [
            (1600, 25.999),
            (1700, 2.0),
            (1800, -22.0),
            (1900, -46.0),
            (2000, -69.999),
            (2100, -73.18)
        ]
    );
}

#[test]
fn a_navgoal_out_of_reach_or_cleared_moves_no_one() {
    // 500 units away, on a level without a nav graph, he finds no path and stops
    // (`STEER::Stop`): he stays, the task never completes, and he turns to his
    // last path's yaw, never set (0).
    let t = walked(walk_then_play("far"));
    assert_eq!((t.moves.len(), cin(&t)), (0, None));
    assert_eq!(yaws(&t.yaw), [(1600, 25.994), (1700, 1.989), (1800, 0.0)]);
    // "NULL" clears a goal set in a task group, completing its task.
    let mut script = vec![task("walk", vec![set("SET_NAVGOAL", "lukeNav1")])];
    script.push(command(id::DO, vec![text("walk")]));
    script.push(set("SET_NAVGOAL", "NULL"));
    script.extend(do_and_wait("walk")[1..].iter().cloned());
    script.push(set("SET_ANIM_BOTH", "BOTH_CIN_1"));
    let t = walked(script);
    assert_eq!(t.moves.len(), 0);
    assert_eq!(cin(&t), Some(1550));
}

#[test]
fn walking_npcs_walk_or_run_their_legs_forward_or_back() {
    // Luke backs off toward lukeNav1 at first (forwardmove < 0: PMF_BACKWARDS_RUN),
    // and walks forward once he has turned (his speed under 55 + 5: BUTTON_WALKING).
    // Armed, his torso keeps its stance while he backs up. As he arrives his
    // movement frame stands him, then his script plays BOTH_CIN_1 in the same frame.
    let t = walked(walk_then_play("lukeNav1"));
    assert_eq!(
        changes(&t.legs)[1..5],
        [
            ("BOTH_WALKBACK1", 1600),
            ("BOTH_WALK1", 1800),
            ("BOTH_STAND1", 5450),
            ("BOTH_CIN_1", 5450)
        ]
    );
    assert_eq!(
        changes(&t.torso)[1..4],
        [
            ("BOTH_WALK1", 1800),
            ("BOTH_STAND1", 5450),
            ("BOTH_CIN_1", 5450)
        ]
    );
    // Unarmed, it follows his legs back too; with nothing played after, both
    // return to the stance as he arrives.
    let mut script = vec![task("walk", vec![set("SET_NAVGOAL", "lukeNav1")])];
    script.extend(do_and_wait("walk"));
    let unarmed = Pose {
        unarmed: true,
        ..walker()
    };
    let t = walked_as(unarmed, script);
    for part in [&t.legs, &t.torso] {
        assert_eq!(
            changes(part)[1..4],
            [
                ("BOTH_WALKBACK1", 1600),
                ("BOTH_WALK1", 1800),
                ("BOTH_STAND1", 5450)
            ]
        );
    }
    // Running (SET_RUNNING), at 150 from his first think (MAX_FORCE), over 55 + 5.
    let mut script = vec![set("SET_RUNNING", "true")];
    script.extend(walk_then_play("lukeNav1"));
    let t = walked(script);
    assert_eq!(
        changes(&t.legs)[1..4],
        [
            ("BOTH_RUNBACK1", 1600),
            ("BOTH_RUN1", 1800),
            ("BOTH_STAND1", 2550)
        ]
    );
}

#[test]
fn runners_and_npcs_update_by_entity_number() {
    // The intro (entity 5) sets Luke's navgoal in the frame after the player
    // begins (2050). Luke updating after it thinks then; before it, a frame later
    // (t1_sour's intro walkers, 144 and 146, and its runner, 336).
    let first_move = |number: i32| {
        let mut level = intro(vec![affect("luke", false, walk_then_play("lukeNav1"))]);
        level.runners[0].number = 5;
        let goals = navgoals();
        level.tags = goals.tags;
        level.radii = goals.radii;
        let pose = Pose { number, ..walker() };
        let npc = Npc {
            stance: &pose,
            script: None,
            holds: &holds,
        };
        let t = timelines(&[npc], &level, &|_| None).npcs.remove(0);
        t.moves.first().map(|&(level, _)| level)
    };
    assert_eq!(first_move(7), Some(2050));
    assert_eq!(first_move(3), Some(2100));
}
