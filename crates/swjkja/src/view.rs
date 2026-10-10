//! The map viewer: a window showing a map, flown through with the mouse and
//! keyboard (see [`crate::fly`]), or replaying a recorded camera path.
//!
//! Controls: mouse to look, W A S D to move, Space and C (or Ctrl) up and down,
//! Shift to go faster, Esc to quit. The title shows the frame rate and frame time.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use swjkja_render::path::{self, PathFrame};
use swjkja_render::{Camera, FrameLoop, Gpu, Swapchain, Target, TextureSet, WorldRenderer};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::fly::{self, Keys};

pub struct Options {
    pub game_data: PathBuf,
    /// As the original names it, e.g. `mp/ffa3`.
    pub map: String,
    pub vsync: bool,
    /// Replay this path once instead of flying, then write it with this viewer's
    /// timestamps to `out` and quit.
    pub replay: Option<(Vec<PathFrame>, PathBuf)>,
}

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
/// frames in flight and the renderer go before the GPU objects they use.
struct View {
    frames: FrameLoop,
    renderer: WorldRenderer,
    target: Target,
    depth_slot: u32,
    textures: TextureSet,
    swapchain: Swapchain,
    gpu: std::sync::Arc<Gpu>,
    window: Window,
    map: String,
    camera: Camera,
    keys: Keys,
    start: Instant,
    last_frame: Instant,
    /// Frame times since the title was last updated, and GPU times.
    stats: (Vec<f64>, Vec<f64>, Instant),
    /// Replay: the next frame and each frame's timestamp so far.
    replay: Option<(usize, Vec<i64>)>,
}

impl App {
    fn open(&self, event_loop: &ActiveEventLoop) -> Result<View, String> {
        let o = &self.options;
        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title(format!("swjkja: {}", o.map))
                    .with_inner_size(PhysicalSize::new(SIZE.0, SIZE.1)),
            )
            .map_err(|e| e.to_string())?;
        let (gpu, surface) = Gpu::new_for_window(
            window.display_handle().map_err(|e| e.to_string())?.as_raw(),
            window.window_handle().map_err(|e| e.to_string())?.as_raw(),
        )
        .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let swapchain = Swapchain::new(&gpu, surface, (size.width, size.height), o.vsync)
            .map_err(|e| e.to_string())?;

        let vfs = swjkja_fs::Vfs::mount_game(std::slice::from_ref(&o.game_data), &["base"])
            .map_err(|e| format!("{}: {e}", o.game_data.display()))?;
        let shaders = swjkja_shader::ShaderText::load(&vfs).map_err(|e| e.to_string())?;
        let file = format!("maps/{}.bsp", o.map);
        let bsp = vfs
            .read(&file)
            .map_err(|e| format!("{file}: {e}"))
            .and_then(|data| swjkja_bsp::Bsp::parse(&data).map_err(|e| format!("{file}: {e}")))?;
        let world = swjkja_world::World::load(&bsp, &Default::default());
        let mut textures = TextureSet::new(&gpu).map_err(|e| e.to_string())?;
        let renderer = WorldRenderer::new(&gpu, &mut textures, &vfs, &shaders, &bsp, &world)
            .map_err(|e| e.to_string())?;
        let extent = swapchain.extent;
        let target = Target::new(&gpu, extent.width, extent.height).map_err(|e| e.to_string())?;
        let depth_slot = textures.add_view(target.depth_copy.view);

        let (origin, yaw) = spawn_point(&bsp.entities);
        let camera = Camera::new(origin, [0.0, yaw, 0.0], extent.width, extent.height);
        if o.replay.is_some() && (extent.width, extent.height) != SIZE {
            eprintln!(
                "warning: the window is {}x{}, not {}x{}; replay times are not comparable",
                extent.width, extent.height, SIZE.0, SIZE.1
            );
        } else if o.replay.is_none() {
            grab_mouse(&window);
        }
        eprintln!(
            "{}: {} triangles, {} materials on {} ({:?}, {:?})",
            o.map,
            renderer.triangles,
            renderer.material_count,
            gpu.name,
            swapchain.format,
            swapchain.present_mode
        );
        let now = Instant::now();
        Ok(View {
            frames: FrameLoop::new(&gpu).map_err(|e| e.to_string())?,
            renderer,
            target,
            depth_slot,
            textures,
            swapchain,
            gpu,
            window,
            map: o.map.clone(),
            camera,
            keys: Keys::default(),
            start: now,
            last_frame: now,
            stats: (Vec::new(), Vec::new(), now),
            replay: o.replay.as_ref().map(|_| (0, Vec::new())),
        })
    }
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
        let now = Instant::now();
        let dt = now - self.last_frame;
        self.last_frame = now;
        let (camera, time) = match (&mut self.replay, replay) {
            (Some((next, usec)), Some((path, _))) => {
                let Some(f) = path.get(*next) else {
                    return Ok(false);
                };
                usec.push((now - self.start).as_micros() as i64);
                *next += 1;
                let e = self.swapchain.extent;
                (f.camera(e.width, e.height), f.time_ms as f32 / 1000.0)
            }
            _ => {
                fly::fly(&mut self.camera, self.keys, dt.as_secs_f32());
                (self.camera, (now - self.start).as_secs_f32())
            }
        };
        let (renderer, textures, depth_slot) =
            (&mut self.renderer, &self.textures, self.depth_slot);
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
        self.stats.0.push(dt.as_secs_f64() * 1000.0);
        self.stats.1.extend(presented.gpu_ms);
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
        self.textures
            .set_view(self.depth_slot, self.target.depth_copy.view);
        (self.camera.width, self.camera.height) = (e.width, e.height);
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        match self.open(event_loop) {
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
            WindowEvent::Focused(true) if self.options.replay.is_none() => grab_mouse(&view.window),
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
                    if let (Some((path, out)), Some((_, usec))) =
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
            && self.options.replay.is_none()
            && view.window.has_focus()
        {
            fly::look(&mut view.camera, delta.0, delta.1);
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(view) = &self.view {
            view.window.request_redraw();
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some(view) = &mut self.view {
            let _ = view.frames.finish();
        }
        self.view = None;
    }
}
