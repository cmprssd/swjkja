//! Rain (`tr_WorldEffects.cpp`): each rain command makes a cloud of particles that
//! falls around the view, pushed by gravity and the winds, drawn only where the map
//! counts as outside (`swjkja_world::weather::Outside`) and fading in and out as
//! particles enter and leave the view (`CParticleCloud`, `CWindZone`).
//!
//! The original draws its numbers from the renderer module's own `Q_flrand` and
//! `Q_irand` (here a [`Rand`] of its own) and the C library's `rand()` (the winds'
//! chance of a lull), and moves particles by each frame's real time: where a
//! capture's streaks fall cannot be reproduced, so comparisons leave world effects
//! out on both sides (DECISIONS.md, 2026-10-24).
//!
//! Left out: snow, space dust, fog clouds and puffs (the other particle clouds), the
//! clouds' rotation (snow's), in-game cinematics (the original draws no world
//! effects during one), and the rain image's nearest filtering (drawn here with the
//! clamping sampler's linear filter).

use swjkja_world::Vec3;
use swjkja_world::rand::Rand;
use swjkja_world::weather::{Command, Outside};

/// `MAX_PARTICLE_CLOUDS`.
pub const MAX_CLOUDS: usize = 5;
/// `MAX_WIND_ZONES`.
pub const MAX_WINDS: usize = 12;
/// The rain's image.
pub const RAIN_IMAGE: &str = "gfx/world/rain.jpg";

/// The C library's `rand()` as the reference build's (MSVC), never seeded.
#[derive(Debug, Clone)]
struct Crt(u32);

impl Crt {
    /// `FloatRand`: `rand() / RAND_MAX`.
    fn float(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        ((self.0 >> 16) & 0x7fff) as f32 / 32767.0
    }
}

/// `SVecRange::Pick`: x, then y, then z.
fn pick(rand: &mut Rand, mins: Vec3, maxs: Vec3) -> Vec3 {
    let x = rand.flrand(mins[0], maxs[0]);
    let y = rand.flrand(mins[1], maxs[1]);
    let z = rand.flrand(mins[2], maxs[2]);
    [x, y, z]
}

/// `SVecRange::In`: strictly inside.
fn inside(p: Vec3, mins: Vec3, maxs: Vec3) -> bool {
    (0..3).all(|i| p[i] > mins[i] && p[i] < maxs[i])
}

/// A wind (`CWindZone`): global, or inside its box.
#[derive(Debug, Clone)]
struct Wind {
    global: bool,
    bounds: [Vec3; 2],
    velocity_range: [Vec3; 2],
    duration: [i32; 2],
    dead_time: [i32; 2],
    max_delta: f32,
    chance_of_dead_time: f32,
    current: Vec3,
    target: Vec3,
    /// Updates left until the next target; -1 for a constant wind.
    remaining: i32,
}

impl Wind {
    /// `CWindZone::Initialize`.
    fn new() -> Self {
        Self {
            global: true,
            bounds: [[0.0; 3]; 2],
            velocity_range: [[-1500.0, -1500.0, -10.0], [1500.0, 1500.0, 10.0]],
            duration: [1000, 2000],
            dead_time: [1000, 3000],
            max_delta: 10.0,
            chance_of_dead_time: 0.3,
            current: [0.0; 3],
            target: [0.0; 3],
            remaining: 0,
        }
    }

    /// `CWindZone::Update`, once a frame: a new target (or a lull) when the last
    /// one's updates run out, else up to `max_delta` closer to it.
    fn update(&mut self, rand: &mut Rand, crt: &mut Crt) {
        if self.remaining == 0 {
            if crt.float() < self.chance_of_dead_time {
                self.remaining = rand.irand(self.dead_time[0], self.dead_time[1]);
                self.target = [0.0; 3];
            } else {
                self.remaining = rand.irand(self.duration[0], self.duration[1]);
                self.target = pick(rand, self.velocity_range[0], self.velocity_range[1]);
            }
        } else if self.remaining != -1 {
            self.remaining -= 1;
            let mut d = sub(self.target, self.current);
            let len = normalize(&mut d).min(self.max_delta);
            self.current = add(self.current, scale(d, len));
        }
    }
}

/// One particle (`WFXParticle`).
#[derive(Debug, Clone, Default)]
struct Particle {
    alpha: f32,
    render: bool,
    fade_in: bool,
    fade_out: bool,
    position: Vec3,
    velocity: Vec3,
    mass: f32,
}

/// A particle cloud (`CParticleCloud`) as a rain command sets it up: triangles
/// stretched along their velocity, added to the frame.
#[derive(Debug, Clone)]
struct Cloud {
    particles: Vec<Particle>,
    populated: bool,
    width: f32,
    height: f32,
    gravity: f32,
    fade: f32,
    color: [f32; 4],
    spawn_plane_distance: f32,
    spawn_plane_size: f32,
    /// Around the view (`mSpawnRange`).
    spawn_range: [Vec3; 2],
    friction_inverse: f32,
}

impl Cloud {
    /// `Initialize` after `Reset`, with the rain commands' settings: each
    /// particle's mass drawn in turn.
    fn rain(
        rand: &mut Rand,
        count: usize,
        width: f32,
        gravity: f32,
        fade: f32,
        color: [f32; 4],
    ) -> Self {
        let particles = (0..count)
            .map(|_| Particle {
                mass: rand.flrand(5.0, 10.0),
                ..Default::default()
            })
            .collect();
        Self {
            particles,
            populated: false,
            width,
            height: 80.0,
            gravity,
            fade,
            color,
            spawn_plane_distance: 500.0,
            spawn_plane_size: 500.0,
            spawn_range: [[-625.0; 3], [625.0; 3]],
            friction_inverse: 0.7,
        }
    }
}

/// The view a frame's weather moves around: its origin and axes (forward, left,
/// up: `backEnd.viewParms.ori`).
#[derive(Debug, Clone, Copy)]
pub struct WeatherView {
    pub origin: Vec3,
    pub axes: [Vec3; 3],
}

/// A triangle corner of a drawn particle: where, its texture coordinate, and its
/// colour (the cloud's colour times the particle's alpha, every channel).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherVertex {
    pub xyz: Vec3,
    pub st: [f32; 2],
    pub color: [f32; 4],
}

/// A map's rain and winds, as its world effect commands set them up.
#[derive(Debug, Clone)]
pub struct Weather {
    outside: Outside,
    clouds: Vec<Cloud>,
    winds: Vec<Wind>,
    rand: Rand,
    crt: Crt,
    /// The first frame only caches the outside (`COutside::Cache`).
    cached: bool,
    /// The last frame's time in ms (`RE_RenderScene`'s `lastTime`).
    last_ms: i32,
}

impl Weather {
    /// The weather `commands` set up, in order, over the scanned `outside`. `None`
    /// when they make no rain (nothing is drawn).
    pub fn new(commands: &[Command], outside: Outside) -> Option<Self> {
        let mut rand = Rand::default();
        let mut clouds = Vec::new();
        let mut winds = Vec::new();
        for c in commands {
            let wind = match c {
                Command::Wind => Some(Wind::new()),
                Command::ConstantWind(v) => Some(Wind {
                    current: *v,
                    remaining: -1,
                    ..Wind::new()
                }),
                Command::GustingWind => Some(Wind {
                    velocity_range: [[-3000.0, -3000.0, -100.0], [3000.0, 3000.0, 100.0]],
                    duration: [1000, 3000],
                    chance_of_dead_time: 0.5,
                    dead_time: [2000, 4000],
                    ..Wind::new()
                }),
                Command::WindZone {
                    mins,
                    maxs,
                    velocity,
                } => Some(Wind {
                    global: false,
                    bounds: [*mins, *maxs],
                    current: *velocity,
                    remaining: -1,
                    ..Wind::new()
                }),
                _ => None,
            };
            if let Some(w) = wind {
                if winds.len() < MAX_WINDS {
                    winds.push(w);
                }
                continue;
            }
            let (count, width, gravity, fade, color) = match c {
                Command::LightRain => (500, 1.2, 2000.0, 100.0, [0.5; 4]),
                Command::Rain => (1000, 1.2, 2000.0, 100.0, [0.5; 4]),
                Command::AcidRain => (1000, 2.0, 2000.0, 100.0, [0.34, 0.70, 0.34, 0.70]),
                Command::HeavyRain => (1000, 1.2, 2800.0, 15.0, [0.5; 4]),
                _ => continue,
            };
            if clouds.len() < MAX_CLOUDS {
                clouds.push(Cloud::rain(&mut rand, count, width, gravity, fade, color));
            }
        }
        (!clouds.is_empty()).then_some(Self {
            outside,
            clouds,
            winds,
            rand,
            crt: Crt(1),
            cached: false,
            last_ms: 0,
        })
    }

    /// How many particles the clouds hold: the most triangles a frame draws.
    pub fn capacity(&self) -> usize {
        self.clouds.iter().map(|c| c.particles.len()).sum()
    }

    /// Moves the weather on to game time `ms` seen from `view`
    /// (`RB_RenderWorldEffects`), and appends each drawn particle's triangle to
    /// `out`, cloud by cloud. The frame's time is the time since the last frame,
    /// from 1 to 1000 ms; the first frame draws nothing.
    pub fn frame(&mut self, ms: i32, view: &WeatherView, out: &mut Vec<WeatherVertex>) {
        let elapsed_ms = ((ms - self.last_ms) as f32).clamp(1.0, 1000.0);
        self.last_ms = ms;
        if !self.cached {
            self.cached = true;
            return;
        }
        let seconds = elapsed_ms / 1000.0;
        let mut global_wind = [0.0; 3];
        for w in &mut self.winds {
            w.update(&mut self.rand, &mut self.crt);
            if w.global {
                global_wind = add(global_wind, w.current);
            }
        }
        let local: Vec<&Wind> = self.winds.iter().filter(|w| !w.global).collect();
        for cloud in &mut self.clouds {
            update(
                cloud,
                &self.outside,
                &local,
                &mut self.rand,
                global_wind,
                seconds,
                view,
            );
            render(cloud, view, out);
        }
    }
}

/// `CParticleCloud::Update` for a cloud without rotation, falling (gravity not 0).
fn update(
    cloud: &mut Cloud,
    outside: &Outside,
    local_winds: &[&Wind],
    rand: &mut Rand,
    global_wind: Vec3,
    seconds: f32,
    view: &WeatherView,
) {
    let camera = view.origin;
    let forward = view.axes[0];
    let fade = cloud.fade * seconds;
    let force = add([0.0, 0.0, -cloud.gravity], global_wind);
    let mut mins = add(camera, cloud.spawn_range[0]);
    let mut maxs = add(camera, cloud.spawn_range[1]);
    // The range grows on the side the force comes from, for the spawn plane's tilt.
    for d in 0..3 {
        if force[d] > 0.01 {
            mins[d] -= cloud.spawn_plane_distance / 2.0;
        } else if force[d] < -0.01 {
            maxs[d] += cloud.spawn_plane_distance / 2.0;
        }
    }
    let mut plane_norm = force;
    normalize(&mut plane_norm);
    let (plane_right, plane_up) = make_normal_vectors(plane_norm);

    for part in &mut cloud.particles {
        if !cloud.populated {
            part.position = pick(rand, mins, maxs);
        }
        let mut part_force = force;
        for w in local_winds {
            if inside(part.position, w.bounds[0], w.bounds[1]) {
                part_force = add(part_force, w.current);
            }
        }
        let part_force = scale(part_force, 1.0 / part.mass);
        part.velocity = scale(add(part.velocity, part_force), cloud.friction_inverse);
        part.position = add(part.position, scale(part.velocity, seconds));

        let to_camera = sub(part.position, camera);
        let mut rendering = part.render;
        let part_outside = outside.particle(part.position, cloud.width, cloud.height);
        let in_range = inside(part.position, mins, maxs);
        let in_view = part_outside && in_range && dot(to_camera, forward) > 0.0;

        // Out of range and not drawn: back to the spawn plane, the force's length
        // behind the view.
        if !in_range && !rendering {
            part.velocity = [0.0; 3];
            let mut p = sub(camera, scale(plane_norm, cloud.spawn_plane_distance));
            let r = rand.flrand(-cloud.spawn_plane_size, cloud.spawn_plane_size);
            p = add(p, scale(plane_right, r));
            let u = rand.flrand(-cloud.spawn_plane_size, cloud.spawn_plane_size);
            p = add(p, scale(plane_up, u));
            part.position = p;
        }

        if rendering && !in_view {
            part.fade_in = false;
            part.fade_out = true;
        } else if rendering && in_view && part.fade_out {
            part.fade_in = true;
            part.fade_out = false;
        } else if !rendering && in_view {
            rendering = true;
            part.alpha = 0.0;
            part.render = true;
            part.fade_in = true;
            part.fade_out = false;
        }
        if rendering {
            if part.fade_out {
                part.alpha -= fade;
                if part.alpha <= 0.0 {
                    part.alpha = 0.0;
                    part.fade_out = false;
                    part.fade_in = false;
                    part.render = false;
                }
            } else if part.fade_in {
                part.alpha += fade;
                if part.alpha >= cloud.color[3] {
                    part.fade_in = false;
                    part.alpha = cloud.color[3];
                }
            }
        }
    }
    cloud.populated = true;
}

/// `CParticleCloud::Render` for triangles oriented along their velocity, added
/// (blend mode 1): from the particle, along the view's left by the width, and back
/// up its path by the height.
fn render(cloud: &Cloud, view: &WeatherView, out: &mut Vec<WeatherVertex>) {
    let left = scale(view.axes[1], cloud.width);
    for part in cloud.particles.iter().filter(|p| p.render) {
        let mut dir = part.velocity;
        normalize(&mut dir);
        let down = scale(dir, -cloud.height);
        let left_plus_down = add(down, left);
        let color = cloud.color.map(|c| c * part.alpha);
        let p = part.position;
        out.extend([
            WeatherVertex {
                xyz: p,
                st: [1.0, 0.0],
                color,
            },
            WeatherVertex {
                xyz: add(p, left),
                st: [0.0, 1.0],
                color,
            },
            WeatherVertex {
                xyz: add(p, left_plus_down),
                st: [0.0, 0.0],
                color,
            },
        ]);
    }
}

/// `MakeNormalVectors`: two unit vectors square to `forward` and each other.
fn make_normal_vectors(forward: Vec3) -> (Vec3, Vec3) {
    let mut right = [forward[2], -forward[0], forward[1]];
    let d = dot(right, forward);
    right = sub(right, scale(forward, d));
    normalize(&mut right);
    let up = cross(right, forward);
    (right, up)
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Vec3, s: f32) -> Vec3 {
    a.map(|x| x * s)
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `VectorNormalize`: scales `v` to unit length (a zero vector stays zero) and
/// returns its length.
fn normalize(v: &mut Vec3) -> f32 {
    let len = dot(*v, *v).sqrt();
    if len != 0.0 {
        *v = scale(*v, 1.0 / len);
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_c_librarys_first_numbers() {
        // MSVC's rand() from seed 1: 41, 18467, 6334.
        let mut crt = Crt(1);
        assert_eq!(crt.float(), 41.0 / 32767.0);
        assert_eq!(crt.float(), 18467.0 / 32767.0);
        assert_eq!(crt.float(), 6334.0 / 32767.0);
    }

    #[test]
    fn normal_vectors_are_square() {
        let (r, u) = make_normal_vectors([0.0, 0.0, -1.0]);
        assert_eq!(r, [-1.0, 0.0, 0.0]);
        assert_eq!(u, [0.0, -1.0, 0.0]);
    }
}
