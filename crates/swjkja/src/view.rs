//! The map viewer: a window showing a map, flown through with the mouse and
//! keyboard (see [`crate::fly`]), or replaying a recorded camera path.
//!
//! Controls: mouse to look, W A S D to move, Space and C (or Ctrl) up and down,
//! Shift to go faster, Esc to quit. The title shows the frame rate and frame time.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use swjkja_render::path::{self, PathFrame};
use swjkja_render::{
    Camera, FRAMES_IN_FLIGHT, FrameLoop, Gpu, Presented, Swapchain, Target, TextureSet,
    WorldRenderer,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

use crate::fly::{self, Keys};

pub struct Options {
    pub game_data: PathBuf,
    /// As the original names it, e.g. `mp/ffa3`.
    pub map: String,
    pub vsync: bool,
    /// Exclusive fullscreen at [`SIZE`] (the screen switches to that mode), so the
    /// desktop compositor is out of the way.
    pub fullscreen: bool,
    /// Replay this path once instead of flying (after [`WARM_UP_FRAMES`] of its first
    /// view), then write it with this viewer's timestamps to `out` and quit.
    pub replay: Option<(Vec<PathFrame>, PathBuf)>,
    /// Write every frame's step times (see [`write_timings`]) here on quitting.
    pub timings: Option<PathBuf>,
}

/// Frames of a replay's first view drawn before the timed replay starts. The first
/// presents after the window opens (in fullscreen, after the mode switch) stall for
/// up to 160 ms, while the original's timed demo starts long after its own. 100
/// frames is about half a second.
const WARM_UP_FRAMES: usize = 100;

/// The window's size when it opens: the benchmark's 1280x720, in physical pixels.
const SIZE: (u32, u32) = (1280, 720);

pub fn run(options: Options) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        options,
        view: None,
        error: None,
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}

struct App {
    options: Options,
    /// Created when the event loop starts (winit creates windows only then).
    view: Option<View>,
    error: Option<String>,
}

/// Everything that lives while the window is open. Field order is drop order: the
/// frames in flight and the scene go before the GPU objects they use.
struct View {
    frames: FrameLoop,
    /// The loaded map, once [`View::loading`] is done.
    scene: Option<Scene>,
    loading: Option<Loading>,
    target: Target,
    swapchain: Swapchain,
    gpu: Arc<Gpu>,
    window: Window,
    map: String,
    keys: Keys,
    /// When the scene became ready (frame times and replays count from there).
    start: Instant,
    last_frame: Instant,
    /// When the last frame's work ended (the event loop runs until the next).
    frame_end: Instant,
    /// Frame times since the title was last updated, and GPU times.
    stats: (Vec<f64>, Vec<f64>, Instant),
    /// Replay: warm-up frames left, the next frame and each frame's timestamp so far.
    replay: Option<(usize, usize, Vec<i64>)>,
    /// With `--timings`: every frame's [`Timing`].
    timings: Option<Vec<Timing>>,
}

/// The map, ready to draw. The renderer goes before the textures it uses.
struct Scene {
    renderer: WorldRenderer,
    textures: TextureSet,
    depth_slot: u32,
    camera: Camera,
}

/// The map loading on its own thread, while the window shows a progress bar.
struct Loading {
    thread: std::thread::JoinHandle<Result<Loaded, String>>,
    /// How many of [`STEPS`] are done.
    done: Arc<AtomicUsize>,
    /// The step shown in the title.
    shown: Option<usize>,
    /// When to draw the next loading frame.
    next_frame: Instant,
    /// When the window was asked for.
    opened: Instant,
}

/// What the loading thread hands over.
struct Loaded {
    renderer: WorldRenderer,
    textures: TextureSet,
    /// The camera's start: origin and yaw.
    spawn: ([f32; 3], f32),
}

/// The steps of loading a map, in order, with their share of the loading time: the
/// progress bar advances by these. Measured on mp/ffa3 with the game files not yet
/// in the file cache (8.8 s, 3.2 s of it mounting the game data), when the bar is
/// up long enough to matter. From the cache the whole
/// load takes 0.75-1.2 s, most of it textures and pipelines.
const STEPS: [(&str, f32); 5] = [
    ("game data", 0.36),
    ("shaders", 0.005),
    ("map", 0.01),
    ("world", 0.005),
    ("textures and pipelines", 0.62),
];

/// Loading frames are drawn at about 60 per second: more would only take CPU and
/// GPU time from the loading thread.
const LOADING_FRAME: Duration = Duration::from_micros(16_667);

/// How far loading is, from 0 to 1, once `done` of [`STEPS`] are done.
fn progress(done: usize) -> f32 {
    // Not `sum`: summing no floats gives -0.0, which prints as "-0".
    let share = |steps: &[(&str, f32)]| steps.iter().fold(0.0, |sum, s| sum + s.1);
    share(&STEPS[..done.min(STEPS.len())]) / share(&STEPS)
}

/// The window's title while loading, once `done` of [`STEPS`] are done.
fn loading_title(map: &str, done: usize) -> String {
    format!(
        "swjkja: {map} | loading: {} ({:.0}%)",
        STEPS[done.min(STEPS.len() - 1)].0,
        progress(done) * 100.0
    )
}

/// The progress bar in a `width` x `height` image, as (x, y, width, height): its
/// track, half as wide as the image and centred, and the part filled at `fraction`.
fn progress_bar(width: u32, height: u32, fraction: f32) -> ([u32; 4], [u32; 4]) {
    let (w, h) = (width / 2, (height / 90).max(2));
    let (x, y) = ((width - w) / 2, (height - h) / 2);
    let filled = (w as f32 * fraction.clamp(0.0, 1.0)).round() as u32;
    ([x, y, w, h], [x, y, filled, h])
}

/// One frame's line in the `--timings` file (times in ms).
#[derive(Clone, Copy)]
struct Timing {
    /// When the frame started, since the view opened.
    t: f64,
    /// Since the previous frame started.
    dt: f64,
    /// In the event loop between the previous frame's end and this one's start.
    event_loop: f64,
    presented: Presented,
}

impl App {
    /// Opens the window and starts loading the map on its own thread, so the window
    /// shows frames and handles events from the start.
    fn open(&self, event_loop: &ActiveEventLoop) -> Result<View, String> {
        let opened = Instant::now();
        let o = &self.options;
        let mut attributes = Window::default_attributes()
            .with_title(format!("swjkja: {}", o.map))
            .with_inner_size(PhysicalSize::new(SIZE.0, SIZE.1));
        if o.fullscreen {
            // The fastest refresh rate the monitor has at that size.
            let mode = event_loop
                .primary_monitor()
                .into_iter()
                .flat_map(|m| m.video_modes())
                .filter(|m| (m.size().width, m.size().height) == SIZE)
                .max_by_key(|m| (m.bit_depth(), m.refresh_rate_millihertz()))
                .ok_or(format!("the monitor has no {}x{} mode", SIZE.0, SIZE.1))?;
            attributes = attributes.with_fullscreen(Some(Fullscreen::Exclusive(mode)));
        }
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| e.to_string())?;
        let (gpu, surface) = Gpu::new_for_window(
            window.display_handle().map_err(|e| e.to_string())?.as_raw(),
            window.window_handle().map_err(|e| e.to_string())?.as_raw(),
        )
        .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut swapchain = Swapchain::new(&gpu, surface, (size.width, size.height), o.vsync)
            .map_err(|e| e.to_string())?;
        swapchain.direct = !o.fullscreen;
        let extent = swapchain.extent;
        let target = Target::new(&gpu, extent.width, extent.height).map_err(|e| e.to_string())?;

        let done = Arc::new(AtomicUsize::new(0));
        let thread = {
            let (gpu, done) = (gpu.clone(), done.clone());
            let (game_data, map) = (o.game_data.clone(), o.map.clone());
            std::thread::Builder::new()
                .name("load".into())
                .spawn(move || load(&gpu, &game_data, &map, &done))
                .map_err(|e| e.to_string())?
        };
        let now = Instant::now();
        Ok(View {
            frames: FrameLoop::new(&gpu).map_err(|e| e.to_string())?,
            scene: None,
            loading: Some(Loading {
                thread,
                done,
                shown: None,
                next_frame: now,
                opened,
            }),
            target,
            swapchain,
            gpu,
            window,
            map: o.map.clone(),
            keys: Keys::default(),
            start: now,
            last_frame: now,
            frame_end: now,
            stats: (Vec::new(), Vec::new(), now),
            replay: o.replay.as_ref().map(|_| (WARM_UP_FRAMES, 0, Vec::new())),
            timings: o.timings.as_ref().map(|_| Vec::new()),
        })
    }
}

/// Loads the map and uploads it, counting finished [`STEPS`] in `done`, and prints
/// how long each step took.
fn load(gpu: &Arc<Gpu>, game_data: &Path, map: &str, done: &AtomicUsize) -> Result<Loaded, String> {
    let start = Instant::now();
    let mut last = start;
    let mut times = Vec::new();
    let mut step = |i: usize| {
        let now = Instant::now();
        times.push(format!(
            "{} {:.2} s",
            STEPS[i].0,
            (now - last).as_secs_f64()
        ));
        last = now;
        done.store(i + 1, Ordering::Relaxed);
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[game_data.to_owned()], &["base"])
        .map_err(|e| format!("{}: {e}", game_data.display()))?;
    step(0);
    let shaders = swjkja_shader::ShaderText::load(&vfs).map_err(|e| e.to_string())?;
    step(1);
    let file = format!("maps/{map}.bsp");
    let bsp = vfs
        .read(&file)
        .map_err(|e| format!("{file}: {e}"))
        .and_then(|data| swjkja_bsp::Bsp::parse(&data).map_err(|e| format!("{file}: {e}")))?;
    step(2);
    let world = swjkja_world::World::load(&bsp, &Default::default());
    step(3);
    let mut textures = TextureSet::new(gpu).map_err(|e| e.to_string())?;
    let renderer = WorldRenderer::new(gpu, &mut textures, &vfs, &shaders, &bsp, &world)
        .map_err(|e| e.to_string())?;
    step(4);
    eprintln!(
        "{map}: loaded in {:.2} s ({})",
        start.elapsed().as_secs_f64(),
        times.join(", ")
    );
    Ok(Loaded {
        renderer,
        textures,
        spawn: spawn_point(&bsp.entities),
    })
}

/// The first player start (multiplayer or single-player) with the eyes 36 units up,
/// as the reference views use; the map's origin if it has none.
fn spawn_point(entities: &[swjkja_bsp::Entity]) -> ([f32; 3], f32) {
    entities
        .iter()
        .find(|e| {
            matches!(
                e.get("classname"),
                Some("info_player_deathmatch" | "info_player_start")
            )
        })
        .and_then(|e| {
            let o: Vec<f32> = e
                .get("origin")?
                .split_whitespace()
                .map(|v| v.parse().ok())
                .collect::<Option<_>>()?;
            let yaw = e.get("angle").and_then(|a| a.parse().ok()).unwrap_or(0.0);
            Some(([*o.first()?, *o.get(1)?, *o.get(2)? + 36.0], yaw))
        })
        .unwrap_or(([0.0; 3], 0.0))
}

fn grab_mouse(window: &Window) {
    // Locked keeps the cursor still (not on Windows); Confined keeps it in the window.
    let _ = window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
    window.set_cursor_visible(false);
}

impl View {
    /// Draws and presents one frame. Returns false when the replay has finished.
    fn frame(&mut self, replay: Option<&(Vec<PathFrame>, PathBuf)>) -> Result<bool, String> {
        let Some(scene) = &mut self.scene else {
            self.loading_frame(replay.is_some())?;
            return Ok(true);
        };
        let now = Instant::now();
        let dt = now - self.last_frame;
        self.last_frame = now;
        let event_loop = now - self.frame_end;
        let (camera, time) = match (&mut self.replay, replay) {
            (Some((warm_up, next, usec)), Some((path, _))) => {
                let Some(f) = path.get(*next) else {
                    return Ok(false);
                };
                if *warm_up > 0 {
                    *warm_up -= 1;
                } else {
                    usec.push((now - self.start).as_micros() as i64);
                    *next += 1;
                }
                let e = self.swapchain.extent;
                (f.camera(e.width, e.height), f.time_ms as f32 / 1000.0)
            }
            _ => {
                fly::fly(&mut scene.camera, self.keys, dt.as_secs_f32());
                (scene.camera, (now - self.start).as_secs_f32())
            }
        };
        let (renderer, textures, depth_slot) =
            (&mut scene.renderer, &scene.textures, scene.depth_slot);
        let presented = self
            .frames
            .present(
                &self.swapchain,
                &self.target,
                renderer.clear_color(),
                |frame| {
                    renderer
                        .draw(frame, textures, &camera, time, depth_slot)
                        .unwrap()
                },
            )
            .map_err(|e| e.to_string())?;
        if presented.out_of_date {
            self.resize()?;
        }
        if let Some(t) = &mut self.timings {
            t.push(Timing {
                t: (now - self.start).as_secs_f64() * 1000.0,
                dt: dt.as_secs_f64() * 1000.0,
                event_loop: event_loop.as_secs_f64() * 1000.0,
                presented,
            });
        }
        self.stats.0.push(dt.as_secs_f64() * 1000.0);
        self.stats.1.extend(presented.gpu.map(|g| g.world_ms));
        if now - self.stats.2 >= Duration::from_millis(500) {
            let (frames, gpu, since) = &mut self.stats;
            let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
            self.window.set_title(&format!(
                "swjkja: {} | {:.0} FPS, {:.2} ms (GPU {:.2} ms)",
                self.map,
                1000.0 / mean(frames),
                mean(frames),
                mean(gpu)
            ));
            frames.clear();
            gpu.clear();
            *since = now;
        }
        self.frame_end = Instant::now();
        Ok(true)
    }

    /// Matches the swapchain and target to the window's size.
    fn resize(&mut self) -> Result<(), String> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(()); // Minimised: nothing to draw into.
        }
        self.frames.finish().map_err(|e| e.to_string())?;
        self.swapchain
            .resize((size.width, size.height))
            .map_err(|e| e.to_string())?;
        let e = self.swapchain.extent;
        self.target = Target::new(&self.gpu, e.width, e.height).map_err(|e| e.to_string())?;
        if let Some(scene) = &mut self.scene {
            scene
                .textures
                .set_view(scene.depth_slot, self.target.depth_copy.view);
            (scene.camera.width, scene.camera.height) = (e.width, e.height);
        }
        Ok(())
    }

    /// Draws a loading frame: the progress bar on black, with the step in the title.
    /// Takes the map once the loading thread is done.
    fn loading_frame(&mut self, replay: bool) -> Result<(), String> {
        let loading = self
            .loading
            .as_mut()
            .expect("loading until the scene is ready");
        if loading.thread.is_finished() {
            let thread = self.loading.take().unwrap().thread;
            let loaded = thread
                .join()
                .map_err(|_| "loading the map failed (the thread panicked)".to_string())??;
            self.ready(loaded, replay);
            return Ok(());
        }
        let done = loading.done.load(Ordering::Relaxed);
        let first = loading.shown.is_none();
        if loading.shown != Some(done) {
            loading.shown = Some(done);
            self.window.set_title(&loading_title(&self.map, done));
        }
        let e = self.swapchain.extent;
        let (track, filled) = progress_bar(e.width, e.height, progress(done));
        let presented = self
            .frames
            .present(
                &self.swapchain,
                &self.target,
                [0.0, 0.0, 0.0, 1.0],
                |frame| {
                    frame.fill(track, [0.2, 0.2, 0.2, 1.0]);
                    frame.fill(filled, [0.8, 0.8, 0.8, 1.0]);
                },
            )
            .map_err(|e| e.to_string())?;
        if first && let Some(loading) = &self.loading {
            eprintln!(
                "first frame presented {:.0} ms after opening the window",
                loading.opened.elapsed().as_secs_f64() * 1000.0
            );
        }
        if presented.out_of_date {
            self.resize()?;
        }
        Ok(())
    }

    /// Makes the loaded map the scene, at its spawn point; frame times start now.
    fn ready(&mut self, loaded: Loaded, replay: bool) {
        let Loaded {
            renderer,
            mut textures,
            spawn: (origin, yaw),
        } = loaded;
        let depth_slot = textures.add_view(self.target.depth_copy.view);
        let e = self.swapchain.extent;
        if replay && (e.width, e.height) != SIZE {
            eprintln!(
                "warning: the window is {}x{}, not {}x{}; replay times are not comparable",
                e.width, e.height, SIZE.0, SIZE.1
            );
        } else if !replay {
            grab_mouse(&self.window);
        }
        eprintln!(
            "{}: {} triangles, {} materials on {} ({:?}, {:?})",
            self.map,
            renderer.triangles,
            renderer.material_count,
            self.gpu.name,
            self.swapchain.format,
            self.swapchain.present_mode
        );
        self.window.set_title(&format!("swjkja: {}", self.map));
        self.scene = Some(Scene {
            renderer,
            textures,
            depth_slot,
            camera: Camera::new(origin, [0.0, yaw, 0.0], e.width, e.height),
        });
        let now = Instant::now();
        (self.start, self.last_frame, self.frame_end) = (now, now, now);
        self.stats = (Vec::new(), Vec::new(), now);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        // The first loading frame is drawn now rather than at the first redraw, which
        // comes about 100 ms later while Windows sets the new window up.
        let replay = self.options.replay.is_some();
        match self
            .open(event_loop)
            .and_then(|mut view| view.loading_frame(replay).map(|()| view))
        {
            Ok(view) => self.view = Some(view),
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(view) = &mut self.view else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => {
                if let Err(e) = view.resize() {
                    self.error = Some(e);
                    event_loop.exit();
                }
            }
            // The mouse is grabbed once the map is ready: while it loads, the window can
            // still be moved.
            WindowEvent::Focused(true) if self.options.replay.is_none() && view.scene.is_some() => {
                grab_mouse(&view.window)
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                let down = state == ElementState::Pressed;
                let k = &mut view.keys;
                match code {
                    KeyCode::Escape if down => event_loop.exit(),
                    KeyCode::KeyW | KeyCode::ArrowUp => k.forward = down,
                    KeyCode::KeyS | KeyCode::ArrowDown => k.back = down,
                    KeyCode::KeyA | KeyCode::ArrowLeft => k.left = down,
                    KeyCode::KeyD | KeyCode::ArrowRight => k.right = down,
                    KeyCode::Space => k.up = down,
                    KeyCode::KeyC | KeyCode::ControlLeft => k.down = down,
                    KeyCode::ShiftLeft | KeyCode::ShiftRight => k.fast = down,
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => match view.frame(self.options.replay.as_ref()) {
                Ok(true) => {}
                Ok(false) => {
                    // Replay done: write this viewer's timestamps and quit.
                    if let (Some((path, out)), Some((_, _, usec))) =
                        (&self.options.replay, &view.replay)
                    {
                        if let Err(e) = std::fs::write(out, path::write(path, usec)) {
                            self.error = Some(format!("{}: {e}", out.display()));
                        } else {
                            eprintln!("{} frames written to {}", usec.len(), out.display());
                        }
                    }
                    event_loop.exit();
                }
                Err(e) => {
                    self.error = Some(e);
                    event_loop.exit();
                }
            },
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let (Some(view), DeviceEvent::MouseMotion { delta }) = (&mut self.view, event)
            && let Some(scene) = &mut view.scene
            && self.options.replay.is_none()
            && view.window.has_focus()
        {
            fly::look(&mut scene.camera, delta.0, delta.1);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(view) = &mut self.view else { return };
        match &mut view.loading {
            Some(loading) => {
                let now = Instant::now();
                if now >= loading.next_frame {
                    view.window.request_redraw();
                    loading.next_frame = now + LOADING_FRAME;
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(loading.next_frame));
            }
            None => {
                event_loop.set_control_flow(ControlFlow::Poll);
                view.window.request_redraw();
            }
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some(view) = &mut self.view {
            let _ = view.frames.finish();
            if let (Some(out), Some(rows)) = (&self.options.timings, &view.timings) {
                match std::fs::write(out, write_timings(rows)) {
                    Ok(()) => eprintln!(
                        "{} frames' timings written to {}",
                        rows.len(),
                        out.display()
                    ),
                    Err(e) => self.error = Some(format!("{}: {e}", out.display())),
                }
            }
        }
        self.view = None;
    }
}

/// The `--timings` file: a header, then one tab-separated line per frame. CPU
/// columns are the steps of presenting that frame; GPU columns are that frame's own
/// timestamps (reported [`FRAMES_IN_FLIGHT`] frames later, so the last frames have
/// none and show `-`). `gpu_start_ms` is on the GPU's clock, relative to the first
/// frame: the gap to the previous frame's end is GPU time spent on other work.
fn write_timings(rows: &[Timing]) -> String {
    let mut out = String::from(
        "frame\tt_ms\tdt_ms\tloop_ms\twait_ms\tacquire_ms\tsubmit_ms\tpresent_ms\t\
         gpu_start_ms\tgpu_world_ms\tgpu_blit_ms\n",
    );
    let first = rows
        .iter()
        .find_map(|r| r.presented.gpu)
        .map_or(0.0, |g| g.start_ms);
    let opt = |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{v:.3}"));
    for (i, r) in rows.iter().enumerate() {
        let gpu = rows.get(i + FRAMES_IN_FLIGHT).and_then(|r| r.presented.gpu);
        let c = r.presented.cpu;
        out += &format!(
            "{i}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{}\t{}\t{}\n",
            r.t,
            r.dt,
            r.event_loop,
            c.wait_ms,
            c.acquire_ms,
            c.submit_ms,
            c.present_ms,
            opt(gpu.map(|g| g.start_ms - first)),
            opt(gpu.map(|g| g.world_ms)),
            opt(gpu.and_then(|g| g.blit_ms)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use swjkja_render::{GpuTimes, Steps};

    #[test]
    fn progress_counts_finished_steps_by_their_share() {
        assert_eq!(progress(0), 0.0);
        assert_eq!(progress(STEPS.len()), 1.0);
        assert_eq!(progress(STEPS.len() + 1), 1.0);
        for done in 1..=STEPS.len() {
            assert!(progress(done) > progress(done - 1));
        }
    }

    #[test]
    fn loading_title_names_the_step_under_way() {
        assert_eq!(
            loading_title("mp/ffa3", 0),
            "swjkja: mp/ffa3 | loading: game data (0%)"
        );
        assert_eq!(
            loading_title("mp/ffa3", 4),
            "swjkja: mp/ffa3 | loading: textures and pipelines (38%)"
        );
    }

    #[test]
    fn progress_bar_is_centred_and_filled_from_the_left() {
        assert_eq!(
            progress_bar(1280, 720, 0.25),
            ([320, 356, 640, 8], [320, 356, 160, 8])
        );
        assert_eq!(progress_bar(1280, 720, 0.0).1, [320, 356, 0, 8]);
        assert_eq!(progress_bar(1280, 720, 1.5).1, [320, 356, 640, 8]);
        // Tiny windows still get a visible bar.
        assert_eq!(progress_bar(10, 10, 1.0), ([2, 4, 5, 2], [2, 4, 5, 2]));
    }

    #[test]
    fn timings_put_gpu_times_on_the_frame_they_belong_to() {
        let frame = |i: usize, gpu: Option<GpuTimes>| Timing {
            t: i as f64 * 4.0,
            dt: 4.0,
            event_loop: 0.0625,
            presented: Presented {
                gpu,
                cpu: Steps {
                    wait_ms: 1.0,
                    acquire_ms: 0.5,
                    submit_ms: 0.25,
                    present_ms: 0.125,
                },
                out_of_date: false,
            },
        };
        // Frame 2 reports frame 0's times, frame 3 frame 1's.
        let gpu = |start_ms| GpuTimes {
            start_ms,
            world_ms: 3.0,
            blit_ms: Some(0.2),
        };
        let rows = [
            frame(0, None),
            frame(1, None),
            frame(2, Some(gpu(100.0))),
            frame(3, Some(gpu(104.5))),
        ];
        assert_eq!(
            write_timings(&rows),
            "frame\tt_ms\tdt_ms\tloop_ms\twait_ms\tacquire_ms\tsubmit_ms\tpresent_ms\t\
             gpu_start_ms\tgpu_world_ms\tgpu_blit_ms\n\
             0\t0.000\t4.000\t0.062\t1.000\t0.500\t0.250\t0.125\t0.000\t3.000\t0.200\n\
             1\t4.000\t4.000\t0.062\t1.000\t0.500\t0.250\t0.125\t4.500\t3.000\t0.200\n\
             2\t8.000\t4.000\t0.062\t1.000\t0.500\t0.250\t0.125\t-\t-\t-\n\
             3\t12.000\t4.000\t0.062\t1.000\t0.500\t0.250\t0.125\t-\t-\t-\n"
        );
    }
}
