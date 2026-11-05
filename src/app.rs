//! The windowed run: winit's event loop driving one window.

use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::args::RunArgs;
use crate::context::UserEvent;
use crate::gamepad::Gamepads;
use crate::window::WindowState;
use crate::{Config, Game, LoopMode, Result};

/// How often a sleeping turn-based loop checks gamepads, whose events do
/// not wake the window system.
const PAD_POLL: Duration = Duration::from_millis(16);

struct App<G: Game> {
    config: Config,
    args: RunArgs,
    proxy: EventLoopProxy<UserEvent>,
    window: Option<WindowState<G>>,
    gamepads: Option<Gamepads>,
    result: Result<()>,
}

impl<G: Game> App<G> {
    /// Records the first error and ends the loop.
    fn fail(&mut self, el: &ActiveEventLoop, result: Result<()>) {
        if let Err(e) = result {
            if self.result.is_ok() {
                self.result = Err(e);
            }
            el.exit();
        }
    }

    /// Ends the loop when the game quit or `--seconds` elapsed.
    fn check_end(&mut self, el: &ActiveEventLoop) {
        let Some(w) = &self.window else {
            return;
        };
        let out_of_time = self.args.seconds.is_some_and(|s| w.ctx.time.elapsed() >= s);
        if w.ctx.requests.quit || out_of_time {
            el.exit();
        }
    }
}

impl<G: Game> ApplicationHandler<UserEvent> for App<G> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        match WindowState::create(el, &self.config, self.args.fixed_clock, self.proxy.clone()) {
            Ok(w) => self.window = Some(w),
            Err(e) => self.fail(el, Err(e)),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(w) = &mut self.window else {
            return;
        };
        let result = if id == w.window.id() {
            match event {
                WindowEvent::RedrawRequested => {
                    let draw = !w.hidden();
                    w.frame(draw)
                }
                other => w.window_event(other),
            }
        } else if let Some(extra) = w.ctx.windows.by_native(id) {
            // Extra windows are drawn with the main window's frame.
            match event {
                WindowEvent::RedrawRequested => Ok(()),
                other => w.window_event_for(extra, other),
            }
        } else {
            return;
        };
        w.apply_requests(el);
        let result = result.and_then(|()| w.apply_windows(el));
        self.fail(el, result);
        self.check_end(el);
    }

    fn device_event(&mut self, el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let (Some(w), DeviceEvent::MouseMotion { delta }) = (&mut self.window, event) {
            let result = w.mouse_motion(delta);
            self.fail(el, result);
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::Wake = event;
        if let Some(w) = &mut self.window {
            w.turn_pending = true;
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(w) = &mut self.window else {
            return;
        };
        let mut result = Ok(());
        if let Some(pads) = &mut self.gamepads {
            for ev in pads.poll() {
                result = result.and_then(|()| w.input(ev));
            }
        }
        let now = Instant::now();
        let flow = match self.config.loop_mode {
            LoopMode::FixedStep { .. } if w.hidden() => {
                // Hidden windows keep updating at the fixed rate without
                // drawing, asleep between steps.
                result = result.and_then(|()| w.frame(false));
                ControlFlow::WaitUntil(now + Duration::from_secs_f64(w.step()))
            }
            LoopMode::FixedStep { .. } => {
                w.window.request_redraw();
                ControlFlow::Wait
            }
            LoopMode::TurnBased => {
                let r = &mut w.ctx.requests;
                if r.wake_at.is_some_and(|t| t <= now) {
                    r.wake_at = None;
                    w.turn_pending = true;
                }
                if std::mem::take(&mut r.redraw) {
                    w.turn_pending = true;
                }
                if w.turn_pending && w.hidden() {
                    result = result.and_then(|()| w.frame(false));
                } else if w.turn_pending {
                    w.window.request_redraw();
                }
                let mut deadline = w.ctx.requests.wake_at;
                if self.gamepads.as_ref().is_some_and(Gamepads::any_connected) {
                    let poll = now + PAD_POLL;
                    deadline = Some(deadline.map_or(poll, |d| d.min(poll)));
                }
                deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
            }
        };
        w.apply_requests(el);
        let result = result.and_then(|()| w.apply_windows(el));
        el.set_control_flow(flow);
        self.fail(el, result);
        self.check_end(el);
    }
}

/// Runs the game in a window until it quits, the window closes, or
/// `--seconds` of game time pass.
pub(crate) fn run_windowed<G: Game>(config: Config, args: RunArgs) -> Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut app = App::<G> {
        proxy: event_loop.create_proxy(),
        gamepads: Gamepads::new(),
        config,
        args,
        window: None,
        result: Ok(()),
    };
    event_loop.run_app(&mut app)?;
    app.result
}
