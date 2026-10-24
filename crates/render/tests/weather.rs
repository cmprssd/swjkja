//! Rain's particles, without the GPU or game data.

use swjkja_render::weather::{Weather, WeatherVertex, WeatherView};
use swjkja_world::collision::Collision;
use swjkja_world::weather::{CONTENTS_OUTSIDE, Command, Outside};

const BOUNDS: ([f32; 3], [f32; 3]) = ([-4096.0; 3], [4096.0; 3]);

/// A map without outside or inside brushes: all of it outside.
fn open_sky() -> Outside {
    Outside::scan(|_| 0, &[], BOUNDS).unwrap()
}

/// A map whose only outside brush is far from the view: around it, inside.
fn indoors() -> Outside {
    let mut world = Collision::default();
    world.add_box([3000.0; 3], [3100.0; 3], CONTENTS_OUTSIDE);
    Outside::scan(|p| world.point_contents(p), &[], BOUNDS).unwrap()
}

/// Looking along +x from the origin (forward, left, up).
const VIEW: WeatherView = WeatherView {
    origin: [0.0; 3],
    axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
};

/// The triangles drawn at each of `times` (ms), the last frame's.
fn frames(weather: &mut Weather, times: &[i32]) -> Vec<WeatherVertex> {
    let mut out = Vec::new();
    for &ms in times {
        out.clear();
        weather.frame(ms, &VIEW, &mut out);
    }
    out
}

fn length(v: [f32; 3]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

#[test]
fn only_rain_makes_a_cloud() {
    assert!(Weather::new(&[], open_sky()).is_none());
    assert!(Weather::new(&[Command::Wind, Command::Other("snow".into())], open_sky()).is_none());
    assert!(Weather::new(&[Command::Rain], open_sky()).is_some());
}

#[test]
fn the_first_frame_draws_nothing() {
    let mut rain = Weather::new(&[Command::Rain], open_sky()).unwrap();
    assert!(frames(&mut rain, &[16]).is_empty());
    assert!(!frames(&mut rain, &[32]).is_empty());
}

#[test]
fn rain_in_front_of_the_view_fades_in_at_once_to_half() {
    let mut rain = Weather::new(&[Command::Rain], open_sky()).unwrap();
    let tris = frames(&mut rain, &[16, 32]);
    assert_eq!(tris.len() % 3, 0);
    let drawn = tris.len() / 3;
    // A thousand particles spread over the range around the view: those ahead of
    // it are drawn, about half.
    assert!((350..650).contains(&drawn), "{drawn}");
    // Fading 100 a second over 16 ms passes the cloud's alpha of 0.5 at once; the
    // colour is 0.5 times that on every channel (added to the frame).
    assert!(tris.iter().all(|v| v.color == [0.25; 4]));
    assert!(tris.iter().all(|v| v.xyz[0] > -1.3), "ahead of the view");
}

#[test]
fn a_drop_is_a_streak_back_up_its_path_and_a_hair_wide() {
    let mut rain = Weather::new(&[Command::Rain], open_sky()).unwrap();
    let tris = frames(&mut rain, &[16, 32, 48]);
    for t in tris.chunks(3) {
        let width = [0, 1, 2].map(|k| t[1].xyz[k] - t[0].xyz[k]);
        assert!(width[0] == 0.0 && (width[1] - 1.2).abs() < 1e-4 && width[2] == 0.0);
        let streak = [0, 1, 2].map(|k| t[2].xyz[k] - t[1].xyz[k]);
        assert!((length(streak) - 80.0).abs() < 1e-3, "{streak:?}");
        // Without wind it falls straight down: the streak points up.
        assert!(streak[2] > 79.9, "{streak:?}");
        assert_eq!(
            [t[0].st, t[1].st, t[2].st],
            [[1.0, 0.0], [0.0, 1.0], [0.0, 0.0]]
        );
    }
}

#[test]
fn light_rain_has_half_the_drops() {
    let drawn = |c: Command| {
        let mut rain = Weather::new(&[c], open_sky()).unwrap();
        frames(&mut rain, &[16, 32]).len() / 3
    };
    let (light, full) = (drawn(Command::LightRain), drawn(Command::Rain));
    assert!(light * 3 < full * 2 && light * 3 > full, "{light} {full}");
}

#[test]
fn acid_rain_is_green_and_wider() {
    let mut rain = Weather::new(&[Command::AcidRain], open_sky()).unwrap();
    let tris = frames(&mut rain, &[16, 32]);
    let a = 0.7f32;
    assert!(
        tris.iter()
            .all(|v| v.color == [0.34 * a, 0.70 * a, 0.34 * a, 0.70 * a])
    );
    assert_eq!(tris[1].xyz[1] - tris[0].xyz[1], 2.0);
}

#[test]
fn no_rain_falls_indoors() {
    let mut rain = Weather::new(&[Command::Rain], indoors()).unwrap();
    assert!(frames(&mut rain, &[16, 32, 48, 64]).is_empty());
}

#[test]
fn a_constant_wind_slants_the_streaks() {
    let mut rain = Weather::new(
        &[Command::ConstantWind([800.0, 0.0, 0.0]), Command::Rain],
        open_sky(),
    )
    .unwrap();
    let tris = frames(&mut rain, &(1..=20).map(|f| f * 16).collect::<Vec<_>>());
    assert!(!tris.is_empty());
    // Blown along +x, a streak trails back along -x.
    for t in tris.chunks(3) {
        let streak = [0, 1, 2].map(|k| t[2].xyz[k] - t[1].xyz[k]);
        assert!(streak[0] < -1.0, "{streak:?}");
    }
}

#[test]
fn drops_fall_at_their_terminal_speed() {
    // Each frame keeps 0.7 of the velocity plus the force over the mass (5 to 10):
    // 2000 down settles at 2000 * 0.7 / 0.3 / mass, 467 to 933 units a second, so
    // a drop drawn in two frames 16 ms apart moved 7.5 to 15 units, less if it
    // came back to the spawn plane lately (from rest it nears that speed within
    // ten frames).
    let mut rain = Weather::new(&[Command::Rain], open_sky()).unwrap();
    let times: Vec<i32> = (1..=60).map(|f| f * 16).collect();
    let before = frames(&mut rain, &times);
    let mut after = Vec::new();
    rain.frame(61 * 16, &VIEW, &mut after);
    // Pair drops by their streak's x and y (they fall straight down).
    let (mut matched, mut settled) = (0, 0);
    for t in before.chunks(3) {
        if let Some(u) = after
            .chunks(3)
            .find(|u| u[0].xyz[0] == t[0].xyz[0] && u[0].xyz[1] == t[0].xyz[1])
        {
            let fell = t[0].xyz[2] - u[0].xyz[2];
            assert!(fell > 0.0 && fell < 15.1, "{fell}");
            matched += 1;
            settled += usize::from(fell > 7.4);
        }
    }
    assert!(
        matched > 100 && settled * 10 > matched * 8,
        "{settled} of {matched}"
    );
}
