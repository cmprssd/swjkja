use swjkja_bsp::Entity;
use swjkja_world::collision::Collision;
use swjkja_world::npc::{
    attached_model, npc_gear, npc_look, npc_models, npc_spawns, saber_look, standing_at,
};
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
    npc_spawns(entities, SP)
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
        // A random saber cultist: the first strength, throwing.
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
            "StormTrooper",
            "stcommander",
            "stofficer",
            "jedi2",
            "tavion_new",
            "human_merc_key",
            "cultist_saber_med_throw",
            "cultist_saber",
            "cultist_saber_strong2",
            "cultist_saber_throw",
            "reborn_staff2",
            "Gran",
        ]
    );
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
    let spawns = npc_spawns(&entities, SP);
    let [kyle] = &spawns[..] else {
        panic!("{spawns:?}")
    };
    assert_eq!(kyle.npc_type, "Kyle");
    assert_eq!(kyle.origin, [1.0, 2.0, 3.0]);
    assert_eq!(kyle.angles, [0.0, 90.0, 0.0]);
    assert!(npc_spawns(&entities, Rules::Multiplayer { gametype: 0 }).is_empty());
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
    let t = npc_look(PARMS, "TROOPER", false).unwrap();
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
    let o = npc_look(PARMS, "officer", false).unwrap();
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
    let c = npc_look(PARMS, "Commander", true).unwrap();
    assert_eq!(
        c.switches,
        [("l_arm_key".into(), true), ("l_arm_key".into(), false)]
    );
    assert_eq!(
        npc_look(PARMS, "Commander", false).unwrap().switches.len(),
        1
    );

    // A rancor's radius is fixed by its class.
    assert_eq!(npc_look(PARMS, "rancor", false).unwrap().radius, 150.0);
    // The player's character, with no switches.
    let p = npc_look(PARMS, "player", false).unwrap();
    assert_eq!(
        (p.model.as_str(), p.skin.as_str()),
        ("jedi_tf", "head_a1|torso_a1|lower_a1")
    );
    assert!(p.switches.is_empty());
    // MD3 NPCs, vehicles and unknown names are not drawn.
    assert_eq!(npc_look(PARMS, "mouse", false), None);
    assert_eq!(npc_look(PARMS, "speeder", false), None);
    assert_eq!(npc_look(PARMS, "nobody", false), None);
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
    let models = npc_models(&entities, SP, PARMS, b"", &Collision::default());
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
    // Linked with the block's box around the spawner, an epsilon larger.
    assert_eq!(
        officer.entity_box,
        Some([[-15.0, -14.0, -22.0], [17.0, 18.0, 44.0]])
    );
    let g = officer.ghoul2.as_ref().unwrap();
    assert_eq!(
        g.skin.as_deref(),
        Some("models/players/stormtrooper/model_officer.skin")
    );
    // A blaster in the right hand, legs apart, the torso holding it idle.
    assert_eq!(g.pose, Some(pose("BOTH_STAND3", "TORSO_WEAPONIDLE3")));
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
    let look = npc_look(GEAR, npc_type, false).unwrap();
    let (pose, attached) = npc_gear(&look, npc_type, flags, SABERS);
    let held = attached.into_iter().map(|a| (a.model, a.bolt)).collect();
    (pose, held)
}

fn pose(legs: &'static str, torso: &'static str) -> Pose {
    let idle = match legs {
        "BOTH_STAND1" => Some("BOTH_STAND1IDLE1"),
        "BOTH_STAND3" => Some("BOTH_STAND3IDLE1"),
        _ => None,
    };
    Pose {
        legs,
        torso,
        idle,
        still_since: 0,
    }
}

#[test]
fn npcs_hold_their_weapons_and_stand_with_them() {
    // Two sabers, blades off: hilts in both hands (one at the wrist, with its skin),
    // the torso following the legs.
    let look = npc_look(GEAR, "jedi", false).unwrap();
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
        (pose("BOTH_STAND4", "BOTH_STAND4"), vec![])
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

#[test]
fn npcs_standing_still_play_their_idle_after_five_seconds() {
    // BOTH_STAND3IDLE1: 151 frames at 20 per second, held for 150 of them.
    let idle = Some((7500, 7550));
    let still = pose("BOTH_STAND3", "TORSO_WEAPONIDLE3");
    let at = |p: &Pose, time| standing_at(p, idle, time);
    // The stance from the first frame; the idle from the first frame past 5 s of
    // level time (5050), on the client's clock a frame behind.
    assert_eq!(at(&still, 4999), ("BOTH_STAND3", "TORSO_WEAPONIDLE3", 1550));
    assert_eq!(
        at(&still, 5000),
        ("BOTH_STAND3IDLE1", "BOTH_STAND3IDLE1", 5000)
    );
    assert_eq!(
        at(&still, 12345),
        ("BOTH_STAND3IDLE1", "BOTH_STAND3IDLE1", 5000)
    );
    // Held until 12550, then the stance until 5 s after the idle's full length.
    assert_eq!(
        at(&still, 12500),
        ("BOTH_STAND3", "TORSO_WEAPONIDLE3", 12500)
    );
    assert_eq!(
        at(&still, 17599),
        ("BOTH_STAND3", "TORSO_WEAPONIDLE3", 12500)
    );
    assert_eq!(
        at(&still, 17600),
        ("BOTH_STAND3IDLE1", "BOTH_STAND3IDLE1", 17600)
    );
    // One that fell moved on the first frame: its idle comes 1550 ms later.
    let fell = Pose {
        still_since: 1550,
        ..still.clone()
    };
    assert_eq!(at(&fell, 6549).2, 1550);
    assert_eq!(
        at(&fell, 6550),
        ("BOTH_STAND3IDLE1", "BOTH_STAND3IDLE1", 6550)
    );
    // No idle animation (the stance has none, or the skeleton lacks it).
    let imperial = pose("BOTH_STAND4", "BOTH_STAND4");
    assert_eq!(at(&imperial, 12345), ("BOTH_STAND4", "BOTH_STAND4", 1550));
    assert_eq!(standing_at(&still, None, 12345).2, 1550);
}
