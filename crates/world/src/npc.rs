//! Single player's NPCs as the map spawns them (`code/game/NPC_spawn.cpp`: the
//! `SP_NPC_*` spawn functions, `SP_NPC_spawner`, `NPC_Spawn_Do`) and how they look
//! (`NPC_stats.cpp`: `NPC_ParseParms` reading the `ext_data/npcs/*.npc` blocks;
//! `g_client.cpp`: `G_SetG2PlayerModel`), drawn by cgame as players
//! (`code/cgame/cg_players.cpp`: `CG_Player`).
//!
//! Only the NPCs a map spawns by itself are here: a spawner with a "targetname"
//! waits to be used, and the ones it spawns depend on scripts. Each stands where
//! its spawner is, as it spawned, dropped to the floor: the game then runs its AI
//! and scripts.
//!
//! What the spawn picks at random is the original's pick: the game's generator
//! ([`crate::rand`]) is replayed through the spawn functions, the ammo racks
//! placing their goods, the saber items finishing their spawn, and each NPC's
//! block.

use swjkja_bsp::Entity;
use swjkja_shader::text::Cursor;

use crate::Vec3;
use crate::collision::{Collision, MASK_NPCSOLID, MIN_WORLD_COORD};
use crate::entity_number::entity_numbers;
use crate::rand::Rand;
use crate::script::{FIRST_FRAME, FirstPass, first_pass};
use crate::static_model::{
    Attached, Ghoul2, Pose, Rules, StaticModel, ammo_rack_goods, angles_to_axis, gun_rack_goods,
    origin_and_angles, spawns_at,
};
use crate::style::atoi;
use std::sync::Arc;
use swjkja_icarus::Node;

/// An NPC spawner's type, as its spawn function picks it.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcSpawn {
    /// The `.npc` block it reads, as named (looked up ignoring case).
    pub npc_type: String,
    pub origin: Vec3,
    pub angles: Vec3,
    /// Whether the spawner has a "message" (a key the NPC carries).
    pub message: bool,
    /// The spawner's "spawnflags", which the NPC keeps.
    pub flags: i64,
    /// The script it runs as it begins ("spawnscript"), unless that names a
    /// behavior state (`G_ActivateBehavior` sets those instead).
    pub spawnscript: Option<String>,
    /// The name scripts find the NPC by ("NPC_targetname", its `targetname`).
    pub targetname: Option<String>,
    /// The NPC's entity number ([`crate::entity_number`]).
    pub number: i32,
}

/// The spawn function's choice of type for spawner class `class` (lowercase) with
/// `flags`, `key` its "NPC_type" and `message` whether it has one, drawing its
/// random choices from `rand`. `None` for the spawners that make no NPC here:
/// vehicles (another model, chosen by `vehicles.dat`), `NPC_Galak` (spawns
/// nothing), and an `NPC_spawner` without a type (a random one). A random saber
/// cultist's spawn function sets its flags anew.
fn npc_type(
    class: &str,
    flags: &mut i64,
    key: Option<&str>,
    message: bool,
    rand: &mut Rand,
) -> Option<String> {
    let flags_in = *flags;
    let f = |bit: i64| flags_in & bit != 0;
    // The spawn functions that keep a type the spawner names.
    let keeps_key = matches!(
        class,
        "npc_spawner"
            | "npc_jedi"
            | "npc_prisoner"
            | "npc_rebel"
            | "npc_human_merc"
            | "npc_rockettrooper"
            | "npc_hazardtrooper"
            | "npc_ugnaught"
            | "npc_jawa"
            | "npc_gran"
            | "npc_rodian"
            | "npc_weequay"
            | "npc_trandoshan"
            | "npc_tusken"
            | "npc_noghri"
            | "npc_swamptrooper"
            | "npc_imperial"
            | "npc_bespincop"
            | "npc_reborn"
            | "npc_colombian_soldier"
            | "npc_colombian_rebel"
            | "npc_reborn_new"
            | "npc_cultist_saber"
            | "npc_cultist_saber_powers"
            | "npc_cultist"
            | "npc_cultist_commando"
            | "npc_shadowtrooper"
            | "npc_colombian_emplacedgunner"
            | "npc_saboteur"
            | "npc_droid_assassin"
            | "npc_droid_saber"
    );
    if keeps_key && let Some(key) = key {
        return Some(key.to_owned());
    }
    // SP_NPC_Cultist_Saber(_Powers): medium (1), strong (2), or neither; throwing
    // (8). Its "all" branch tests 2 again, so 4 is never reached, and the
    // throwing plain one has no "2".
    let cultist_saber = |flags: i64, powers: bool| {
        let f = |bit: i64| flags & bit != 0;
        let name = match (f(1), f(2), f(8)) {
            (true, _, true) => "cultist_saber_med_throw",
            (true, _, false) => "cultist_saber_med",
            (false, true, true) => "cultist_saber_strong_throw",
            (false, true, false) => "cultist_saber_strong",
            (false, false, true) => return "cultist_saber_throw".to_owned(),
            (false, false, false) => "cultist_saber",
        };
        if powers {
            format!("{name}2")
        } else {
            name.to_owned()
        }
    };
    let name = match class {
        "npc_player" => "Player",
        "npc_kyle" if f(1) => "Kyle_boss",
        "npc_kyle" => "Kyle",
        "npc_lando" => "Lando",
        "npc_jan" => "Jan",
        "npc_luke" => "Luke",
        "npc_monmothma" => "MonMothma",
        "npc_rosh_penin" if f(1) => "rosh_dark",
        "npc_rosh_penin" if f(2) => "rosh_penin_noforce",
        "npc_rosh_penin" => "rosh_penin",
        "npc_tavion" => "Tavion",
        "npc_tavion_new" if f(1) => "tavion_scepter",
        "npc_tavion_new" if f(2) => "tavion_sith_sword",
        "npc_tavion_new" => "tavion_new",
        "npc_alora" if f(1) => "alora_dual",
        "npc_alora" => "alora",
        "npc_reelo" => "Reelo",
        "npc_desann" | "npc_manuel_vergara_rmg" => "Desann",
        "npc_rax" => "Rax",
        "npc_bobafett" => "Boba_Fett",
        "npc_ragnos" => "Ragnos",
        "npc_lannik_racto" => "lannik_racto",
        "npc_kothos" if f(1) => "VKothos",
        "npc_kothos" => "DKothos",
        "npc_chewbacca" => "Chewie",
        "npc_bartender" => "Bartender",
        "npc_morgankatarn" => "MorganKatarn",
        "npc_jedi" if f(4) => random_jedi(rand),
        "npc_jedi" if f(2) => "jedimaster",
        "npc_jedi" if f(1) => "jeditrainer",
        "npc_jedi" => rand.flip("Jedi", "Jedi2"),
        "npc_prisoner" if f(1) => rand.flip("elder", "elder2"),
        "npc_prisoner" => rand.flip("Prisoner", "Prisoner2"),
        "npc_merchant" => "merchant",
        "npc_rebel" => "Rebel",
        "npc_human_merc" if message => "human_merc_key",
        "npc_human_merc" if f(1) => "human_merc_bow",
        "npc_human_merc" if f(2) => "human_merc_rep",
        "npc_human_merc" if f(4) => "human_merc_flc",
        "npc_human_merc" if f(8) => "human_merc_cnc",
        "npc_human_merc" => "human_merc",
        "npc_stormtrooper" if f(8) => "rockettrooper",
        "npc_stormtrooper" if f(4) => "stofficeralt",
        "npc_stormtrooper" if f(2) => "stcommander",
        "npc_stormtrooper" | "npc_stormtrooperofficer" if f(1) => "stofficer",
        "npc_stormtrooperofficer" => "stofficer",
        "npc_stormtrooper" => rand.flip("StormTrooper", "StormTrooper2"),
        "npc_snowtrooper" => "snowtrooper",
        "npc_tie_pilot" => "stormpilot",
        "npc_rockettrooper" if f(1) => "rockettrooper2Officer",
        "npc_rockettrooper" => "rockettrooper2",
        "npc_hazardtrooper" if f(1) => "hazardtrooperofficer",
        "npc_hazardtrooper" if f(2) => "hazardtrooperconcussion",
        "npc_hazardtrooper" => "hazardtrooper",
        "npc_ugnaught" => rand.flip("Ugnaught", "Ugnaught2"),
        "npc_jawa" if f(1) => "jawa_armed",
        "npc_jawa" => "jawa",
        "npc_gran" if f(1) => "granshooter",
        "npc_gran" if f(2) => "granboxer",
        "npc_gran" => rand.flip("gran", "gran2"),
        "npc_rodian" if f(1) => "rodian2",
        "npc_rodian" => "rodian",
        "npc_weequay" => ["Weequay", "Weequay2", "Weequay3", "Weequay4"][rand.irand(0, 3) as usize],
        "npc_trandoshan" => "Trandoshan",
        "npc_tusken" if f(1) => "tuskensniper",
        "npc_tusken" => "tusken",
        "npc_noghri" => "noghri",
        "npc_swamptrooper" if f(1) => "SwampTrooper2",
        "npc_swamptrooper" => "SwampTrooper",
        "npc_imperial" if f(1) => "ImpOfficer",
        "npc_imperial" if f(2) => "ImpCommander",
        "npc_imperial" => "Imperial",
        "npc_impworker" => "ImpWorker",
        "npc_bespincop" => rand.flip("BespinCop2", "BespinCop"),
        "npc_reborn" | "npc_colombian_soldier" | "npc_colombian_rebel" => match () {
            _ if f(1) => "rebornforceuser",
            _ if f(2) => "rebornfencer",
            _ if f(4) => "rebornacrobat",
            _ if f(8) => "rebornboss",
            _ => "reborn",
        },
        "npc_reborn_new" => match (f(8), f(4), f(1), f(2)) {
            (true, _, true, _) => "RebornMasterDual",
            (true, _, _, true) => "RebornMasterStaff",
            (true, ..) => "RebornMaster",
            (_, true, true, _) => "reborn_dual2",
            (_, true, _, true) => "reborn_staff2",
            (_, true, ..) => "reborn_new2",
            (_, _, true, _) => "reborn_dual",
            (_, _, _, true) => "reborn_staff",
            _ => "reborn_new",
        },
        "npc_cultist_saber" => return Some(cultist_saber(flags_in, false)),
        "npc_cultist_saber_powers" => return Some(cultist_saber(flags_in, true)),
        // A random saber cultist (1): medium, strong or "all" (4, which ends up
        // neither), and throwing or not.
        "npc_cultist" if f(1) => {
            *flags = [1, 2, 4][rand.irand(0, 2) as usize];
            *flags |= rand.flip(8, 0);
            return Some(cultist_saber(*flags, false));
        }
        "npc_cultist" if f(2) => "cultist_grip",
        "npc_cultist" if f(4) => "cultist_lightning",
        "npc_cultist" if f(8) => "cultist_drain",
        "npc_cultist" => "cultist",
        "npc_cultist_commando" => "cultistcommando",
        "npc_cultist_destroyer" => "cultist_destroyer",
        "npc_shadowtrooper" | "npc_colombian_emplacedgunner" => {
            rand.flip("ShadowTrooper2", "ShadowTrooper")
        }
        "npc_saboteur" if f(1) => "saboteursniper",
        "npc_saboteur" if f(2) => "saboteurpistol",
        "npc_saboteur" if f(4) => "saboteurcommando",
        "npc_saboteur" => "saboteur",
        "npc_monster_murjj" => "Murjj",
        "npc_monster_swamp" => "Swamp",
        "npc_monster_howler" => "howler",
        "npc_monster_rancor" if f(1) => "mutant_rancor",
        "npc_monster_rancor" => "rancor",
        "npc_monster_mutant_rancor" => "mutant_rancor",
        "npc_monster_wampa" => "wampa",
        "npc_minemonster" => "minemonster",
        "npc_monster_claw" => "Claw",
        "npc_monster_glider" => "Glider",
        "npc_monster_flier2" => "Flier2",
        "npc_monster_lizard" => "Lizard",
        "npc_monster_fish" => "Fish",
        "npc_monster_sand_creature" if f(1) => "sand_creature_fast",
        "npc_monster_sand_creature" => "sand_creature",
        "npc_droid_interrogator" => "interrogator",
        "npc_droid_probe" => "probe",
        "npc_droid_mark1" => "mark1",
        "npc_droid_mark2" => "mark2",
        "npc_droid_atst" => "atst",
        "npc_droid_remote" => "remote_sp",
        "npc_droid_seeker" => "seeker",
        "npc_droid_sentry" => "sentry",
        "npc_droid_gonk" => "gonk",
        "npc_droid_mouse" => "mouse",
        "npc_droid_r2d2" if f(1) => "r2d2_imp",
        "npc_droid_r2d2" => "r2d2",
        "npc_droid_r5d2" if f(1) => "r5d2_imp",
        "npc_droid_r5d2" => "r5d2",
        "npc_droid_protocol" if f(1) => "protocol_imp",
        "npc_droid_protocol" => "protocol",
        "npc_droid_assassin" => "assassin_droid",
        "npc_droid_saber" if f(1) => "saber_droid_training",
        "npc_droid_saber" => "saber_droid",
        _ => return None,
    };
    Some(name.to_owned())
}

/// `SP_NPC_Jedi`'s random Jedi (spawn flag 4): one of twelve, drawn again (up to
/// 20 draws) while it is one of the player's character's (`g_char_model`,
/// "jedi_tf" by default).
fn random_jedi(rand: &mut Rand) -> &'static str {
    const JEDI: [&str; 12] = [
        "jedi_hf1",
        "jedi_hf2",
        "jedi_hm1",
        "jedi_hm2",
        "jedi_kdm1",
        "jedi_kdm2",
        "jedi_rm1",
        "jedi_rm2",
        "jedi_tf1",
        "jedi_tf2",
        "jedi_zf1",
        "jedi_zf2",
    ];
    let mut name = JEDI[0];
    for _ in 0..20 {
        name = JEDI[rand.irand(0, 11) as usize];
        if !name.contains(PLAYER_MODEL) {
            break;
        }
    }
    name
}

/// The NPCs `entities` spawn as the map loads under `rules` (none in multiplayer), in
/// entity order: the spawners not left out by skill and not waiting to be used (no
/// "targetname").
///
/// The map's spawn functions draw from `rand` in entity order ([`spawn_draws`],
/// and the gun racks' spread). Other entities' draws are left out.
pub fn npc_spawns(
    entities: &[Entity],
    rules: Rules,
    sabers: &[u8],
    rand: &mut Rand,
) -> Vec<NpcSpawn> {
    let Rules::SinglePlayer { skill } = rules else {
        return Vec::new();
    };
    let numbers = entity_numbers(entities, skill);
    let mut spawns = Vec::new();
    for (i, e) in entities
        .iter()
        .enumerate()
        .filter(|(_, e)| spawns_at(e, skill))
    {
        let Some(class) = e.get("classname").map(str::to_ascii_lowercase) else {
            continue;
        };
        if class == "misc_model_gun_rack" {
            gun_rack_goods(e, rand);
            continue;
        }
        let Some((npc_type, flags)) = spawn_draws(e, &class, sabers, rand) else {
            continue;
        };
        if e.get("targetname").is_some() {
            continue;
        }
        let (origin, angles) = origin_and_angles(e);
        let spawnscript = e
            .get("spawnscript")
            .filter(|s| !s.is_empty() && !s.to_ascii_uppercase().starts_with("BS_"))
            .map(str::to_owned);
        let targetname = e.get("NPC_targetname").map(str::to_owned);
        spawns.push(NpcSpawn {
            npc_type,
            origin,
            angles,
            message: e.get("message").is_some(),
            flags,
            spawnscript,
            targetname,
            number: numbers.npc[i].unwrap_or(0),
        });
    }
    spawns
}

/// The draws entity `e` (its class `class`, in lower case) makes from `rand` as it
/// spawns, but a rack's: an NPC spawner's random type (a waiting spawner's too),
/// returned with the spawnflags it then has, or a player start's reading the
/// player's saber's block in `sabers` (the `.sab` files' text) to precache it
/// (`SP_info_player_deathmatch`; not with the stun baton, flag 32).
pub(crate) fn spawn_draws(
    e: &Entity,
    class: &str,
    sabers: &[u8],
    rand: &mut Rand,
) -> Option<(String, i64)> {
    let mut flags = e.get("spawnflags").map_or(0, atoi);
    if matches!(class, "info_player_start" | "info_player_deathmatch") {
        if flags & 32 == 0 {
            draw_saber_colors(sabers, PLAYER_SABER, rand);
        }
        return None;
    }
    let message = e.get("message").is_some();
    let npc_type = npc_type(class, &mut flags, e.get("NPC_type"), message, rand)?;
    Some((npc_type, flags))
}

/// The draws the ammo racks `entities` spawn under `rules` make placing their goods
/// (`spawn_rack_goods`, 100 ms into the level; in entity order).
fn ammo_rack_draws(entities: &[Entity], rules: Rules, rand: &mut Rand) {
    let Rules::SinglePlayer { skill } = rules else {
        return;
    };
    for e in entities.iter().filter(|e| spawns_at(e, skill)) {
        if e.get("classname")
            .is_some_and(|c| c.eq_ignore_ascii_case("misc_model_ammo_rack"))
        {
            ammo_rack_goods(e, rand);
        }
    }
}

/// The draws the saber items `entities` spawn under `rules` make as they finish
/// spawning (`FinishSpawningItem`, the frame after 1250 ms; in entity order): each
/// `weapon_saber` naming a saber ("saberType"; "player" the player's) reads its
/// block in `sabers` for its model, but one waiting to be used ("targetname").
fn item_saber_draws(entities: &[Entity], rules: Rules, sabers: &[u8], rand: &mut Rand) {
    let Rules::SinglePlayer { skill } = rules else {
        return;
    };
    for e in entities.iter().filter(|e| spawns_at(e, skill)) {
        if !e
            .get("classname")
            .is_some_and(|c| c.eq_ignore_ascii_case("weapon_saber"))
            || e.get("targetname").is_some()
        {
            continue;
        }
        match e.get("saberType") {
            Some(t) if t.eq_ignore_ascii_case("player") => {
                draw_saber_colors(sabers, PLAYER_SABER, rand);
            }
            Some(t) if !t.is_empty() => draw_saber_colors(sabers, t, rand),
            _ => {}
        }
    }
}

/// The draws reading saber `name`'s block in `sabers` makes: one per colour it
/// picks at random (`Q_irand(SABER_ORANGE, SABER_PURPLE)`).
fn draw_saber_colors(sabers: &[u8], name: &str, rand: &mut Rand) {
    for _ in 0..saber_look(sabers, name).random_colors {
        rand.irand(1, 5);
    }
}

/// The `.npc` files' texts as `NPC_LoadParms` joins them, in the order given (the
/// game's file listing of `ext_data/npcs`): each up to its first NUL, a line break
/// between. The `.sab` files join the same way for their tokens
/// (`WP_SaberLoadParms`).
pub fn join_parms<'a>(files: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut parms = Vec::new();
    for file in files {
        parms.extend_from_slice(&file[..file.iter().position(|&b| b == 0).unwrap_or(file.len())]);
        parms.push(b'\n');
    }
    parms
}

/// The inside of the first block named `name` (ignoring case) in `text`, as
/// `NPC_ParseParms` and `WP_SaberParseParms` find it: past its "{". `None` if no
/// block has the name, or it has no "{".
fn block<'a>(text: &'a [u8], name: &str) -> Option<Cursor<'a>> {
    let mut p = Cursor::new(text);
    loop {
        let token = p.token(true);
        if token.is_empty() {
            return None;
        }
        if token.eq_ignore_ascii_case(name.as_bytes()) {
            break;
        }
        p.skip_braced_section(0);
    }
    (p.token(true) == b"{").then_some(p)
}

/// `DEFAULT_MINS_2`: an NPC's box always starts 24 units below its origin.
const DEFAULT_MINS_2: f32 = -24.0;
/// `playerMins` and `playerMaxs`, the box an NPC has unless its block sets one
/// (single player's `DEFAULT_MINS_0` is -16; multiplayer's -15).
const PLAYER_MINS: Vec3 = [-16.0, -16.0, DEFAULT_MINS_2];
const PLAYER_MAXS: Vec3 = [16.0, 16.0, 40.0];
/// The model and skin of the player's character, which an NPC of type "Player"
/// wears (`G_InitPlayerFromCvars`, with `g_char_model` and the `g_char_skin_*` at
/// their defaults).
const PLAYER_MODEL: &str = "jedi_tf";
const PLAYER_SKIN: &str = "head_a1|torso_a1|lower_a1";
/// The saber the player's character carries (`g_saber`'s default).
const PLAYER_SABER: &str = "single_1";

/// The colour sets "customRGBA" names, one drawn at random (`NPC_ParseParms`).
pub const PALETTES: [(&str, &[[u8; 3]]); 7] = [
    (
        "random1",
        &[
            [127, 153, 255],
            [177, 29, 13],
            [47, 90, 40],
            [181, 207, 255],
            [138, 83, 0],
            [254, 199, 14],
        ],
    ),
    (
        "jedi_hf",
        &[
            [165, 48, 21],
            [254, 230, 132],
            [181, 207, 255],
            [233, 183, 208],
            [161, 226, 240],
            [101, 159, 255],
            [255, 157, 114],
            [216, 160, 255],
        ],
    ),
    (
        "jedi_hm",
        &[
            [252, 243, 180],
            [69, 109, 255],
            [254, 197, 73],
            [178, 78, 18],
            [112, 153, 161],
            [123, 182, 255],
            [0, 88, 105],
            [138, 0, 0],
        ],
    ),
    (
        "jedi_kdm",
        &[
            [85, 120, 255],
            [173, 142, 219],
            [254, 197, 73],
            [138, 83, 0],
            [254, 199, 14],
            [68, 194, 217],
            [170, 3, 30],
            [225, 226, 144],
            [167, 202, 255],
        ],
    ),
    (
        "jedi_rm",
        &[
            [127, 153, 255],
            [208, 249, 85],
            [181, 207, 255],
            [138, 83, 0],
            [224, 171, 44],
            [49, 155, 131],
            [163, 79, 17],
            [148, 104, 228],
            [138, 136, 0],
        ],
    ),
    (
        "jedi_tf",
        &[
            [255, 235, 100],
            [62, 155, 255],
            [255, 110, 120],
            [180, 150, 255],
            [255, 200, 212],
            [255, 255, 255],
        ],
    ),
    (
        "jedi_zf",
        &[
            [204, 19, 21],
            [255, 107, 40],
            [255, 148, 155],
            [255, 164, 59],
            [216, 160, 255],
            [101, 159, 255],
            [161, 226, 240],
            [37, 155, 181],
        ],
    ),
];

/// What an NPC's `.npc` block says about how it looks.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcLook {
    /// The folder of its Ghoul2 model under `models/players/`.
    pub model: String,
    /// "default", another skin's name, or three parts "head|torso|lower".
    pub skin: String,
    /// Surface switches in the order the original applies them: (name, off).
    pub switches: Vec<(String, bool)>,
    /// `s.modelScale`.
    pub scale: Vec3,
    pub mins: Vec3,
    pub maxs: Vec3,
    /// `s.radius`, which the renderer culls and levels it by.
    pub radius: f32,
    /// "class", lowercase ("class_stormtrooper"), or "" for none.
    pub class: String,
    pub team: Team,
    /// "rank", in the order of `rank_t` (0 a civilian, the default).
    pub rank: usize,
    /// "weapon", an index into [`WEAPONS`] (0 for none or an unknown name).
    pub weapon: usize,
    /// "saber" and "saber2": the names of their `.sab` blocks.
    pub sabers: [Option<String>; 2],
    /// `customRGBA`, which cgame draws it with (`shaderRGBA`): white, or the
    /// block's "customRGBA": its numbers, a colour of a set ([`PALETTES`]) or three
    /// random components ("random"). The player's character is white (the
    /// `g_char_color_*` defaults).
    pub color: [u8; 4],
    /// "yawSpeed" (`stats.yawSpeed`): 90 unless the block gives more than 0.
    pub yaw_speed: f32,
    /// "walkSpeed" and "runSpeed" (`stats.walkSpeed`, `runSpeed`): 90 and 300
    /// unless the block gives 0 or more.
    pub walk_speed: f32,
    pub run_speed: f32,
}

/// "playerTeam" (`team_t`): any other name is no team the game knows, which acts
/// as [`Team::Free`] does here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Team {
    Free,
    Player,
    Enemy,
    Neutral,
}

/// `NPC_ParseParms` for `npc_type`, reading the `.npc` files' text `parms` (all of
/// them, in order: the first block of a name wins), for an NPC whose spawner has a
/// "message" or not. `None` if no block has the name, or it gives no Ghoul2 model
/// ("playerModel"): the original's MD3 NPCs and vehicles are not drawn here.
///
/// Its random choices are drawn from `rand` as the block is read: "customRGBA"'s,
/// and the colours of the sabers it names (their blocks in `sabers`, the `.sab`
/// files' text; [`SaberLook::random_colors`]; a second saber only beside a
/// one-handed first), and a "saberColor" (or "saber2Color", or either with a blade
/// number) of "random". The player's character then takes the player's saber.
/// No block, no draws.
pub fn npc_look(
    parms: &[u8],
    sabers_text: &[u8],
    npc_type: &str,
    message: bool,
    rand: &mut Rand,
) -> Option<NpcLook> {
    let mut p = block(parms, npc_type)?;
    let mut model = None;
    let mut skin = "default".to_owned();
    let (mut off, mut on) = (String::new(), String::new());
    let mut scale = [1.0; 3];
    let (mut mins, mut maxs) = (PLAYER_MINS, PLAYER_MAXS);
    let mut radius = 0;
    let mut class = String::new();
    let mut team = Team::Free;
    let mut rank = 0;
    let mut weapon = 0;
    let mut sabers = [None, None];
    let mut color = [255; 4];
    let mut yaw_speed = 90.0;
    let (mut walk_speed, mut run_speed) = (90.0, 300.0);
    let mut two_handed = false;
    loop {
        let token = p.token(true);
        if token.is_empty() {
            return None;
        }
        if token == b"}" {
            break;
        }
        let key = String::from_utf8_lossy(token).to_ascii_lowercase();
        // COM_ParseString and COM_ParseInt: a value on the same line, or the key
        // is dropped.
        let value = String::from_utf8_lossy(p.token(false)).into_owned();
        let number = || (!value.is_empty()).then(|| atoi(&value));
        match key.as_str() {
            "playermodel" if !value.is_empty() => model = Some(value),
            "customskin" if !value.is_empty() => skin = value,
            "surfoff" | "surfon" if !value.is_empty() => {
                let list = if key == "surfoff" { &mut off } else { &mut on };
                if !list.is_empty() {
                    list.push(',');
                }
                list.push_str(&value);
            }
            "scale" | "scalex" | "scaley" | "scalez" => {
                if let Some(n) = number().filter(|&n| n >= 0 && n != 100) {
                    let s = n as f32 / 100.0;
                    match key.as_str() {
                        "scalex" => scale[0] = s,
                        "scaley" => scale[1] = s,
                        "scalez" => scale[2] = s,
                        _ => scale = [s; 3],
                    }
                }
            }
            "width" => {
                if let Some(n) = number() {
                    (mins[0], mins[1], maxs[0], maxs[1]) =
                        (-n as f32, -n as f32, n as f32, n as f32);
                }
            }
            "height" => {
                if let Some(n) = number() {
                    maxs[2] = n as f32 + DEFAULT_MINS_2;
                    radius = n;
                }
            }
            "class" if !value.is_empty() => class = value.to_ascii_lowercase(),
            // GetIDForString(TeamTable): -1 for an unknown name.
            "playerteam" if !value.is_empty() => {
                team = match value.to_ascii_lowercase().as_str() {
                    "player" | "team_player" => Team::Player,
                    "enemy" | "team_enemy" => Team::Enemy,
                    "neutral" | "team_neutral" => Team::Neutral,
                    _ => Team::Free,
                }
            }
            // TranslateRankName: a civilian for an unknown name.
            "yawspeed" => {
                if let Some(n) = number().filter(|&n| n > 0) {
                    yaw_speed = n as f32;
                }
            }
            "walkspeed" => {
                if let Some(n) = number().filter(|&n| n >= 0) {
                    walk_speed = n as f32;
                }
            }
            "runspeed" => {
                if let Some(n) = number().filter(|&n| n >= 0) {
                    run_speed = n as f32;
                }
            }
            "rank" if !value.is_empty() => {
                rank = RANKS
                    .iter()
                    .position(|r| r.eq_ignore_ascii_case(&value))
                    .unwrap_or(0)
            }
            // GetIDForString(WPTable): an unknown name changes nothing.
            "weapon" if !value.is_empty() => {
                if let Some(w) = weapon_id(&value) {
                    weapon = w;
                }
            }
            "saber" if !value.is_empty() => {
                two_handed = saber_look(sabers_text, &value).two_handed;
                draw_saber_colors(sabers_text, &value, rand);
                sabers[0] = Some(value);
            }
            "saber2" if !value.is_empty() => {
                if !two_handed {
                    draw_saber_colors(sabers_text, &value, rand);
                }
                sabers[1] = Some(value);
            }
            "customrgba" if !value.is_empty() => {
                let lower = value.to_ascii_lowercase();
                if lower == "random" {
                    color = [0, 0, 0, 255];
                    for c in &mut color[..3] {
                        *c = rand.irand(0, 255) as u8;
                    }
                } else if let Some((_, set)) = PALETTES.iter().find(|(name, _)| *name == lower) {
                    let [r, g, b] = set[rand.irand(0, set.len() as i32 - 1) as usize];
                    color = [r, g, b, 255];
                } else {
                    // COM_ParseInt: each further number on the line, or the rest
                    // stay as they were.
                    color[0] = atoi(&value) as u8;
                    for c in &mut color[1..] {
                        let n = p.token(false);
                        if n.is_empty() {
                            break;
                        }
                        *c = atoi(&String::from_utf8_lossy(n)) as u8;
                    }
                }
            }
            _ if saber_color_key(&key) && !value.is_empty() => {
                if value.eq_ignore_ascii_case("random") {
                    rand.irand(1, 5);
                }
            }
            // Values on the line after the key, unread: the rest of the line.
            _ if value.is_empty() => {}
            _ => p.skip_rest_of_line(),
        }
    }
    let model = model?;
    if class == "class_vehicle" {
        return None;
    }
    // The player's character takes no switches (G_ChangePlayerModel passes none).
    let player = model.eq_ignore_ascii_case("player");
    if player {
        (off, on) = (String::new(), String::new());
        // G_InitPlayerFromCvars: the player's saber, and colour.
        draw_saber_colors(sabers_text, PLAYER_SABER, rand);
        color = [255; 4];
    }
    let (model, skin) = if player {
        (PLAYER_MODEL.to_owned(), PLAYER_SKIN.to_owned())
    } else {
        (model, skin)
    };
    // G_SetG2PlayerModelInfo: surfOff, then surfOn, then an imperial's key sleeve.
    let mut switches = Vec::new();
    for (list, turn_off) in [(&off, true), (&on, false)] {
        let mut words = Cursor::new(list.as_bytes());
        loop {
            let word = words.token(true);
            if word.is_empty() {
                break;
            }
            switches.push((String::from_utf8_lossy(word).into_owned(), turn_off));
        }
    }
    if class == "class_imperial" && message {
        switches.push(("l_arm_key".to_owned(), false));
    }
    // The radius: fixed for some classes, else "height", else the box's largest
    // extent. The spawner's flags reach the NPC only later, so a rancor's is never
    // a mutant's.
    let radius = match class.as_str() {
        "class_sand_creature" => 256.0,
        "class_rancor" => 150.0,
        _ if radius > 0 => radius as f32,
        _ => {
            let extent = mins.iter().map(|m| -m).chain(maxs).fold(f32::MIN, f32::max) as i32;
            if extent == 0 { 60.0 } else { extent as f32 }
        }
    };
    Some(NpcLook {
        model,
        skin,
        switches,
        scale,
        mins,
        maxs,
        radius,
        class,
        team,
        rank,
        weapon,
        sabers,
        color,
        yaw_speed,
        walk_speed,
        run_speed,
    })
}

/// Whether `key` (lowercase) sets a saber's blade colours in an NPC's block:
/// "saberColor" or "saber2Color", alone or with a blade number from 2 to 8.
fn saber_color_key(key: &str) -> bool {
    ["sabercolor", "saber2color"].iter().any(|k| {
        key.strip_prefix(k)
            .is_some_and(|n| n.is_empty() || matches!(n.as_bytes(), [b'2'..=b'8']))
    })
}

/// The weapons (`weapon_t`) in order, each with its model in `weapons.dat` ("" for
/// none): a weapon's number is its index.
pub const WEAPONS: [(&str, &str); 29] = [
    ("WP_NONE", ""),
    ("WP_SABER", "models/weapons2/saber/saber_w.md3"),
    (
        "WP_BLASTER_PISTOL",
        "models/weapons2/blaster_pistol/blaster_pistol.md3",
    ),
    ("WP_BLASTER", "models/weapons2/blaster_r/blaster.md3"),
    ("WP_DISRUPTOR", "models/weapons2/disruptor/disruptor.md3"),
    ("WP_BOWCASTER", "models/weapons2/bowcaster/bowcaster.md3"),
    (
        "WP_REPEATER",
        "models/weapons2/heavy_repeater/heavy_repeater.md3",
    ),
    ("WP_DEMP2", "models/weapons2/demp2/demp2.md3"),
    ("WP_FLECHETTE", "models/weapons2/golan_arms/golan_arms.md3"),
    (
        "WP_ROCKET_LAUNCHER",
        "models/weapons2/merr_sonn/merr_sonn.md3",
    ),
    ("WP_THERMAL", "models/weapons2/thermal/thermal.md3"),
    ("WP_TRIP_MINE", "models/weapons2/laser_trap/laser_trap.md3"),
    ("WP_DET_PACK", "models/weapons2/detpack/det_pack.md3"),
    ("WP_CONCUSSION", "models/weapons2/concussion/c_rifle.md3"),
    ("WP_MELEE", "models/weapons2/noweap/noweap.md3"),
    ("WP_ATST_MAIN", "models/weapons2/noweap/noweap.md3"),
    ("WP_ATST_SIDE", "models/weapons2/noweap/noweap.md3"),
    ("WP_STUN_BATON", "models/weapons2/stun_baton/baton.md3"),
    (
        "WP_BRYAR_PISTOL",
        "models/weapons2/briar_pistol/briar_pistol.md3",
    ),
    ("WP_EMPLACED_GUN", "models/weapons2/noweap/noweap.md3"),
    ("WP_BOT_LASER", "models/weapons2/noweap/noweap.md3"),
    ("WP_TURRET", "models/weapons2/noweap/noweap.md3"),
    ("WP_TIE_FIGHTER", "models/weapons2/noweap/noweap.md3"),
    ("WP_RAPID_FIRE_CONC", "models/weapons2/noweap/noweap.md3"),
    ("WP_JAWA", "models/weapons2/jawa/jawa_gun.md3"),
    (
        "WP_TUSKEN_RIFLE",
        "models/weapons2/tusken_rifle/tusken_rifle.md3",
    ),
    (
        "WP_TUSKEN_STAFF",
        "models/weapons2/tusken_staff/tusken_staff.md3",
    ),
    (
        "WP_SCEPTER",
        "models/weapons2/sith_scepter/sith_scepter.md3",
    ),
    (
        "WP_NOGHRI_STICK",
        "models/weapons2/noghri_stick/noghri_stick.md3",
    ),
];

/// The number of the weapon named `name`, ignoring case ("NULL" is none).
fn weapon_id(name: &str) -> Option<usize> {
    if name.eq_ignore_ascii_case("NULL") {
        return Some(0);
    }
    WEAPONS
        .iter()
        .position(|(w, _)| w.eq_ignore_ascii_case(name))
}

fn weapon(name: &str) -> usize {
    weapon_id(name).expect("a weapon of the table")
}

/// The ranks (`rank_t`) by their names in `.npc` files, lowest first.
const RANKS: [&str; 8] = [
    "civilian",
    "crewman",
    "ensign",
    "ltjg",
    "lt",
    "ltcomm",
    "commander",
    "captain",
];

/// `NPC_WeaponsForTeam` and `NPC_SetWeapons`: the weapon an NPC of `team` and
/// type `npc_type` with spawn flags `flags` gets when its block gives none. Of a
/// pair, the later weapon wins unless it is melee.
fn team_weapon(team: Team, flags: i64, npc_type: &str) -> usize {
    let is = |name: &str| npc_type.eq_ignore_ascii_case(name);
    let starts = |prefix: &str| {
        npc_type
            .get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    };
    let name = match team {
        Team::Enemy => match () {
            _ if is("tavion") || starts("reborn") || is("desann") || starts("shadowtrooper") => {
                "WP_SABER"
            }
            _ if starts("stofficer") => "WP_FLECHETTE",
            _ if is("stcommander") => "WP_REPEATER",
            _ if is("swamptrooper") => "WP_FLECHETTE",
            _ if is("swamptrooper2") => "WP_REPEATER",
            _ if is("rockettrooper") => "WP_ROCKET_LAUNCHER",
            _ if is("imperial") || starts("impworker") || is("stormpilot") => "WP_BLASTER_PISTOL",
            _ if is("galak") => "WP_BLASTER",
            _ if is("galak_mech") => "WP_REPEATER",
            _ if starts("ugnaught") => "WP_NONE",
            _ if is("granshooter") => "WP_BLASTER",
            _ if is("granboxer") => "WP_MELEE",
            // Thermal detonators and melee: the detonators.
            _ if starts("gran") => "WP_THERMAL",
            _ if is("rodian") => "WP_DISRUPTOR",
            _ if is("rodian2") => "WP_BLASTER",
            _ if is("interrogator") || is("sentry") || starts("protocol") => "WP_NONE",
            _ if starts("weequay") => "WP_BOWCASTER",
            _ if is("impofficer") || is("impcommander") => "WP_BLASTER",
            _ if is("probe") || is("seeker") || starts("remote") => "WP_BOT_LASER",
            _ if is("trandoshan") => "WP_REPEATER",
            // The main gun and the side guns: the side guns.
            _ if is("atst") => "WP_ATST_SIDE",
            _ if is("mark1") || is("mark2") => "WP_BOT_LASER",
            _ if is("minemonster") || is("howler") => "WP_MELEE",
            _ => "WP_BLASTER",
        },
        // SFB_RIFLEMAN, SFB_PHASER.
        Team::Player if flags & 2 != 0 => "WP_REPEATER",
        Team::Player if flags & 4 != 0 => "WP_BLASTER_PISTOL",
        Team::Player => match () {
            _ if starts("jedi") || is("luke") => "WP_SABER",
            _ if starts("prisoner") || starts("elder") || is("MonMothma") => "WP_NONE",
            _ if starts("bespincop") => "WP_BLASTER_PISTOL",
            _ => "WP_BLASTER",
        },
        Team::Neutral | Team::Free => "WP_NONE",
    };
    weapon(name)
}

/// What a saber's `.sab` block says about its hilt (`WP_SaberParseParms`).
#[derive(Debug, Clone, PartialEq)]
pub struct SaberLook {
    /// "saberModel": a Ghoul2 model.
    pub model: String,
    /// "customSkin", or `None` for the model's own shaders.
    pub skin: Option<String>,
    /// "twoHanded": no second saber goes with it.
    pub two_handed: bool,
    /// "boltToWrist": bolted to the forearm's cap, not the hand.
    pub to_wrist: bool,
    /// How many of its colours ("saberColor", "saberColor2" to "saberColor7") it
    /// picks at random ("random"): each a draw as the block is read
    /// (`TranslateSaberColor`).
    pub random_colors: usize,
}

/// `WP_SaberParseParms` for saber `name` in the `.sab` files' text `sabers` (all
/// of them, in order: the first block of a name wins), from `WP_SaberSetDefaults`'
/// hilt (saber_reborn's). A saber no block names keeps the defaults.
pub fn saber_look(sabers: &[u8], name: &str) -> SaberLook {
    let mut look = SaberLook {
        model: "models/weapons2/saber_reborn/saber_w.glm".to_owned(),
        skin: None,
        two_handed: false,
        to_wrist: false,
        random_colors: 0,
    };
    let Some(mut p) = block(sabers, name) else {
        return look;
    };
    loop {
        let token = p.token(true);
        if token.is_empty() || token == b"}" {
            return look;
        }
        // Each keyword reads one value on its line, but "notInMP", which skips the
        // line (the original also skips the line of a keyword it does not know).
        let key = String::from_utf8_lossy(token).to_ascii_lowercase();
        if key == "notinmp" {
            p.skip_rest_of_line();
            continue;
        }
        let value = String::from_utf8_lossy(p.token(false)).into_owned();
        match key.as_str() {
            "sabermodel" if !value.is_empty() => look.model = value,
            "customskin" if !value.is_empty() => look.skin = Some(value),
            "twohanded" if !value.is_empty() => look.two_handed = atoi(&value) != 0,
            "bolttowrist" if !value.is_empty() => look.to_wrist = atoi(&value) != 0,
            "sabercolor" | "sabercolor2" | "sabercolor3" | "sabercolor4" | "sabercolor5"
            | "sabercolor6" | "sabercolor7"
                if value.eq_ignore_ascii_case("random") =>
            {
                look.random_colors += 1
            }
            _ => {}
        }
    }
}

/// `G_CreateG2AttachedWeaponModel`'s Ghoul2 model for a weapon model `name`: an
/// MD3's name loses ".md3" and what follows, gains "_w" unless it has one (or is
/// "noweap", which has no Ghoul2 model), and ends in ".glm".
pub fn attached_model(name: &str) -> String {
    let Some(at) = name.find(".md3") else {
        return name.to_owned();
    };
    let base = &name[..at];
    let w = if base.contains("_w") || base.contains("noweap") {
        ""
    } else {
        "_w"
    };
    format!("{base}{w}.glm")
}

/// What an NPC of `look`, type `npc_type`, with spawn flags `flags` holds as it
/// spawns and the animations it then stands in, with `sabers` the `.sab` files'
/// text.
///
/// Its weapon is its block's, or one its team gives (`NPC_Begin`). A saber
/// wielder holds its saber's hilt, a second in the left hand when it has two
/// (`WP_SaberAddG2SaberModels`); others hold their weapon in the right hand, as
/// `NPC_SetMiscDefaultData` gives them by team: none for good Jedi, saber-wielding
/// enemies, flying droids or neutrals, and a reborn officer's blaster pistol twice.
///
/// It stands as its first movement frames leave it with no enemy, its weapon idle
/// (`PM_Footsteps`, `PM_TorsoAnimation`): legs apart (`BOTH_STAND3`) holding a gun,
/// an imperial with none in `BOTH_STAND4`, others in `BOTH_STAND1`; the torso in
/// its weapon's idle animation. A saber's blade is off, so its torso follows its
/// legs.
pub fn npc_gear(
    look: &NpcLook,
    npc_type: &str,
    flags: i64,
    sabers: &[u8],
) -> (Pose, Vec<Attached>) {
    let class = look.class.as_str();
    let weapon_number = match look.weapon {
        0 => team_weapon(look.team, flags, npc_type),
        w => w,
    };
    let held = |name: &str| weapon_number == weapon(name);
    let mut attached = Vec::new();
    // ps.saber[0].type is set once the block names a saber.
    if let (Some(first), true) = (&look.sabers[0], held("WP_SABER")) {
        let first = saber_look(sabers, first);
        let second = look.sabers[1]
            .as_ref()
            .map(|name| saber_look(sabers, name))
            .filter(|s| !first.two_handed && !s.two_handed);
        for (saber, hand) in [(Some(first), "r"), (second, "l")] {
            let Some(saber) = saber else { continue };
            let bolt = match (saber.to_wrist, hand) {
                (true, "r") => "*r_hand_cap_r_arm",
                (true, _) => "*l_hand_cap_l_arm",
                (false, "r") => "*r_hand",
                (false, _) => "*l_hand",
            };
            attached.push(Attached {
                model: saber.model,
                bolt,
                skin: saber.skin,
            });
        }
    }
    let gun = |bolt| Attached {
        model: attached_model(WEAPONS[weapon_number].1),
        bolt,
        skin: None,
    };
    let armed = weapon_number != 0 && !held("WP_SABER");
    match look.team {
        Team::Player
            if !matches!(
                class,
                "class_seeker" | "class_jedi" | "class_kyle" | "class_luke"
            ) =>
        {
            if armed {
                attached.push(gun("*r_hand"));
            }
        }
        Team::Enemy
            if !matches!(
                class,
                "class_tavion"
                    | "class_alora"
                    | "class_desann"
                    | "class_shadowtrooper"
                    | "class_probe"
                    | "class_remote"
                    | "class_interrogator"
                    | "class_sentry"
            ) && !(class == "class_reborn" && held("WP_SABER")) =>
        {
            if armed {
                attached.push(gun("*r_hand"));
                // Dual blaster pistols for a reborn of RANK_LT_COMM or above.
                if held("WP_BLASTER_PISTOL") && class == "class_reborn" && look.rank >= 5 {
                    attached.push(gun("*l_hand"));
                }
            }
        }
        Team::Free if armed && !held("WP_MELEE") => attached.push(gun("*r_hand")),
        _ => {}
    }

    let dual_pistols = attached.len() == 2 && held("WP_BLASTER_PISTOL");
    let (legs, torso) = stance(class, weapon_number, dual_pistols);
    (
        Pose {
            legs,
            torso,
            unarmed: weapon_number == 0,
            still_since: 0,
            script: None,
            bad_bones: bad_bones(class),
            strides: strides(class),
            ..Pose::default()
        },
        attached,
    )
}

/// How fast class `class`'s walks and runs carry it at their own speed
/// (`PM_SetAnimFinal`'s `moveSpeedOfAnim`), `None` for the classes it leaves at
/// their own speed (the howler, wampa, gonk, mouse, probe, protocol droid, R2 and
/// R5 droids and the seeker).
fn strides(class: &str) -> Option<[f32; 2]> {
    match class {
        "class_howler" | "class_wampa" | "class_gonk" | "class_mouse" | "class_probe"
        | "class_protocol" | "class_r2d2" | "class_r5d2" | "class_seeker" => None,
        "class_hazard_trooper" => Some([50.0, 50.0]),
        "class_rancor" => Some([173.0, 173.0]),
        _ => Some([50.0, 150.0]),
    }
}

/// `G_ClassHasBadBones`: whether class `class`'s bones are too odd for cgame to
/// turn its spine against its "Motion" bone.
fn bad_bones(class: &str) -> bool {
    matches!(
        class,
        "class_wampa"
            | "class_rockettrooper"
            | "class_saber_droid"
            | "class_hazard_trooper"
            | "class_assassin_droid"
            | "class_rancor"
    )
}

/// The stance an NPC of class `class` holding weapon `weapon_number` stands in with
/// its weapon idle, `dual_pistols` when it holds a blaster pistol in each hand: its
/// legs' and torso's animations (`PM_Footsteps`, `PM_TorsoAnimation`).
fn stance(class: &str, weapon_number: usize, dual_pistols: bool) -> (&'static str, &'static str) {
    // PM_Footsteps: a gun between the saber and the detonation pack (exclusive).
    let gun_held = (weapon("WP_SABER") + 1..weapon("WP_DET_PACK")).contains(&weapon_number);
    let legs = match class {
        "class_galakmech" => "BOTH_STAND2",
        "class_atst" => "BOTH_STAND1",
        _ if gun_held => "BOTH_STAND3",
        "class_imperial" => "BOTH_STAND4",
        _ => "BOTH_STAND1",
    };
    // PM_TorsoAnimation, weapon idle: the legs' guard idles first, then by weapon.
    let torso = match WEAPONS[weapon_number].0 {
        _ if class == "class_galakmech" => "BOTH_STAND2",
        "WP_SABER" => legs,
        _ if legs == "BOTH_STAND4" => legs,
        "WP_NONE" => "BOTH_STAND1",
        "WP_BLASTER_PISTOL" if dual_pistols => "BOTH_STAND1",
        "WP_BRYAR_PISTOL" | "WP_BLASTER_PISTOL" | "WP_BOT_LASER" => "TORSO_WEAPONIDLE2",
        "WP_MELEE" if class == "class_rancor" => "BOTH_STAND1",
        "WP_MELEE"
            if matches!(
                class,
                "class_probe"
                    | "class_seeker"
                    | "class_interrogator"
                    | "class_sentry"
                    | "class_remote"
            ) =>
        {
            "BOTH_STAND1"
        }
        "WP_MELEE" => "BOTH_STAND6",
        "WP_TUSKEN_STAFF" => "BOTH_STAND3",
        "WP_DISRUPTOR" | "WP_TUSKEN_RIFLE" => "TORSO_WEAPONREADY3",
        "WP_THERMAL" | "WP_TRIP_MINE" | "WP_DET_PACK" => "BOTH_STAND1",
        _ => "TORSO_WEAPONIDLE3",
    };
    (legs, torso)
}

/// The models of the NPCs `entities` spawn under `rules`, with `parms` the `.npc`
/// files' text and `sabers` the `.sab` files', in spawn order: each at its
/// spawner's origin, turned to its yaw (`CG_G2PlayerAngles` keeps a standing NPC
/// upright), standing as its first movement frames leave it with what it holds
/// ([`npc_gear`]), and linked with its box. A scaled NPC is drawn 24 units higher
/// per unit of height scale above 1 (`CG_Player`), its axes scaled
/// (`ScaleModelAxis`). An NPC whose block is missing or gives no Ghoul2 model is
/// left out (the original logs an error for a missing one).
///
/// Each stands where its first movement frames put it ([`settle`]) in `world`.
///
/// `scripts` reads a spawnscript by name (`scripts/<name>.IBI`). Its first pass
/// ([`first_pass`]) may put the NPC's weapon away (nothing bolted to it, and the
/// unarmed stance) or make it invisible (left out); the NPC keeps the script, which
/// [`crate::script::timeline`] runs.
pub fn npc_models(
    entities: &[Entity],
    rules: Rules,
    parms: &[u8],
    sabers: &[u8],
    world: &Collision,
    scripts: &dyn Fn(&str) -> Option<Vec<Node>>,
) -> Vec<StaticModel> {
    // The spawn functions draw as the map spawns, the ammo racks as they place
    // their goods (1100 ms), the saber items as they finish spawning (1300 ms), then each NPC's block is read in turn as it spawns
    // (NPC_Spawn_Go, the frame after 1350 ms), drawn here or not.
    let mut rand = Rand::default();
    let spawns = npc_spawns(entities, rules, sabers, &mut rand);
    ammo_rack_draws(entities, rules, &mut rand);
    item_saber_draws(entities, rules, sabers, &mut rand);
    let looks: Vec<_> = spawns
        .iter()
        .map(|n| npc_look(parms, sabers, &n.npc_type, n.message, &mut rand))
        .collect();
    spawns
        .into_iter()
        .zip(looks)
        .filter_map(|(n, look)| {
            let look = look?;
            let (mut pose, mut attached) = npc_gear(&look, &n.npc_type, n.flags, sabers);
            let script = n.spawnscript.as_deref().and_then(scripts).map(Arc::new);
            let first = script
                .as_deref()
                .map_or_else(FirstPass::default, |s| first_pass(s));
            if first.invisible {
                return None;
            }
            if first.unarmed {
                (pose.legs, pose.torso) = stance(&look.class, 0, false);
                pose.unarmed = true;
                attached.clear();
            }
            let folder = format!("models/players/{}/", look.model);
            let skin = if look.skin.contains('|') {
                format!("{folder}|{}", look.skin)
            } else {
                format!("{folder}model_{}.skin", look.skin)
            };
            let scaled = look.scale.map(|s| s != 0.0 && s != 1.0);
            let (at, moving) = settle(world, n.origin, look.mins, look.maxs, n.flags);
            let pose = Pose {
                still_since: if moving { FIRST_FRAME } else { 0 },
                script,
                name: n.targetname,
                yaw: n.angles[1],
                yaw_speed: yaw_speed(&look, &n.npc_type),
                origin: at,
                mins: look.mins,
                maxs: look.maxs,
                walk_speed: look.walk_speed,
                run_speed: look.run_speed,
                not_solid: n.flags & 64 != 0,
                number: n.number,
                eyes: [
                    at[0],
                    at[1],
                    at[2] + look.maxs[2] + STANDARD_VIEWHEIGHT_OFFSET,
                ],
                ..pose
            };
            let mut origin = at;
            if scaled[2] {
                origin[2] += 24.0 * (look.scale[2] - 1.0);
            }
            let axis = angles_to_axis([0.0, n.angles[1], 0.0]);
            Some(StaticModel {
                model: format!("{folder}model.glm"),
                origin,
                axes: [0, 1, 2]
                    .map(|i| axis[i].map(|x| if scaled[i] { x * look.scale[i] } else { x })),
                scale: look.scale,
                normalized: !scaled.contains(&true),
                zoffset: 0.0,
                entity_box: Some([
                    [0, 1, 2].map(|i| at[i] + look.mins[i] - 1.0),
                    [0, 1, 2].map(|i| at[i] + look.maxs[i] + 1.0),
                ]),
                ghoul2: Some(Ghoul2 {
                    skin: Some(skin),
                    switches: look.switches,
                    radius: look.radius,
                    pose: Some(pose),
                    attached,
                    color: look.color,
                }),
                mover: None,
            })
        })
        .collect()
}

/// `STANDARD_VIEWHEIGHT_OFFSET`: an NPC's eyes stand this far below its box's top.
const STANDARD_VIEWHEIGHT_OFFSET: f32 = -4.0;

/// How fast an NPC of type `npc_type` turns (`stats.yawSpeed`) at the easiest skill
/// (`g_spskill` 0, the cvar's default, as the captures play): its block's, three
/// quarters of it for stormtroopers, swamp troopers, imperial workers and "rodian2"
/// (`NPC_Spawn_Go`).
fn yaw_speed(look: &NpcLook, npc_type: &str) -> f32 {
    let slower = matches!(
        look.class.as_str(),
        "class_stormtrooper" | "class_swamptrooper" | "class_impworker"
    ) || npc_type.eq_ignore_ascii_case("rodian2");
    if slower {
        look.yaw_speed * 0.75
    } else {
        look.yaw_speed
    }
}

/// Where an NPC spawned at `origin` with the box `mins`..`maxs` comes to rest: lifted
/// out of a floor its box's lower half sinks into or touches (`G_CheckInSolid` with
/// fix, at spawn: a spawner standing on the floor rises by the trace's epsilon),
/// unless its spawner's `flags` have STARTINSOLID (128), then fallen onto the floor
/// below, as the first movement frames drop it
/// (`Pmove` falls until `PM_GroundTrace` finds ground; with no sideways speed the
/// fall ends where a box trace straight down does). An NPC stuck in solid, or with
/// no floor below, stays where it is.
///
/// Also whether it is still moving on its first frame that checks for idling
/// (1550 ms): it fell (its floor lay further below than `PM_GroundTrace` looks, a
/// quarter unit) onto a slope, where landing clips its fall into the slope's plane
/// and leaves it sliding sideways for a frame. Falling onto flat ground or lifted
/// out of a slope, it is already still (measured on t2_dpred and kor2).
pub fn settle(world: &Collision, origin: Vec3, mins: Vec3, maxs: Vec3, flags: i64) -> (Vec3, bool) {
    let mut at = origin;
    if flags & 128 == 0 {
        let feet = [origin[0], origin[1], origin[2] + mins[2]];
        let tr = world.trace(origin, feet, [mins[0], mins[1], 0.0], maxs, MASK_NPCSOLID);
        if tr.start_solid || tr.all_solid {
            return (origin, false);
        }
        if tr.fraction < 1.0 {
            at = [tr.end[0], tr.end[1], tr.end[2] - mins[2]];
        }
    }
    let tr = world.trace(
        at,
        [at[0], at[1], MIN_WORLD_COORD],
        mins,
        maxs,
        MASK_NPCSOLID,
    );
    if tr.all_solid || tr.start_solid || tr.fraction == 1.0 {
        return (at, false);
    }
    (tr.end, at[2] - tr.end[2] > 0.25 && tr.normal[2] < 1.0)
}
