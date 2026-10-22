//! The window's main loop (m4-plan Decision 2).
//!
//! winit owns the main thread and calls [`App`] back: for each window and
//! device event, and in `about_to_wait` once it has handled everything that
//! arrived. The embedded server stays on its own thread, and the
//! `LocalChannel` is still the only thing between them.
//!
//! **Ticks don't depend on frames** (worth teaching). The host sends one
//! `PlayerAuthInput` every 50 ms, as the official client does, and the
//! server drops a client it hasn't heard from in 10 s. Frames come at the
//! display's rate while the window is visible, and not at all while it's
//! minimised. So `about_to_wait` sends the ticks that are due, whatever
//! happened to frames, and then asks winit to wake it at the next tick at
//! the latest (`ControlFlow::WaitUntil`).

use std::{
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use glam::DVec3;
use pumpkin_util::math::vector3::Vector3;
use tracing::{debug, error, info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::{CursorGrabMode, Window, WindowId},
};

use crate::{
    embedded::EmbeddedServer,
    input::{Action, Input},
    render::{Frame, Gpu, Needs, SKY, camera::Camera},
    session::{Session, Stage, TICK},
    settings::Settings,
};

/// After a stall, at most this many overdue ticks are sent at once. The rest
/// are skipped: a burst of inputs would only make the server's player jump.
const MAX_CATCH_UP: u32 = 3;
/// Bedrock's eye height above the feet, for `/tp`, which places the feet.
const EYE_HEIGHT: f64 = 1.62;
/// The longest step the camera takes in one frame, so a hitch doesn't fling
/// it far away.
const MAX_FRAME_SECONDS: f64 = 0.25;

/// Opens the window, joins the embedded server as the host, and runs until
/// the window closes or the server ends the connection.
pub fn run(server: &EmbeddedServer, name: &str, radius: i32, settings: Settings, needs: Needs) -> ExitCode {
    let session = match Session::connect(&server.connector, name, radius) {
        Ok(session) => session,
        Err(err) => {
            error!("Couldn't connect to the embedded server: {err}");
            return ExitCode::FAILURE;
        }
    };
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            error!("Couldn't start the window system: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut app = App::new(server, session, settings, needs);
    if let Err(err) = event_loop.run_app(&mut app) {
        error!("The window's event loop failed: {err}");
        return ExitCode::FAILURE;
    }
    // Dropping the app drops the session, so the host leaves before the
    // caller stops the server.
    match app.failure {
        Some(err) => {
            error!("{err}");
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}

pub struct App<'a> {
    server: &'a EmbeddedServer,
    session: Session,
    settings: Settings,
    needs: Needs,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    /// Placed where `StartGame` put the host, once it has spawned.
    camera: Option<Camera>,
    input: Input,
    cursor_grabbed: bool,
    /// Whether frames can be seen: not minimised or covered.
    visible: bool,
    next_tick: Instant,
    last_frame: Instant,
    /// Counting frames for the title, which updates once a second.
    second_started: Instant,
    frames_this_second: u32,
    /// Why the loop ended early, if it did.
    failure: Option<String>,
}

impl<'a> App<'a> {
    fn new(server: &'a EmbeddedServer, session: Session, settings: Settings, needs: Needs) -> Self {
        let now = Instant::now();
        Self {
            server,
            session,
            settings,
            needs,
            window: None,
            gpu: None,
            camera: None,
            input: Input::default(),
            cursor_grabbed: false,
            visible: true,
            next_tick: now,
            last_frame: now,
            second_started: now,
            frames_this_second: 0,
            failure: None,
        }
    }

    /// Sends the ticks that are due: one `PlayerAuthInput` per 50 ms, with
    /// the camera's pose once there is a camera.
    fn send_ticks(&mut self) {
        let now = Instant::now();
        let mut sent = 0;
        while now >= self.next_tick {
            if sent == MAX_CATCH_UP {
                self.next_tick = now + TICK;
                break;
            }
            if let Some(camera) = &self.camera {
                let eye = camera.position.as_vec3();
                self.session
                    .set_pose(Vector3::new(eye.x, eye.y, eye.z), camera.pitch, camera.yaw);
            }
            self.session.send_input();
            self.next_tick += TICK;
            sent += 1;
        }
    }

    /// Places the camera where the host spawned, once.
    fn place_camera(&mut self) {
        if self.camera.is_some() || self.session.stage() != Stage::Spawned {
            return;
        }
        let Some(spawn) = self.session.spawn() else {
            return;
        };
        let eye = spawn.eye_position;
        let mut camera = Camera::new(
            DVec3::new(f64::from(eye.x), f64::from(eye.y), f64::from(eye.z)),
            spawn.pitch,
            spawn.yaw,
        );
        if let Some(window) = &self.window {
            let size = window.inner_size();
            camera.aspect = aspect(size);
        }
        self.camera = Some(camera);
        info!("The camera is at the host's spawn; click in the window to look around");
        self.session.check_gamemode(self.settings.gamemode);
    }

    fn grab_cursor(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        // Locked keeps the cursor still; where it's unavailable (Windows),
        // Confined keeps it in the window. Found out by trying, not by
        // checking the OS.
        let grabbed = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .map(|()| CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined).map(|()| CursorGrabMode::Confined));
        match grabbed {
            Ok(mode) => {
                window.set_cursor_visible(false);
                self.cursor_grabbed = true;
                debug!("Cursor captured ({mode:?})");
            }
            Err(err) => warn!("Couldn't capture the cursor: {err}"),
        }
    }

    fn release_cursor(&mut self) {
        if let Some(window) = &self.window {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
        self.cursor_grabbed = false;
    }

    /// Logs the camera's pose as a command to type into the official client,
    /// to look from the same place.
    fn log_pose(&self) {
        if let Some(camera) = &self.camera {
            let p = camera.position;
            info!(
                "Pose: /tp @s {:.2} {:.2} {:.2} {:.1} {:.1}",
                p.x,
                p.y - EYE_HEIGHT,
                p.z,
                camera.yaw,
                camera.pitch
            );
        }
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let seconds = (now - self.last_frame).as_secs_f64().min(MAX_FRAME_SECONDS);
        self.last_frame = now;

        for action in self.input.take_pressed() {
            match action {
                Action::ReleaseCursor => self.release_cursor(),
                Action::LogPose => self.log_pose(),
                _ => {}
            }
        }
        let (dx, dy) = self.input.take_mouse();
        if let Some(camera) = &mut self.camera {
            if self.cursor_grabbed {
                camera.turn(dx, dy);
            }
            camera.fly(self.input.movement(), seconds);
        }

        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let drawn = gpu.frame(|_, view, encoder| {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(SKY),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        });
        if drawn == Frame::Drawn {
            self.frames_this_second += 1;
        }
        self.once_a_second(now);
    }

    /// The title, and at debug level the camera beside the server's own
    /// copy of the host, which should follow it a tick or two behind.
    fn once_a_second(&mut self, now: Instant) {
        let elapsed = now - self.second_started;
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let fps = f64::from(self.frames_this_second) / elapsed.as_secs_f64();
        self.second_started = now;
        self.frames_this_second = 0;
        let Some(camera) = &self.camera else {
            return;
        };
        let p = camera.position;
        if let Some(window) = &self.window {
            window.set_title(&format!(
                "Limonite: {fps:.0} fps, {:.1} ms; {} chunks; at {:.1}, {:.1}, {:.1}",
                1000.0 / fps.max(0.001),
                self.session.world().chunk_count(),
                p.x,
                p.y,
                p.z
            ));
        }
        if let Some(player) = self.server.server.get_all_players().first() {
            let server = player.position();
            debug!(
                "Camera eyes at ({:.2}, {:.2}, {:.2}); the server's player feet at ({:.2}, {:.2}, {:.2}); \
                 {} LevelChunks so far",
                p.x,
                p.y,
                p.z,
                server.x,
                server.y,
                server.z,
                self.session.level_chunks()
            );
        }
    }
}

/// Width over height, never dividing by zero.
fn aspect(size: PhysicalSize<u32>) -> f32 {
    size.width.max(1) as f32 / size.height.max(1) as f32
}

impl ApplicationHandler for App<'_> {
    /// The window and the GPU come up here, once: some platforms only allow
    /// creating windows after the loop has started.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Limonite")
            .with_inner_size(PhysicalSize::new(1280, 720));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.failure = Some(format!("Couldn't open a window: {err}"));
                event_loop.exit();
                return;
            }
        };
        match Gpu::new(window.clone(), event_loop.owned_display_handle(), &self.needs) {
            Ok(gpu) => self.gpu = Some(gpu),
            Err(err) => {
                self.failure = Some(format!("Can't draw: {err}"));
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.visible = size.width > 0 && size.height > 0;
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                if let Some(camera) = &mut self.camera {
                    camera.aspect = aspect(size);
                }
            }
            WindowEvent::Occluded(occluded) => self.visible = !occluded,
            WindowEvent::Focused(false) => {
                self.release_cursor();
                self.input.clear();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.input.key(key, event.state);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if !self.cursor_grabbed => self.grab_cursor(),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    /// Raw mouse motion: unaffected by pointer acceleration, and still
    /// reported when the cursor is locked in place.
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && self.cursor_grabbed
        {
            self.input.mouse_motion(dx, dy);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.session.drain();
        if self.session.stage() == Stage::Disconnected {
            self.failure = Some("The embedded server ended the connection".to_string());
            event_loop.exit();
            return;
        }
        self.place_camera();
        self.send_ticks();
        if self.visible
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}
