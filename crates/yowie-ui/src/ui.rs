//! The retained half of the toolkit: what survives from one frame to the
//! next (focus, scroll offsets, editors, window rectangles, open popups).

use std::any::Any;
use std::collections::HashMap;

use yowie_core::{Rect, Vec2};
use yowie_gfx::{Font, Graphics, layers};
use yowie_input::InputState;

use crate::access::AccessNode;
use crate::clipboard::{Clipboard, MemoryClipboard};
use crate::edit::TextEdit;
use crate::frame::Frame;
use crate::id::WidgetId;
use crate::painter::{GfxPainter, Painter};
use crate::theme::Theme;

/// How many frames a widget's retained state outlives its last use.
const KEEP_FRAMES: u64 = 600;

/// A map of per-widget state that forgets widgets not used for a while.
#[derive(Debug)]
pub(crate) struct Store<T> {
    map: HashMap<WidgetId, (u64, T)>,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl<T: Default> Store<T> {
    /// The state of `id`, created on first use and stamped as used in `frame`.
    pub(crate) fn get(&mut self, id: WidgetId, frame: u64) -> &mut T {
        let e = self.map.entry(id).or_insert_with(|| (frame, T::default()));
        e.0 = frame;
        &mut e.1
    }

    pub(crate) fn sweep(&mut self, frame: u64) {
        self.map.retain(|_, e| e.0 + KEEP_FRAMES >= frame);
    }
}

/// A scrolling container's offset and the sizes measured last frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScrollState {
    pub(crate) offset: Vec2,
    pub(crate) content: Vec2,
    pub(crate) viewport: Vec2,
    /// Where the pointer grabbed a scrollbar thumb, along its axis.
    pub(crate) grab: Option<(usize, f32)>,
}

/// A window's rectangle, kept when the game does not hold one.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DragRect {
    pub(crate) grab: Vec2,
}

/// A list's or tab bar's cursor and typed-search state.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ListState {
    pub(crate) cursor: usize,
    /// The popup's size measured last frame.
    pub(crate) size: Vec2,
    /// How many items the popup held last frame.
    pub(crate) count: usize,
}

/// Wrapped line spans kept until the text or width changes.
#[derive(Clone, Debug, Default)]
pub(crate) struct TextCache {
    pub(crate) key: u64,
    pub(crate) width: f32,
    pub(crate) lines: Vec<crate::edit::LineSpan>,
}

/// A drag in progress.
pub(crate) struct DragPayload {
    pub(crate) source: WidgetId,
    pub(crate) payload: Box<dyn Any>,
    pub(crate) label: String,
}

/// A layer (window, popup, modal) as the last frame left it, for deciding
/// which one the pointer is over.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayerRec {
    pub(crate) id: WidgetId,
    pub(crate) z: f32,
    pub(crate) rect: Rect,
}

/// A widget the keyboard can reach.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Focusable {
    pub(crate) id: WidgetId,
    pub(crate) rect: Rect,
    pub(crate) layer: WidgetId,
    pub(crate) scroll: WidgetId,
}

/// Keys the focused widget takes for itself instead of letting navigation
/// have them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Wants {
    pub(crate) horizontal: bool,
    pub(crate) vertical: bool,
    pub(crate) text: bool,
}

/// An interface: retained state shared by every frame built from it.
///
/// Create one, keep it, and build each frame with [`Ui::frame`] or
/// [`Ui::draw`]. The state it keeps is keyed by [`WidgetId`], which is
/// derived from labels and the containers around them, so it survives
/// frames in which a widget is not drawn and is dropped after a long
/// absence.
pub struct Ui {
    pub(crate) theme: Theme,
    pub(crate) frame_no: u64,
    pub(crate) time: f32,
    pub(crate) dt: f32,
    pub(crate) active: WidgetId,
    pub(crate) focus: WidgetId,
    pub(crate) nav_visible: bool,
    pub(crate) wants: Wants,
    pub(crate) scroll_to_focus: bool,
    pub(crate) focusables_prev: Vec<Focusable>,
    pub(crate) focusables: Vec<Focusable>,
    pub(crate) layers_prev: Vec<LayerRec>,
    pub(crate) layers: Vec<LayerRec>,
    pub(crate) window_order: Vec<WidgetId>,
    pub(crate) windows: Store<Rect>,
    pub(crate) window_drags: Store<DragRect>,
    pub(crate) scrolls: Store<ScrollState>,
    pub(crate) edits: Store<TextEdit>,
    pub(crate) lists: Store<ListState>,
    pub(crate) text_cache: Store<TextCache>,
    pub(crate) expanded: HashMap<WidgetId, bool>,
    pub(crate) popup: WidgetId,
    pub(crate) menu_path: Vec<WidgetId>,
    pub(crate) context_menu: Option<(WidgetId, Vec2)>,
    pub(crate) modal_prev: Option<LayerRec>,
    pub(crate) modal: Option<LayerRec>,
    pub(crate) drag: Option<DragPayload>,
    pub(crate) press_origin: Vec2,
    pub(crate) hover_id: WidgetId,
    pub(crate) hover_time: f32,
    pub(crate) hover_seen: bool,
    pub(crate) mouse_prev: Vec2,
    pub(crate) wants_mouse: bool,
    pub(crate) text_area: Option<Rect>,
    pub(crate) text_focus: bool,
    pub(crate) nodes_prev: Vec<AccessNode>,
    pub(crate) clipboard: Box<dyn Clipboard>,
    pub(crate) fonts: Option<(Font, Font)>,
}

impl std::fmt::Debug for Ui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ui")
            .field("frame", &self.frame_no)
            .field("focus", &self.focus)
            .finish_non_exhaustive()
    }
}

impl Default for Ui {
    fn default() -> Self {
        Ui::new(Theme::default())
    }
}

impl Ui {
    /// An interface with `theme` and an in-memory clipboard.
    pub fn new(theme: Theme) -> Ui {
        Ui {
            theme,
            frame_no: 0,
            time: 0.0,
            dt: 1.0 / 60.0,
            active: WidgetId::NONE,
            focus: WidgetId::NONE,
            nav_visible: false,
            wants: Wants::default(),
            scroll_to_focus: false,
            focusables_prev: Vec::new(),
            focusables: Vec::new(),
            layers_prev: Vec::new(),
            layers: Vec::new(),
            window_order: Vec::new(),
            windows: Store::default(),
            window_drags: Store::default(),
            scrolls: Store::default(),
            edits: Store::default(),
            lists: Store::default(),
            text_cache: Store::default(),
            expanded: HashMap::new(),
            popup: WidgetId::NONE,
            menu_path: Vec::new(),
            context_menu: None,
            modal_prev: None,
            modal: None,
            drag: None,
            press_origin: Vec2::ZERO,
            hover_id: WidgetId::NONE,
            hover_time: 0.0,
            hover_seen: false,
            mouse_prev: Vec2::ZERO,
            wants_mouse: false,
            text_area: None,
            text_focus: false,
            nodes_prev: Vec::new(),
            clipboard: Box::new(MemoryClipboard::default()),
            fonts: None,
        }
    }

    /// The current theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replaces the theme; the next frame draws with it.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Sets the proportional and monospaced fonts [`draw`](Self::draw) uses
    /// instead of the renderer's bundled ones. Fonts belong to the
    /// renderer that loaded them.
    pub fn set_fonts(&mut self, regular: Font, mono: Font) {
        self.fonts = Some((regular, mono));
    }

    /// Sets what cut, copy and paste in text fields use.
    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.clipboard = clipboard;
    }

    /// Sets the seconds the next frame advances interface time by, which
    /// drives tooltip delays and caret blinking.
    pub fn set_delta_time(&mut self, seconds: f32) {
        self.dt = seconds.clamp(0.0, 0.25);
    }

    /// Builds one frame of interface on `painter` from `input`, calling
    /// `body` with the [`Frame`] to add widgets to. The frame is finished
    /// (popups settled, retained state updated) when `body` returns.
    pub fn frame<R>(
        &mut self,
        painter: &mut dyn Painter,
        input: &InputState,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        let mut f = Frame::begin(self, painter, input);
        let r = body(&mut f);
        f.finish();
        r
    }

    /// Builds one frame drawing through `gfx` on [`layers::UI`].
    pub fn draw<R>(
        &mut self,
        gfx: &mut Graphics,
        input: &InputState,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        self.draw_on(gfx, layers::UI, input, body)
    }

    /// Builds one frame drawing through `gfx` on `layer`.
    pub fn draw_on<R>(
        &mut self,
        gfx: &mut Graphics,
        layer: i32,
        input: &InputState,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        let mut painter = match &self.fonts {
            Some((regular, mono)) => GfxPainter::with_fonts(gfx, layer, regular.clone(), mono.clone()),
            None => GfxPainter::new(gfx, layer),
        };
        self.frame(&mut painter, input, body)
    }

    /// Whether the pointer is over a window, panel or popup, or a drag is
    /// under way, as of the last frame. Games use it to ignore clicks the
    /// interface took. It does not consume input.
    pub fn wants_mouse(&self) -> bool {
        self.wants_mouse
    }

    /// Whether a text editor has keyboard focus, as of the last frame.
    /// Games use it to leave typed keys alone.
    pub fn wants_keyboard(&self) -> bool {
        self.text_focus
    }

    /// Where the focused text editor is in view units, when one has focus.
    /// The engine passes it to the input method so candidate windows appear
    /// beside the caret.
    pub fn text_input_area(&self) -> Option<Rect> {
        self.text_area
    }

    /// The widget with keyboard focus, if any.
    pub fn focused(&self) -> Option<WidgetId> {
        (self.focus != WidgetId::NONE).then_some(self.focus)
    }

    /// Gives keyboard focus to `id` (the id a widget's
    /// [`Response`](crate::Response) reports).
    pub fn set_focus(&mut self, id: WidgetId) {
        self.focus = id;
    }

    /// Drops keyboard focus.
    pub fn clear_focus(&mut self) {
        self.focus = WidgetId::NONE;
    }

    /// The last completed frame's widgets as a tree of roles, names and
    /// values, in the order they were added.
    pub fn accessibility(&self) -> &[AccessNode] {
        &self.nodes_prev
    }

    /// Seconds of interface time since the first frame.
    pub fn time(&self) -> f32 {
        self.time
    }
}
