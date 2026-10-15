use swjkja_icarus::{Block, Member, Node, id};
use swjkja_world::script::{Change, FirstPass, first_pass, script_path, timeline};
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
    }
}

/// Animations' holds and lengths (ms): (frames - 1) and frames at 20 per second.
fn holds(name: &str) -> Option<(i32, i32)> {
    let frames = match name {
        "BOTH_STAND1" | "BOTH_STAND3" | "TORSO_WEAPONIDLE3" | "BOTH_SIT5" | "BOTH_CIN_1" => 21,
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
    // Held for its 1000 ms: the stance returns on the frame its timer runs out
    // (2550, drawn from 2500), and the idle comes 5 s after the last frame that held
    // it (2500 + 5000, the first frame past it 7550).
    let script = vec![set("SET_ANIM_BOTH", "BOTH_CIN_1")];
    let t = timeline(&stance("BOTH_STAND1", "BOTH_STAND1"), Some(&script), &holds);
    assert_eq!(
        changes(&t.legs)[..5],
        [
            ("BOTH_CIN_1", 1550),
            ("BOTH_STAND1", 2500),
            ("BOTH_STAND1IDLE1", 7500),
            ("BOTH_STAND1", 9450),
            ("BOTH_STAND1IDLE1", 14550),
        ]
    );
    assert_eq!(t.blending_at(0, 2550), Some(("BOTH_CIN_1", 1550, 0.5)));
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
