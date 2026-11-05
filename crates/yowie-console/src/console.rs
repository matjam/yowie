//! The console object: state, registration and the per-frame entry points.

use std::collections::{BTreeMap, VecDeque};

use yowie_core::Rect;
use yowie_gfx::{FrameStats, Graphics, Texture, layers};
use yowie_input::{InputState, Key};
use yowie_ui::{Frame, Theme, Ui};

use crate::history::History;
use crate::logcap::{LogCapture, LogLine};
use crate::vars::{BoolVar, FloatVar, IntVar, StringVar, Var};

/// The result of a command: text to print, or an error message to print
/// in red.
pub type CommandResult = Result<String, String>;

/// Reads a script file by path for `exec`; the error is printed.
pub type FileReader = Box<dyn Fn(&str) -> Result<String, String>>;

type PanelBody = Box<dyn FnMut(&mut Frame<'_>)>;

pub(crate) type Handler = Box<dyn FnMut(&[String]) -> CommandResult>;
pub(crate) type Completer = Box<dyn Fn(&[String]) -> Vec<String>>;

pub(crate) struct Command {
    pub(crate) help: String,
    pub(crate) run: Handler,
    pub(crate) complete: Option<Completer>,
}

/// Something the console asks the engine to do, which it cannot do itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// End the game.
    Quit,
    /// Save the next frame to this PNG path.
    Screenshot(String),
}

/// One frame's timings, in milliseconds, for the frame graph.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameTimes {
    /// Smoothed frames per second.
    pub fps: f32,
    /// Wall time of the frame.
    pub frame_ms: f32,
    /// Time spent in updates.
    pub update_ms: f32,
    /// Time spent recording the draw.
    pub draw_ms: f32,
    /// What the renderer reported for the previous frame.
    pub stats: FrameStats,
}

/// How the console looks and which keys control it.
pub struct ConsoleOptions {
    /// Opens and closes the drop-down. Backtick by default.
    pub toggle_key: Key,
    /// Opens and closes the debug panels. F4 by default.
    pub panel_key: Key,
    /// How many output and log lines are kept.
    pub lines: usize,
    /// The drop-down's height as a fraction of the view.
    pub height: f32,
    /// Reads files for the `exec` command, so scripts can come from a
    /// pack file; reads from disk when `None`.
    pub reader: Option<FileReader>,
}

impl Default for ConsoleOptions {
    fn default() -> Self {
        Self {
            toggle_key: Key::Grave,
            panel_key: Key::F4,
            lines: 1000,
            height: 0.45,
            reader: None,
        }
    }
}

/// The graphics context facts the GPU panel shows.
#[derive(Clone, Debug, Default)]
pub(crate) struct GpuInfo {
    pub(crate) adapter: String,
    pub(crate) backend: String,
    pub(crate) device_type: String,
    pub(crate) output: (u32, u32),
    pub(crate) format: String,
    pub(crate) view: (f32, f32),
    pub(crate) pixels_per_unit: f32,
    pub(crate) stats: FrameStats,
    pub(crate) timing_supported: bool,
}

pub(crate) struct Core {
    pub(crate) opts: ConsoleOptions,
    pub(crate) open: bool,
    pub(crate) panels_open: bool,
    /// The console turned GPU timing on for its panels.
    pub(crate) gpu_timing: bool,
    pub(crate) just_toggled: bool,
    pub(crate) line: String,
    pub(crate) caret_end: bool,
    pub(crate) history: History,
    pub(crate) scroll: usize,
    pub(crate) commands: BTreeMap<String, Command>,
    pub(crate) vars: BTreeMap<String, Var>,
    pub(crate) binds: BTreeMap<Key, String>,
    pub(crate) capture: LogCapture,
    pub(crate) requests: Vec<Request>,
    pub(crate) frames: VecDeque<FrameTimes>,
    pub(crate) gpu: GpuInfo,
    pub(crate) textures: Vec<(String, Texture)>,
    pub(crate) infos: Vec<(String, Box<dyn FnMut() -> String>)>,
    pub(crate) user_panels: Vec<(String, PanelBody)>,
    pub(crate) tab: usize,
    pub(crate) panel_rect: Rect,
    pub(crate) var_edits: BTreeMap<String, String>,
    pub(crate) pending_error: Option<String>,
    pub(crate) sim: crate::sim::Sim,
}

/// The in-game debug console: a drop-down command line, a window of
/// panels showing what the engine is doing, commands, typed variables,
/// key bindings and the captured log.
///
/// The engine builds one when `Config::console` is set and draws it after
/// everything else; get it from `ctx.console`. A disabled console accepts
/// every call and does nothing visible.
pub struct Console {
    pub(crate) enabled: bool,
    pub(crate) ui: Ui,
    pub(crate) core: Core,
}

impl std::fmt::Debug for Console {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Console")
            .field("enabled", &self.enabled)
            .field("open", &self.core.open)
            .finish_non_exhaustive()
    }
}

impl Console {
    /// An enabled console with `options`. It captures log records through
    /// the logger [`install_logger`](crate::install_logger) set up when
    /// there is one, and keeps its own output otherwise.
    pub fn new(options: ConsoleOptions) -> Console {
        let capture = LogCapture::global().unwrap_or_else(|| LogCapture::new(options.lines));
        let mut theme = Theme::dark();
        theme.panel = theme.panel.with_alpha(0.94);
        theme.text_size = 14.0;
        theme.mono_size = 13.0;
        theme.row_height = 24.0;
        let mut c = Console {
            enabled: true,
            ui: Ui::new(theme),
            core: Core {
                opts: options,
                open: false,
                panels_open: false,
                gpu_timing: false,
                just_toggled: false,
                line: String::new(),
                caret_end: false,
                history: History::default(),
                scroll: 0,
                commands: BTreeMap::new(),
                vars: BTreeMap::new(),
                binds: BTreeMap::new(),
                capture,
                requests: Vec::new(),
                frames: VecDeque::new(),
                gpu: GpuInfo::default(),
                textures: Vec::new(),
                infos: Vec::new(),
                user_panels: Vec::new(),
                tab: 0,
                panel_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
                var_edits: BTreeMap::new(),
                pending_error: None,
                sim: crate::sim::Sim::default(),
            },
        };
        c.core.print("yowie console: type help for commands");
        c
    }

    /// A console that draws nothing and takes no input. Registration and
    /// printing still work, so a game's setup code needs no conditions.
    pub fn disabled() -> Console {
        let mut c = Console::new(ConsoleOptions::default());
        c.enabled = false;
        c.core.capture.clear();
        c
    }

    /// Whether the console draws and reads input.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether the drop-down is open. While it is, it has the keyboard:
    /// the game should leave keys alone. It describes the drop-down only,
    /// not the panels.
    pub fn is_open(&self) -> bool {
        self.enabled && self.core.open
    }

    /// Whether the panels window is shown.
    pub fn panels_open(&self) -> bool {
        self.enabled && self.core.panels_open
    }

    /// Opens or closes the drop-down.
    pub fn set_open(&mut self, open: bool) {
        self.core.open = open && self.enabled;
    }

    /// Shows or hides the panels window.
    pub fn set_panels_open(&mut self, open: bool) {
        self.core.panels_open = open && self.enabled;
    }

    /// Replaces the console's theme.
    pub fn set_theme(&mut self, theme: Theme) {
        self.ui.set_theme(theme);
    }

    /// Sets what cut, copy and paste in the command line use.
    pub fn set_clipboard(&mut self, clipboard: Box<dyn yowie_ui::Clipboard>) {
        self.ui.set_clipboard(clipboard);
    }

    /// Whether the command line has the keyboard focus, for the engine to
    /// turn on input-method composition.
    pub fn wants_keyboard(&self) -> bool {
        self.is_open() || self.ui.wants_keyboard()
    }

    /// Where the command line's caret is, in view units, when it has focus.
    pub fn text_input_area(&self) -> Option<Rect> {
        self.ui.text_input_area()
    }

    /// Whether the pointer is over the console or its panels.
    pub fn wants_mouse(&self) -> bool {
        self.enabled && (self.core.open || self.ui.wants_mouse())
    }

    // ---- output ----

    /// Prints a line of text in the output.
    pub fn print(&mut self, text: &str) {
        self.core.print(text);
    }

    /// Prints a line in red.
    pub fn print_error(&mut self, text: &str) {
        self.core.print_error(text);
    }

    /// The most recent output and log lines, oldest first.
    pub fn lines(&self, count: usize) -> Vec<LogLine> {
        self.core.capture.window(count, 0)
    }

    // ---- commands ----

    /// Registers a command. `help` is a one-line description shown by
    /// `help`. The handler gets the words after the command's name; what it
    /// returns is printed, an error in red. Registering a name again
    /// replaces the command, built-ins included.
    pub fn register(
        &mut self,
        name: &str,
        help: &str,
        handler: impl FnMut(&[String]) -> CommandResult + 'static,
    ) {
        self.core.commands.insert(
            name.to_owned(),
            Command {
                help: help.to_owned(),
                run: Box::new(handler),
                complete: None,
            },
        );
    }

    /// Like [`register`](Self::register), with a completer. It gets the
    /// words typed after the command, the last being the partial word, and
    /// returns the names Tab may complete it to.
    pub fn register_completing(
        &mut self,
        name: &str,
        help: &str,
        handler: impl FnMut(&[String]) -> CommandResult + 'static,
        completer: impl Fn(&[String]) -> Vec<String> + 'static,
    ) {
        self.core.commands.insert(
            name.to_owned(),
            Command {
                help: help.to_owned(),
                run: Box::new(handler),
                complete: Some(Box::new(completer)),
            },
        );
    }

    /// Runs a command line as if it were typed, printing its output.
    pub fn execute(&mut self, line: &str) {
        self.core.execute(line);
    }

    // ---- variables ----

    /// Registers a float variable and returns the handle the game reads.
    /// `set name 2.5` changes it. Registering the name again replaces it.
    pub fn float(&mut self, name: &str, default: f32, help: &str) -> FloatVar {
        let (v, h) = Var::float(default, help);
        self.core.vars.insert(name.to_owned(), v);
        h
    }

    /// Registers a whole-number variable and returns its handle.
    pub fn int(&mut self, name: &str, default: i64, help: &str) -> IntVar {
        let (v, h) = Var::int(default, help);
        self.core.vars.insert(name.to_owned(), v);
        h
    }

    /// Registers a boolean variable (`true`, `false`, `on`, `off`, `1`,
    /// `0`) and returns its handle.
    pub fn boolean(&mut self, name: &str, default: bool, help: &str) -> BoolVar {
        let (v, h) = Var::boolean(default, help);
        self.core.vars.insert(name.to_owned(), v);
        h
    }

    /// Registers a string variable and returns its handle.
    pub fn string(&mut self, name: &str, default: &str, help: &str) -> StringVar {
        let (v, h) = Var::text(default, help);
        self.core.vars.insert(name.to_owned(), v);
        h
    }

    // ---- key bindings ----

    /// Runs `command` whenever `key` is pressed while the console is shut.
    pub fn bind(&mut self, key: Key, command: &str) {
        self.core.binds.insert(key, command.to_owned());
    }

    /// Removes a binding. Returns whether there was one.
    pub fn unbind(&mut self, key: Key) -> bool {
        self.core.binds.remove(&key).is_some()
    }

    // ---- panels and services ----

    /// Adds a line to the Services panel, redrawn from `info` every frame.
    pub fn add_info(&mut self, name: &str, info: impl FnMut() -> String + 'static) {
        self.core.infos.push((name.to_owned(), Box::new(info)));
    }

    /// Adds a panel tab. `body` runs every frame the tab is shown and adds
    /// widgets to the frame it is given.
    pub fn add_panel(&mut self, name: &str, body: impl FnMut(&mut Frame<'_>) + 'static) {
        self.core
            .user_panels
            .push((name.to_owned(), Box::new(body)));
    }

    /// Lists a texture in the GPU panel with its size and an estimate of
    /// its memory. Call it again with the same name to replace the entry.
    pub fn watch_texture(&mut self, name: &str, texture: &Texture) {
        let list = &mut self.core.textures;
        match list.iter_mut().find(|(n, _)| n == name) {
            Some(e) => e.1 = texture.clone(),
            None => list.push((name.to_owned(), texture.clone())),
        }
    }

    /// Stops listing a texture.
    pub fn unwatch_texture(&mut self, name: &str) {
        self.core.textures.retain(|(n, _)| n != name);
    }

    // ---- per frame ----

    /// Records one frame's timings for the Engine panel and the `fps` and
    /// `stats` commands. The engine calls it every frame.
    pub fn record_frame(&mut self, times: FrameTimes) {
        let f = &mut self.core.frames;
        if f.len() >= 240 {
            f.pop_front();
        }
        f.push_back(times);
    }

    /// What the console asks the engine to do since the last call.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.core.requests)
    }

    /// Reads the toggle keys and bindings, then draws the console on
    /// [`layers::CONSOLE`]. `dt` is the seconds since the last frame. Call
    /// it last in the frame, after the game's own interface.
    pub fn draw(&mut self, gfx: &mut Graphics, input: &InputState, dt: f32) {
        if !self.enabled {
            return;
        }
        self.ui.set_delta_time(dt);
        self.core.refresh_gpu(gfx);
        self.core.handle_keys(input);
        // GPU timing runs while the panels are open, for the GPU panel.
        if self.core.panels_open != self.core.gpu_timing {
            self.core.gpu_timing = self.core.panels_open;
            gfx.set_gpu_timing(self.core.gpu_timing);
        }
        if !self.core.open && !self.core.panels_open {
            return;
        }
        let Console { ui, core, .. } = self;
        ui.draw_on(gfx, layers::CONSOLE, input, |f| core.build(f, input));
    }
}

impl Core {
    pub(crate) fn print(&mut self, text: &str) {
        for line in text.lines() {
            self.capture.push(LogLine {
                level: log::Level::Info,
                target: "console".into(),
                text: line.to_owned(),
            });
        }
        self.scroll = 0;
    }

    pub(crate) fn print_error(&mut self, text: &str) {
        for line in text.lines() {
            self.capture.push(LogLine {
                level: log::Level::Error,
                target: "console".into(),
                text: line.to_owned(),
            });
        }
        self.scroll = 0;
    }

    fn refresh_gpu(&mut self, gfx: &Graphics) {
        let info = gfx.adapter_info();
        let size = gfx.output_size();
        let view = gfx.view_size();
        self.gpu = GpuInfo {
            adapter: info.name.clone(),
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            output: (size.x, size.y),
            format: format!("{:?}", gfx.output_format()),
            view: (view.x, view.y),
            pixels_per_unit: gfx.pixels_per_unit(),
            stats: gfx.stats(),
            timing_supported: gfx.gpu_timing_supported(),
        };
    }

    pub(crate) fn default_panel_rect(view: Rect) -> Rect {
        let w = 440.0_f32.min(view.w - 20.0).max(200.0);
        let h = 480.0_f32.min(view.h - 60.0).max(200.0);
        Rect::new((view.w - w - 12.0).max(0.0), 44.0, w, h)
    }
}
