use swjkja_bsp::{Entity, Model};
use swjkja_world::collision::{CONTENTS_SOLID, Collision};
use swjkja_world::weather::{
    CONTENTS_INSIDE, CONTENTS_OUTSIDE, Command, MixedBrushes, Outside, commands,
};

fn entity(pairs: &[(&str, &str)]) -> Entity {
    Entity {
        pairs: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

fn model(mins: [f32; 3], maxs: [f32; 3]) -> Model {
    Model {
        mins,
        maxs,
        first_surface: 0,
        num_surfaces: 0,
        first_brush: 0,
        num_brushes: 0,
    }
}

fn rain(flags: &str) -> Entity {
    entity(&[("classname", "fx_rain"), ("spawnflags", flags)])
}

#[test]
fn rain_asks_for_its_kind_by_its_first_flag() {
    let kinds = |flags| commands(&[rain(flags)], &[], 0);
    // No flag: light rain with a wind changing at random.
    assert_eq!(kinds("0"), ["lightrain", "wind"]);
    assert_eq!(kinds("1"), ["lightrain"]);
    assert_eq!(kinds("2"), ["rain"]);
    assert_eq!(kinds("3"), ["lightrain"]);
    // Heavy rain brings its own fog.
    assert_eq!(kinds("4"), ["heavyrain", "heavyrainfog"]);
    assert_eq!(kinds("8"), ["acidrain"]);
    assert_eq!(kinds("50"), ["rain", "outsideShake", "fog"]);
}

#[test]
fn a_string_is_added_once_in_the_order_first_added() {
    let cmds = commands(
        &[
            entity(&[("classname", "worldspawn")]),
            rain("2"),
            entity(&[("classname", "fx_snow")]),
            rain("2"),
            entity(&[("classname", "fx_wind"), ("spawnflags", "33")]),
        ],
        &[],
        0,
    );
    assert_eq!(cmds, ["rain", "snow", "fog", "wind"]);
}

#[test]
fn rain_left_out_at_the_skill_spawns_nothing() {
    // Spawnflag 256 leaves an entity out at the easiest skill.
    assert!(commands(&[rain("258")], &[], 0).is_empty());
    assert_eq!(commands(&[rain("258")], &[], 1), ["rain"]);
}

#[test]
fn a_weather_zone_is_its_brush_models_bounds_spread_by_a_unit() {
    let models = [
        model([-4096.0; 3], [4096.0; 3]),
        model([-100.5, 0.0, 16.0], [200.0, 300.0, 400.25]),
    ];
    let cmds = commands(
        &[entity(&[
            ("classname", "misc_weather_zone"),
            ("model", "*1"),
        ])],
        &models,
        0,
    );
    assert_eq!(
        cmds,
        ["zone ( -101.500000 -1.000000 15.000000 ) ( 201.000000 301.000000 401.250000 )"]
    );
    assert_eq!(
        Command::parse(&cmds[0]),
        Some(Command::Zone {
            mins: [-101.5, -1.0, 15.0],
            maxs: [201.0, 301.0, 401.25]
        })
    );
}

#[test]
fn a_constant_wind_blows_along_its_angles_at_its_speed() {
    let cmds = commands(
        &[entity(&[
            ("classname", "fx_wind"),
            ("spawnflags", "2"),
            ("angle", "90"),
            ("speed", "250"),
        ])],
        &[],
        0,
    );
    let Some(Command::ConstantWind(v)) = Command::parse(&cmds[0]) else {
        panic!("{cmds:?}");
    };
    assert!(
        v[0].abs() < 1e-3 && (v[1] - 250.0).abs() < 1e-3 && v[2] == 0.0,
        "{v:?}"
    );
}

#[test]
fn commands_read_as_the_renderer_reads_them() {
    assert_eq!(Command::parse("LightRain"), Some(Command::LightRain));
    assert_eq!(Command::parse("heavyrain"), Some(Command::HeavyRain));
    assert_eq!(
        Command::parse("heavyrainfog"),
        Some(Command::Other("heavyrainfog".into()))
    );
    assert_eq!(Command::parse(""), None);
    // A zone missing a parenthesis is ignored; a wind without its vector blows
    // 800 units a second along y.
    assert_eq!(Command::parse("zone ( 0 0 0 ) 1 2 3"), None);
    assert_eq!(
        Command::parse("constantwind"),
        Some(Command::ConstantWind([0.0, 800.0, 0.0]))
    );
}

/// A world 512 units each way with a brush of `contents` over x > 0.
fn courtyard(contents: i32) -> (Collision, ([f32; 3], [f32; 3])) {
    let mut world = Collision::default();
    world.add_box([0.0, -512.0, -512.0], [512.0, 512.0, 512.0], contents);
    world.add_box(
        [-512.0, -512.0, -576.0],
        [512.0, 512.0, -512.0],
        CONTENTS_SOLID,
    );
    (world, ([-512.0; 3], [512.0; 3]))
}

#[test]
fn outside_brushes_mark_where_particles_fall() {
    let (world, bounds) = courtyard(CONTENTS_OUTSIDE);
    let outside = Outside::scan(&world, &[], bounds).unwrap();
    assert!(outside.point([100.0, 0.0, 0.0]));
    assert!(!outside.point([-100.0, 0.0, 0.0]));
    // A cell is outside by its centre: x 0-32's centre is in the brush.
    assert!(outside.particle([1.0, 0.0, 0.0], 1.2, 80.0));
    assert!(!outside.particle([-1.0, 0.0, 0.0], 1.2, 80.0));
    // Beyond every zone, unmarked: inside, as the scan marked the outside.
    assert!(!outside.point([600.0, 0.0, 0.0]));
}

#[test]
fn inside_brushes_mark_where_they_do_not() {
    let (world, bounds) = courtyard(CONTENTS_INSIDE);
    let outside = Outside::scan(&world, &[], bounds).unwrap();
    assert!(!outside.point([100.0, 0.0, 0.0]));
    assert!(outside.point([-100.0, 0.0, 0.0]));
    assert!(outside.point([600.0, 0.0, 0.0]));
}

#[test]
fn a_map_without_either_is_all_outside() {
    let (world, bounds) = courtyard(CONTENTS_SOLID);
    let outside = Outside::scan(&world, &[], bounds).unwrap();
    assert!(outside.point([100.0, 0.0, 0.0]) && outside.point([-100.0, 0.0, 0.0]));
}

#[test]
fn both_kinds_stop_the_load() {
    let (mut world, bounds) = courtyard(CONTENTS_OUTSIDE);
    world.add_box([-512.0; 3], [-256.0; 3], CONTENTS_INSIDE);
    assert_eq!(Outside::scan(&world, &[], bounds), Err(MixedBrushes));
}

#[test]
fn zones_limit_the_scan() {
    let (world, bounds) = courtyard(CONTENTS_OUTSIDE);
    // One zone over x 256-512 only: points beyond it count as inside.
    let outside = Outside::scan(&world, &[([256.0, -512.0, -512.0], [512.0; 3])], bounds).unwrap();
    assert!(outside.point([300.0, 0.0, 0.0]));
    assert!(!outside.point([100.0, 0.0, 0.0]));
}
