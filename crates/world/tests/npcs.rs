use swjkja_bsp::Entity;
use swjkja_world::collision::Collision;
use swjkja_world::npc::{attached_model, npc_gear, npc_look, npc_models, npc_spawns, saber_look};
use swjkja_world::rand::Rand;
use swjkja_world::static_model::{Attached, Pose, Rules, angles_to_axis};

fn entity(pairs: &[(&str, &str)]) -> Entity {
    Entity {
        pairs: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

fn npc(class: &str, extra: &[(&str, &str)]) -> Entity {
    let mut pairs = vec![("classname", class), ("origin", "1 2 3")];
    pairs.extend_from_slice(extra);
    entity(&pairs)
}

const SP: Rules = Rules::SinglePlayer { skill: 0 };

fn types(entities: &[Entity]) -> Vec<String> {
    npc_spawns(entities, SP, b"", &mut Rand::default())
        .into_iter()
        .map(|n| n.npc_type)
        .collect()
}

#[test]
fn spawners_pick_their_type_by_class_flags_and_key() {
    let entities = [
        entity(&[("classname", "worldspawn")]),
        npc("NPC_Stormtrooper", &[]),
        npc("NPC_Stormtrooper", &[("spawnflags", "2")]),
        npc("NPC_StormtrooperOfficer", &[]),
        // Jedi keep a type the spawner names; Tavion_New replaces it.
        npc("NPC_Jedi", &[("NPC_type", "jedi2")]),
        npc("NPC_Tavion_New", &[("NPC_type", "tavion_new_dark")]),
        // A key carrier, whatever the weapon flags say.
        npc(
            "NPC_Human_Merc",
            &[("spawnflags", "1"), ("message", "goodie")],
        ),
        // A random saber cultist: its strength and throwing drawn.
        npc("NPC_Cultist", &[("spawnflags", "1")]),
        // Saber cultists' "all" (4) is unreachable; powers ones add "2", but not
        // the throwing plain one.
        npc("NPC_Cultist_Saber", &[("spawnflags", "4")]),
        npc("NPC_Cultist_Saber_Powers", &[("spawnflags", "2")]),
        npc("NPC_Cultist_Saber_Powers", &[("spawnflags", "8")]),
        npc("NPC_Reborn_New", &[("spawnflags", "6")]),
        npc("NPC_spawner", &[("NPC_type", "Gran")]),
    ];
    assert_eq!(
        types(&entities),
        [
            // The first draw (0): the coin flip's "else".
            "StormTrooper2",
            "stcommander",
            "stofficer",
            "jedi2",
            "tavion_new",
            "human_merc_key",
            // The next two (0, 0): medium, not throwing.
            "cultist_saber_med",
            "cultist_saber",
            "cultist_saber_strong2",
            "cultist_saber_throw",
            "reborn_staff2",
            "Gran",
        ]
    );
}

#[test]
fn random_choices_draw_in_spawn_order() {
    // academy1's draws, logged by the reference capture: eight random Jedi (one
    // drawn twice: jedi_tf1 is the player's character's), then the player start's
    // saber, then the first NPC's colour as it spawns. A waiting spawner draws too.
    let jedi = || npc("NPC_Jedi", &[("spawnflags", "4")]);
    let entities = [
        jedi(),
        jedi(),
        jedi(),
        jedi(),
        jedi(),
        jedi(),
        npc("NPC_Jedi", &[("spawnflags", "4"), ("targetname", "later")]),
        jedi(),
        entity(&[("classname", "info_player_start")]),
    ];
    let mut rand = Rand::default();
    let spawns = npc_spawns(&entities, SP, b"single_1 { saberColor random }", &mut rand);
    let types: Vec<_> = spawns.iter().map(|n| n.npc_type.as_str()).collect();
    assert_eq!(
        types,
        [
            "jedi_kdm2",
            "jedi_hm1",
            "jedi_hf2",
            "jedi_hf2",
            "jedi_zf1",
            "jedi_rm2",
            "jedi_hf1"
        ]
    );
    let parms = b"jedi_hf1 { playerModel jedi_hf customRGBA jedi_hf }";
    let look = npc_look(parms, b"", "jedi_hf1", false, &mut rand).unwrap();
    assert_eq!(look.color, [255, 157, 114, 255]);
}

#[test]
fn saber_items_draw_before_the_npcs_spawn() {
    // As yavin1: the player's saber lying on a seat reads the player's saber's
    // block as it finishes spawning, before the NPC's block is read (one waiting
    // to be used does not).
    let parms = b"any { playerModel kyle customRGBA random }";
    let sabers = b"single_1 { saberColor random }";
    let saber = |extra: &[(&str, &str)]| {
        let mut pairs = vec![("classname", "weapon_saber"), ("saberType", "player")];
        pairs.extend_from_slice(extra);
        entity(&pairs)
    };
    let color = |entities: &[Entity]| {
        let models = npc_models(entities, SP, parms, sabers, &Collision::default(), &|_| {
            None
        });
        models[0].ghoul2.as_ref().unwrap().color
    };
    let any = npc("NPC_spawner", &[("NPC_type", "any")]);
    assert_eq!(color(std::slice::from_ref(&any)), [111, 63, 34, 255]);
    // Its draw comes first though it comes later in entity order: one draw on.
    assert_eq!(color(&[any.clone(), saber(&[])]), [63, 34, 189, 255]);
    assert_eq!(
        color(&[any, saber(&[("targetname", "later")])]),
        [111, 63, 34, 255]
    );
}

#[test]
fn an_npc_s_block_sets_its_colour() {
    let parms = b"
    plain { playerModel kyle }
    red {
        playerModel kyle
        customRGBA 255 0 0
    }
    any {
        playerModel kyle
        customRGBA random
    }
    me {
        playerModel player
        customRGBA 1 2 3 4
    }
    ";
    let color = |name| {
        npc_look(parms, b"", name, false, &mut Rand::default())
            .unwrap()
            .color
    };
    assert_eq!(color("plain"), [255; 4]);
    // The alpha left out stays white.
    assert_eq!(color("red"), [255, 0, 0, 255]);
    // Three draws, 0-255 each.
    assert_eq!(color("any"), [111, 63, 34, 255]);
    // The player's character takes the player's colour.
    assert_eq!(color("me"), [255; 4]);
}

#[test]
fn only_spawners_that_spawn_at_load_make_npcs() {
    let entities = [
        npc("NPC_Kyle", &[("angle", "90")]),
        // Waiting to be used.
        npc("NPC_Kyle", &[("targetname", "kyle")]),
        // Left out at the easiest skill (256).
        npc("NPC_Kyle", &[("spawnflags", "768")]),
        // Vehicles, Galak and a typeless spawner make none here.
        npc("NPC_Vehicle", &[("NPC_type", "swoop")]),
        npc("NPC_Galak", &[]),
        npc("NPC_spawner", &[]),
        npc("misc_model_static", &[]),
    ];
    let spawns = npc_spawns(&entities, SP, b"", &mut Rand::default());
    let [kyle] = &spawns[..] else {
        panic!("{spawns:?}")
    };
    assert_eq!(kyle.npc_type, "Kyle");
    assert_eq!(kyle.origin, [1.0, 2.0, 3.0]);
    assert_eq!(kyle.angles, [0.0, 90.0, 0.0]);
    assert!(
        npc_spawns(
            &entities,
            Rules::Multiplayer { gametype: 0 },
            b"",
            &mut Rand::default()
        )
        .is_empty()
    );
}

const PARMS: &[u8] = b"
Trooper
{
    playerModel stormtrooper
    weapon      WP_BLASTER
    customRGBA  255 0 0 255
    // A comment.
    height      64
    width       20
}
trooper
{
    playerModel other
}
Officer { playerModel stormtrooper customSkin officer scale 105 scaleZ 110
    surfOff \"torso_armor_neck_augment torso_body_neck_augment\" surfOn torso_pauldron
    weapon WP_BLASTER playerTeam TEAM_ENEMY
}
Commander
{
    playerModel imperial
    class       CLASS_IMPERIAL
    surfOff     l_arm_key
}
Mouse
{
    legsmodel   mouse
}
Rancor
{
    playerModel rancor
    class       CLASS_RANCOR
    height      200
}
Speeder
{
    playerModel swoop
    class       CLASS_VEHICLE
}
Player
{
    playerModel player
    surfOff     head
}
";

#[test]
fn looks_come_from_the_first_block_of_the_name() {
    // Matched ignoring case; the second \"trooper\" is never read. Unknown keys skip
    // their line.
    let t = npc_look(PARMS, b"", "TROOPER", false, &mut Rand::default()).unwrap();
    assert_eq!(
        (t.model.as_str(), t.skin.as_str()),
        ("stormtrooper", "default")
    );
    assert!(t.switches.is_empty());
    assert_eq!(t.scale, [1.0; 3]);
    // "width" sets the sides, "height" the top (24 below it is the bottom) and the
    // radius.
    assert_eq!(
        (t.mins, t.maxs),
        ([-20.0, -20.0, -24.0], [20.0, 20.0, 40.0])
    );
    assert_eq!(t.radius, 64.0);

    // Several keys on a line; the switches, off then on, split on whitespace.
    let o = npc_look(PARMS, b"", "officer", false, &mut Rand::default()).unwrap();
    assert_eq!(o.skin, "officer");
    assert_eq!(o.scale, [1.05, 1.05, 1.1]);
    assert_eq!(
        o.switches,
        [
            ("torso_armor_neck_augment".into(), true),
            ("torso_body_neck_augment".into(), true),
            ("torso_pauldron".into(), false),
        ]
    );
    // No height: the default box's largest extent.
    assert_eq!(o.radius, 40.0);

    // An imperial with a key shows its sleeve after the block's switches.
    let c = npc_look(PARMS, b"", "Commander", true, &mut Rand::default()).unwrap();
    assert_eq!(
        c.switches,
        [("l_arm_key".into(), true), ("l_arm_key".into(), false)]
    );
    assert_eq!(
        npc_look(PARMS, b"", "Commander", false, &mut Rand::default())
            .unwrap()
            .switches
            .len(),
        1
    );

    // A rancor's radius is fixed by its class.
    assert_eq!(
        npc_look(PARMS, b"", "rancor", false, &mut Rand::default())
            .unwrap()
            .radius,
        150.0
    );
    // The player's character, with no switches.
    let p = npc_look(PARMS, b"", "player", false, &mut Rand::default()).unwrap();
    assert_eq!(
        (p.model.as_str(), p.skin.as_str()),
        ("jedi_tf", "head_a1|torso_a1|lower_a1")
    );
    assert!(p.switches.is_empty());
    // MD3 NPCs, vehicles and unknown names are not drawn.
    assert_eq!(
        npc_look(PARMS, b"", "mouse", false, &mut Rand::default()),
        None
    );
    assert_eq!(
        npc_look(PARMS, b"", "speeder", false, &mut Rand::default()),
        None
    );
    assert_eq!(
        npc_look(PARMS, b"", "nobody", false, &mut Rand::default()),
        None
    );
}

#[test]
fn npcs_turn_at_their_blocks_yaw_speed_stormtroopers_slower() {
    let parms = b"Trooper { playerModel stormtrooper class CLASS_STORMTROOPER yawSpeed 120 }
        Guard { playerModel imperial yawSpeed 50 }
        Droid { playerModel imperial yawSpeed 0 }";
    let entities = ["trooper", "guard", "droid"].map(|t| npc("NPC_spawner", &[("NPC_type", t)]));
    let models = npc_models(&entities, SP, parms, b"", &Collision::default(), &|_| None);
    let speeds: Vec<f32> = models
        .iter()
        .map(|m| m.ghoul2.as_ref().unwrap().pose.as_ref().unwrap().yaw_speed)
        .collect();
    // Three quarters for a stormtrooper at the easiest skill; a speed of 0 is
    // ignored.
    assert_eq!(speeds, [90.0, 50.0, 90.0]);
}

#[test]
fn npcs_walk_and_run_at_their_blocks_speeds() {
    let parms = b"Luke { playerModel luke walkSpeed 55 runSpeed 200 }
        Still { playerModel imperial walkSpeed 0 runSpeed -5 }
        Plain { playerModel imperial }";
    let entities = ["luke", "still", "plain"].map(|t| npc("NPC_spawner", &[("NPC_type", t)]));
    let models = npc_models(&entities, SP, parms, b"", &Collision::default(), &|_| None);
    let speeds: Vec<(f32, f32)> = models
        .iter()
        .map(|m| {
            let pose = m.ghoul2.as_ref().unwrap().pose.as_ref().unwrap();
            (pose.walk_speed, pose.run_speed)
        })
        .collect();
    // 0 is kept, less than 0 ignored; 90 and 300 by default.
    assert_eq!(speeds, [(55.0, 200.0), (0.0, 300.0), (90.0, 300.0)]);
}

#[test]
fn npcs_stand_upright_at_their_spawner_scaled_from_their_feet() {
    let entities = [
        npc(
            "NPC_spawner",
            &[("NPC_type", "officer"), ("angles", "30 90 10")],
        ),
        npc("NPC_Player", &[]),
    ];
    let models = npc_models(&entities, SP, PARMS, b"", &Collision::default(), &|_| None);
    let [officer, player] = &models[..] else {
        panic!("{models:?}")
    };
    assert_eq!(officer.model, "models/players/stormtrooper/model.glm");
    // 24 units higher per unit of height scale above 1.
    assert!((officer.origin[2] - (3.0 + 24.0 * 0.1)).abs() < 1e-5);
    // Yaw only, each axis scaled.
    let axis = angles_to_axis([0.0, 90.0, 0.0]);
    for ((got, want), s) in officer.axes.iter().zip(axis).zip([1.05, 1.05, 1.1]) {
        assert!(got.iter().zip(want).all(|(g, w)| (g - w * s).abs() < 1e-5));
    }
    assert!(!officer.normalized);
    // Linked with the default box (single player's, 16 units out) around the
    // spawner, an epsilon larger.
    assert_eq!(
        officer.entity_box,
        Some([[-16.0, -15.0, -22.0], [18.0, 19.0, 44.0]])
    );
    let g = officer.ghoul2.as_ref().unwrap();
    assert_eq!(
        g.skin.as_deref(),
        Some("models/players/stormtrooper/model_officer.skin")
    );
    // A blaster in the right hand, legs apart, the torso holding it idle; facing
    // its spawner's yaw, turning, walking and running at the default speeds, its
    // eyes 4 units below its box's top (from its origin before the scale lifts it).
    // Entity 3: the two spawners hold 1 and 2 as it spawns.
    assert_eq!(
        g.pose,
        Some(Pose {
            number: 3,
            yaw: 90.0,
            yaw_speed: 90.0,
            eyes: [1.0, 2.0, 39.0],
            origin: [1.0, 2.0, 3.0],
            mins: [-16.0, -16.0, -24.0],
            maxs: [16.0, 16.0, 40.0],
            walk_speed: 90.0,
            run_speed: 300.0,
            ..pose("BOTH_STAND3", "TORSO_WEAPONIDLE3")
        })
    );
    assert_eq!(
        g.attached,
        [Attached {
            model: "models/weapons2/blaster_r/blaster_w.glm".into(),
            bolt: "*r_hand",
            skin: None
        }]
    );
    // A three-part skin names the folder and the parts.
    assert_eq!(player.model, "models/players/jedi_tf/model.glm");
    assert_eq!(
        player.ghoul2.as_ref().unwrap().skin.as_deref(),
        Some("models/players/jedi_tf/|head_a1|torso_a1|lower_a1")
    );
    assert_eq!(player.origin, [1.0, 2.0, 3.0]);
    assert!(player.normalized);
}

const GEAR: &[u8] = b"
jedi { playerModel jedi_hm weapon WP_SABER playerTeam TEAM_PLAYER class CLASS_JEDI
    saber single saber2 wrist }
staff { playerModel jedi_hm weapon WP_SABER playerTeam TEAM_PLAYER class CLASS_JEDI
    saber staff saber2 single }
imperial { playerModel imperial weapon WP_BLASTER_RIFLE playerTeam TEAM_ENEMY
    class CLASS_IMPERIAL }
ugnaught_imp { playerModel imperial playerTeam enemy class CLASS_IMPERIAL weapon WP_NONE }
cultist { playerModel cultist weapon WP_BLASTER_PISTOL playerTeam TEAM_ENEMY
    class CLASS_REBORN rank ltcomm }
ugnaught { playerModel ugnaught weapon WP_BLASTER playerTeam TEAM_NEUTRAL }
noghri { playerModel noghri weapon WP_NOGHRI_STICK playerTeam TEAM_FREE }
";

const SABERS: &[u8] = b"
single { name \"one\" saberModel models/weapons2/saber_1/saber_1.glm }
wrist { saberModel \"models/weapons2/saber_2/saber_2.glm\" boltToWrist 1
    customSkin models/weapons2/saber_2/red.skin }
staff { saberModel models/weapons2/saber_dual_1/saber_dual_1.glm twoHanded 1 }
";

fn gear(npc_type: &str, flags: i64) -> (Pose, Vec<(String, &'static str)>) {
    let look = npc_look(GEAR, SABERS, npc_type, false, &mut Rand::default()).unwrap();
    let (pose, attached) = npc_gear(&look, npc_type, flags, SABERS);
    let held = attached.into_iter().map(|a| (a.model, a.bolt)).collect();
    (pose, held)
}

fn pose(legs: &'static str, torso: &'static str) -> Pose {
    Pose {
        legs,
        torso,
        still_since: 0,
        script: None,
        name: None,
        bad_bones: false,
        // A humanoid's: walks carry it 50 a second, runs 150.
        strides: Some([50.0, 150.0]),
        ..Pose::default()
    }
}

#[test]
fn npcs_hold_their_weapons_and_stand_with_them() {
    // Two sabers, blades off: hilts in both hands (one at the wrist, with its skin),
    // the torso following the legs.
    let look = npc_look(GEAR, SABERS, "jedi", false, &mut Rand::default()).unwrap();
    let (p, attached) = npc_gear(&look, "jedi", 0, SABERS);
    assert_eq!(p, pose("BOTH_STAND1", "BOTH_STAND1"));
    assert_eq!(
        attached,
        [
            Attached {
                model: "models/weapons2/saber_1/saber_1.glm".into(),
                bolt: "*r_hand",
                skin: None
            },
            Attached {
                model: "models/weapons2/saber_2/saber_2.glm".into(),
                bolt: "*l_hand_cap_l_arm",
                skin: Some("models/weapons2/saber_2/red.skin".into())
            },
        ]
    );
    // A two-handed saber takes no second.
    let (_, held) = gear("staff", 0);
    assert_eq!(
        held,
        [(
            "models/weapons2/saber_dual_1/saber_dual_1.glm".into(),
            "*r_hand"
        )]
    );
    // An unknown weapon name: the team's weapon for the type, a pistol; legs apart.
    assert_eq!(
        gear("imperial", 0),
        (
            pose("BOTH_STAND3", "TORSO_WEAPONIDLE2"),
            vec![(
                "models/weapons2/blaster_pistol/blaster_pistol_w.glm".into(),
                "*r_hand"
            )]
        )
    );
    // No weapon for this type from its team (an ugnaught): an imperial's stance.
    assert_eq!(
        gear("ugnaught_imp", 0),
        (
            Pose {
                unarmed: true,
                ..pose("BOTH_STAND4", "BOTH_STAND4")
            },
            vec![]
        )
    );
    // A reborn officer's pistols, one in each hand, arms down.
    let pistol = "models/weapons2/blaster_pistol/blaster_pistol_w.glm";
    assert_eq!(
        gear("cultist", 0),
        (
            pose("BOTH_STAND3", "BOTH_STAND1"),
            vec![(pistol.into(), "*r_hand"), (pistol.into(), "*l_hand")]
        )
    );
    // Neutrals hold nothing, though they stand as with their weapon.
    assert_eq!(
        gear("ugnaught", 0),
        (pose("BOTH_STAND3", "TORSO_WEAPONIDLE3"), vec![])
    );
    // A weapon past the detonation pack is no gun to the legs.
    assert_eq!(
        gear("noghri", 0),
        (
            pose("BOTH_STAND1", "TORSO_WEAPONIDLE3"),
            vec![(
                "models/weapons2/noghri_stick/noghri_stick_w.glm".into(),
                "*r_hand"
            )]
        )
    );
}

#[test]
fn weapon_models_become_their_ghoul2_models() {
    assert_eq!(
        attached_model("models/weapons2/blaster_r/blaster.md3"),
        "models/weapons2/blaster_r/blaster_w.glm"
    );
    assert_eq!(
        attached_model("models/weapons2/saber/saber_w.md3"),
        "models/weapons2/saber/saber_w.glm"
    );
    assert_eq!(
        attached_model("models/weapons2/noweap/noweap.md3"),
        "models/weapons2/noweap/noweap.glm"
    );
    assert_eq!(attached_model("models/a/b.glm"), "models/a/b.glm");
    // A saber no block names keeps the default hilt.
    assert_eq!(
        saber_look(SABERS, "nothing").model,
        "models/weapons2/saber_reborn/saber_w.glm"
    );
}
