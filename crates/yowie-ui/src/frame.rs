//! One frame of interface: the widget-adding surface, identity, layers,
//! clipping and the pointer and keyboard interaction every widget shares.

use std::collections::HashMap;

use yowie_core::{Rect, Vec2, vec2};
use yowie_input::{GamepadButton, InputState, Key, Modifiers, MouseButton};

use crate::access::{AccessNode, AccessState, Role};
use crate::id::{WidgetId, visible_label};
use crate::layout::Layout;
use crate::painter::Painter;
use crate::response::Response;
use crate::ui::{Focusable, LayerRec, Ui};

/// Draw order inside a layer: backgrounds, then images, then text, then
/// marks that sit over text. Grouping by kind lets a layer's widgets batch
/// into a few draw calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pass {
    Fill = 0,
    Image = 1,
    Text = 2,
    Over = 3,
}

/// Base depths of the layer kinds.
pub(crate) mod depth {
    pub(crate) const PANEL: f32 = 1.0;
    pub(crate) const WINDOW: f32 = 10.0;
    pub(crate) const POPUP: f32 = 1000.0;
    pub(crate) const MODAL: f32 = 2000.0;
    pub(crate) const TOP: f32 = 3000.0;
}

/// A way to move keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Navigation input resolved at the start of a frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Nav {
    pub(crate) activate: bool,
    pub(crate) cancel: bool,
}

/// What a widget reacts to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sense {
    pub(crate) click: bool,
    pub(crate) focus: bool,
}

impl Sense {
    pub(crate) const CLICK: Sense = Sense {
        click: true,
        focus: true,
    };
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LayerCtx {
    pub(crate) id: WidgetId,
    pub(crate) z: f32,
}

/// The surface widgets are added to during one frame. Containers take a
/// closure that receives the same frame, so nesting reads as the interface
/// looks:
///
/// ```ignore
/// ui.frame(&mut painter, &input, |f| {
///     f.panel(Some("Menu"), Rect::new(10.0, 10.0, 200.0, 300.0), |f| {
///         if f.button("Play").clicked { start(); }
///     });
/// });
/// ```
pub struct Frame<'a> {
    pub(crate) ui: &'a mut Ui,
    pub(crate) p: &'a mut dyn Painter,
    pub(crate) input: &'a InputState,
    pub(crate) view: Rect,
    pub(crate) mouse: Vec2,
    pub(crate) pressed: bool,
    pub(crate) down: bool,
    pub(crate) released: bool,
    pub(crate) right_released: bool,
    pub(crate) double: bool,
    pub(crate) wheel: Vec2,
    pub(crate) mods: Modifiers,
    pub(crate) nav: Nav,
    pub(crate) layouts: Vec<Layout>,
    pub(crate) ids: Vec<WidgetId>,
    pub(crate) seq: HashMap<WidgetId, u32>,
    pub(crate) clips: Vec<Rect>,
    pub(crate) layer_stack: Vec<LayerCtx>,
    pub(crate) hover_layer: WidgetId,
    pub(crate) press_taken: bool,
    pub(crate) disabled: u32,
    pub(crate) cur_depth: f32,
    pub(crate) nodes: Vec<AccessNode>,
    pub(crate) node_stack: Vec<usize>,
    pub(crate) scroll_stack: Vec<WidgetId>,
    pub(crate) text_seen: Option<Rect>,
    pub(crate) tooltip: Option<(String, Vec2)>,
    pub(crate) menus: Vec<MenuFrame>,
    pub(crate) activate_menu: bool,
}

/// A menu popup being built: which item is next and which is highlighted.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MenuFrame {
    pub(crate) index: usize,
    pub(crate) cursor: usize,
}

impl<'a> Frame<'a> {
    pub(crate) fn begin(
        ui: &'a mut Ui,
        p: &'a mut dyn Painter,
        input: &'a InputState,
    ) -> Frame<'a> {
        ui.frame_no += 1;
        ui.time += ui.dt;
        let view_size = p.view_size();
        let view = Rect::new(0.0, 0.0, view_size.x, view_size.y);
        let mouse = if input.mouse_inside() {
            input.mouse()
        } else {
            vec2(-1.0e6, -1.0e6)
        };
        ui.layers_prev = std::mem::take(&mut ui.layers);
        ui.focusables_prev = std::mem::take(&mut ui.focusables);
        ui.modal_prev = ui.modal.take();
        ui.hover_seen = false;
        if ui.hover_id != WidgetId::NONE {
            ui.hover_time += ui.dt;
        }
        let spacing = ui.theme.spacing;
        let mut f = Frame {
            view,
            mouse,
            pressed: input.mouse_pressed(MouseButton::Left),
            down: input.mouse_down(MouseButton::Left),
            released: input.mouse_released(MouseButton::Left),
            right_released: input.mouse_released(MouseButton::Right),
            double: input.mouse_double_clicked(MouseButton::Left),
            wheel: input.wheel(),
            mods: input.modifiers(),
            nav: Nav::default(),
            layouts: vec![Layout::column(Vec2::ZERO, view.w, spacing)],
            ids: vec![WidgetId(0x9E37_79B9_7F4A_7C15)],
            seq: HashMap::new(),
            clips: Vec::new(),
            layer_stack: vec![LayerCtx {
                id: WidgetId::NONE,
                z: 0.0,
            }],
            hover_layer: WidgetId::NONE,
            press_taken: false,
            disabled: 0,
            cur_depth: f32::NAN,
            nodes: Vec::new(),
            node_stack: Vec::new(),
            scroll_stack: Vec::new(),
            text_seen: None,
            tooltip: None,
            menus: Vec::new(),
            activate_menu: false,
            ui,
            p,
            input,
        };
        f.hover_layer = f.compute_hover_layer();
        if f.mouse != f.ui.mouse_prev || f.pressed {
            f.ui.nav_visible = false;
        }
        f.resolve_nav();
        if f.input.key_pressed(Key::Escape) && f.ui.drag.is_some() {
            f.ui.drag = None;
            f.ui.active = WidgetId::NONE;
        }
        f
    }

    /// Finishes the frame: draws deferred overlays and settles state.
    pub(crate) fn finish(&mut self) {
        self.draw_deferred();
        let ui = &mut *self.ui;
        if !self.down || self.released {
            ui.active = WidgetId::NONE;
        }
        if !self.down {
            ui.drag = None;
        }
        if !ui.hover_seen {
            ui.hover_id = WidgetId::NONE;
        }
        ui.modal_prev = None;
        ui.scroll_to_focus = false;
        ui.wants_mouse = ui.drag.is_some()
            || ui.layers.iter().any(|l| l.rect.contains(self.mouse))
            || ui.modal.is_some();
        ui.text_focus = self.text_seen.is_some();
        ui.text_area = self.text_seen;
        ui.mouse_prev = self.mouse;
        ui.nodes_prev = std::mem::take(&mut self.nodes);
        if ui.frame_no.is_multiple_of(300) {
            let n = ui.frame_no;
            ui.windows.sweep(n);
            ui.window_drags.sweep(n);
            ui.scrolls.sweep(n);
            ui.edits.sweep(n);
            ui.lists.sweep(n);
            ui.text_cache.sweep(n);
        }
    }

    fn compute_hover_layer(&self) -> WidgetId {
        let modal = self.ui.modal_prev;
        let floor = modal.map_or(f32::MIN, |m| m.z);
        let top = self
            .ui
            .layers_prev
            .iter()
            .filter(|l| l.z >= floor && l.rect.contains(self.mouse))
            .max_by(|a, b| a.z.total_cmp(&b.z));
        match (top, modal) {
            (Some(l), _) => l.id,
            (None, Some(m)) => m.id,
            (None, None) => WidgetId::NONE,
        }
    }

    /// The layer keyboard navigation is confined to: an open popup or the
    /// modal dialog.
    fn nav_scope(&self) -> Option<WidgetId> {
        if self.ui.popup != WidgetId::NONE {
            Some(self.ui.popup)
        } else if let Some(last) = self.ui.menu_path.last() {
            Some(*last)
        } else {
            self.ui.modal_prev.map(|m| m.id)
        }
    }

    fn key_step(&self, key: Key) -> bool {
        self.input.key_pressed(key) || self.input.key_repeated(key)
    }

    fn pad_pressed(&self, b: GamepadButton) -> bool {
        self.input.gamepads().any(|g| g.pressed(b))
    }

    fn resolve_nav(&mut self) {
        let wants = self.ui.wants;
        let shift = self.mods.contains(Modifiers::SHIFT);
        let mut tab = 0;
        if self.key_step(Key::Tab) {
            tab = if shift { -1 } else { 1 };
        }
        let mut dir = None;
        let horizontal = !wants.horizontal && !wants.text;
        let vertical = !wants.vertical && !wants.text;
        if vertical && (self.key_step(Key::Up) || self.pad_pressed(GamepadButton::DPadUp)) {
            dir = Some(Dir::Up);
        } else if vertical
            && (self.key_step(Key::Down) || self.pad_pressed(GamepadButton::DPadDown))
        {
            dir = Some(Dir::Down);
        } else if horizontal
            && (self.key_step(Key::Left) || self.pad_pressed(GamepadButton::DPadLeft))
        {
            dir = Some(Dir::Left);
        } else if horizontal
            && (self.key_step(Key::Right) || self.pad_pressed(GamepadButton::DPadRight))
        {
            dir = Some(Dir::Right);
        }
        let pad_dpad = self.pad_pressed(GamepadButton::DPadUp)
            || self.pad_pressed(GamepadButton::DPadDown)
            || self.pad_pressed(GamepadButton::DPadLeft)
            || self.pad_pressed(GamepadButton::DPadRight);
        let enter = self.input.key_pressed(Key::Enter) || self.input.key_pressed(Key::KeypadEnter);
        let space = self.input.key_pressed(Key::Space);
        self.nav.activate =
            (!wants.text && (enter || space)) || self.pad_pressed(GamepadButton::South);
        self.nav.cancel =
            self.input.key_pressed(Key::Escape) || self.pad_pressed(GamepadButton::East);
        if tab != 0 || dir.is_some() {
            self.ui.nav_visible = true;
            self.move_focus(tab, dir);
        } else if self.nav.activate || pad_dpad {
            self.ui.nav_visible = true;
        }
    }

    fn move_focus(&mut self, tab: i32, dir: Option<Dir>) {
        let scope = self.nav_scope();
        let list: Vec<Focusable> = self
            .ui
            .focusables_prev
            .iter()
            .copied()
            .filter(|f| scope.is_none_or(|s| f.layer == s))
            .collect();
        if list.is_empty() {
            return;
        }
        let cur = list.iter().position(|f| f.id == self.ui.focus);
        let next = match (cur, dir) {
            (None, _) if tab < 0 => Some(list.len() - 1),
            (None, _) => Some(0),
            (Some(i), None) if tab > 0 => Some((i + 1) % list.len()),
            (Some(i), None) => Some((i + list.len() - 1) % list.len()),
            (Some(i), Some(d)) => nearest(&list, i, d),
        };
        if let Some(i) = next {
            self.ui.focus = list[i].id;
            self.ui.scroll_to_focus = true;
        }
    }

    // ---- identity ----

    /// The id of the container being added to.
    pub fn current_id(&self) -> WidgetId {
        self.ids.last().copied().unwrap_or_default()
    }

    /// The id a widget labelled `label` gets here. Calling it twice with
    /// the same label in one container gives different ids.
    pub fn id(&mut self, label: &str) -> WidgetId {
        let base = self.current_id().child(label);
        let n = self.seq.entry(base).or_insert(0);
        let id = if *n == 0 { base } else { base.child(*n) };
        *n += 1;
        id
    }

    /// The id a widget labelled `label` gets here if it is the first with
    /// that label in the container, without using it up.
    pub fn peek_id(&self, label: &str) -> WidgetId {
        self.current_id().child(label)
    }

    /// Whether the widget `id` has keyboard focus.
    pub fn has_focus(&self, id: WidgetId) -> bool {
        self.ui.focus == id && id != WidgetId::NONE
    }

    /// Runs `body` with `key` added to the identity of every widget in it,
    /// so repeated labels (list rows) keep their ids when rows move.
    pub fn with_id<R>(
        &mut self,
        key: impl std::hash::Hash,
        body: impl FnOnce(&mut Frame<'_>) -> R,
    ) -> R {
        let id = self.current_id().child(key);
        self.ids.push(id);
        let r = body(self);
        self.ids.pop();
        r
    }

    // ---- layers and clipping ----

    pub(crate) fn cur_layer(&self) -> LayerCtx {
        self.layer_stack.last().copied().unwrap_or(LayerCtx {
            id: WidgetId::NONE,
            z: 0.0,
        })
    }

    pub(crate) fn push_layer(&mut self, id: WidgetId, z: f32) {
        self.layer_stack.push(LayerCtx { id, z });
        self.cur_depth = f32::NAN;
    }

    pub(crate) fn pop_layer(&mut self, rect: Rect) {
        if let Some(l) = self.layer_stack.pop() {
            self.ui.layers.push(LayerRec {
                id: l.id,
                z: l.z,
                rect,
            });
        }
        self.cur_depth = f32::NAN;
    }

    /// The clip currently applied: the innermost container's visible area.
    pub fn clip_rect(&self) -> Rect {
        self.clips.last().copied().unwrap_or(self.view)
    }

    pub(crate) fn push_clip(&mut self, r: Rect) {
        let r = match self.clips.last() {
            Some(c) => c.intersection(&r).unwrap_or(Rect::ZERO),
            None => r,
        };
        self.clips.push(r);
        self.p.set_clip(Some(r));
    }

    pub(crate) fn pop_clip(&mut self) {
        self.clips.pop();
        self.p.set_clip(self.clips.last().copied());
    }

    /// Runs `body` with clipping removed, for popups that may extend past
    /// their parent's clip.
    pub(crate) fn unclipped<R>(&mut self, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        let saved = std::mem::take(&mut self.clips);
        self.p.set_clip(None);
        let r = body(self);
        self.clips = saved;
        self.p.set_clip(self.clips.last().copied());
        r
    }

    pub(crate) fn pass(&mut self, pass: Pass) {
        let d = self.cur_layer().z + pass as u8 as f32 * 0.2;
        if d != self.cur_depth {
            self.cur_depth = d;
            self.p.set_depth(d);
        }
    }

    // ---- interaction ----

    /// Whether widgets are currently ignoring input.
    pub fn is_disabled(&self) -> bool {
        self.disabled > 0
    }

    /// Runs `body` with every widget in it drawn dimmed and ignoring input
    /// when `disabled` is true.
    pub fn disabled<R>(&mut self, disabled: bool, body: impl FnOnce(&mut Frame<'_>) -> R) -> R {
        self.disabled += u32::from(disabled);
        let r = body(self);
        self.disabled -= u32::from(disabled);
        r
    }

    pub(crate) fn pointer_over(&self, rect: Rect) -> bool {
        self.hover_layer == self.cur_layer().id
            && rect.contains(self.mouse)
            && self.clip_rect().contains(self.mouse)
    }

    /// Runs the pointer and keyboard state machine for a widget occupying
    /// `rect` and reports what happened.
    pub(crate) fn interact(&mut self, id: WidgetId, rect: Rect, sense: Sense) -> Response {
        let mut r = Response::blank(id, rect);
        if self.disabled > 0 {
            return r;
        }
        let over = self.pointer_over(rect);
        let layer = self.cur_layer().id;
        if sense.focus {
            let scroll = self.scroll_stack.last().copied().unwrap_or_default();
            self.ui.focusables.push(Focusable {
                id,
                rect,
                layer,
                scroll,
            });
            r.focused = self.ui.focus == id;
        }
        let dragging = self.ui.drag.is_some();
        r.hovered = over && !dragging && (self.ui.active == WidgetId::NONE || self.ui.active == id);
        if r.hovered {
            if self.ui.hover_id != id {
                self.ui.hover_id = id;
                self.ui.hover_time = 0.0;
            }
            self.ui.hover_seen = true;
        }
        if sense.click {
            if over && self.pressed && !self.press_taken && self.ui.active == WidgetId::NONE {
                self.press_taken = true;
                self.ui.active = id;
                self.ui.press_origin = self.mouse;
                if sense.focus {
                    self.ui.focus = id;
                    r.focused = true;
                }
            }
            if self.ui.active == id {
                r.held = self.down;
                if self.released {
                    r.clicked = over;
                    r.double_clicked = over && self.double;
                    self.ui.active = WidgetId::NONE;
                }
            }
            if over && self.right_released {
                r.right_clicked = true;
            }
            let keys_free = self.ui.popup == WidgetId::NONE && self.ui.menu_path.is_empty();
            if r.focused && self.nav.activate && sense.focus && keys_free {
                r.clicked = true;
            }
        }
        r
    }

    /// Says what keys the widget with focus handles itself, so navigation
    /// leaves them alone next frame.
    pub(crate) fn set_wants(&mut self, r: &Response, horizontal: bool, vertical: bool, text: bool) {
        if r.focused {
            self.ui.wants = crate::ui::Wants {
                horizontal,
                vertical,
                text,
            };
        }
    }

    pub(crate) fn key_repeat(&self, key: Key) -> bool {
        self.key_step(key)
    }

    // ---- layout access ----

    pub(crate) fn layout(&mut self) -> &mut Layout {
        if self.layouts.is_empty() {
            self.layouts.push(Layout::column(
                Vec2::ZERO,
                self.view.w,
                self.ui.theme.spacing,
            ));
        }
        let last = self.layouts.len() - 1;
        &mut self.layouts[last]
    }

    /// Reserves a rectangle in the current container. See
    /// [`Layout::allocate`].
    pub(crate) fn alloc(&mut self, natural_w: f32, h: f32, fill: bool) -> Rect {
        self.layout().allocate(natural_w, h, fill)
    }

    // ---- accessibility ----

    pub(crate) fn node(
        &mut self,
        id: WidgetId,
        role: Role,
        name: &str,
        value: Option<String>,
        mut state: AccessState,
        rect: Rect,
    ) -> usize {
        state.disabled = self.disabled > 0;
        state.focused = self.ui.focus == id && id != WidgetId::NONE;
        self.nodes.push(AccessNode {
            id,
            role,
            name: visible_label(name).to_owned(),
            value,
            state,
            rect,
            parent: self.node_stack.last().copied(),
        });
        self.nodes.len() - 1
    }

    pub(crate) fn push_node(&mut self, index: usize) {
        self.node_stack.push(index);
    }

    pub(crate) fn pop_node(&mut self) {
        self.node_stack.pop();
    }

    // ---- input queries for widgets ----

    /// The pointer position in view units.
    pub fn pointer(&self) -> Vec2 {
        self.mouse
    }

    /// The view's area in view units.
    pub fn view(&self) -> Rect {
        self.view
    }

    /// The theme in use.
    pub fn theme(&self) -> &crate::theme::Theme {
        &self.ui.theme
    }

    /// Whether `Escape` or a gamepad's east button was pressed this frame.
    pub fn cancel_pressed(&self) -> bool {
        self.nav.cancel
    }
}

/// The focusable nearest to `list[from]` in direction `dir`.
fn nearest(list: &[Focusable], from: usize, dir: Dir) -> Option<usize> {
    let c = list[from].rect.center();
    let mut best: Option<(usize, f32)> = None;
    for (i, f) in list.iter().enumerate() {
        if i == from {
            continue;
        }
        let d = f.rect.center() - c;
        let (primary, perp) = match dir {
            Dir::Up => (-d.y, d.x),
            Dir::Down => (d.y, d.x),
            Dir::Left => (-d.x, d.y),
            Dir::Right => (d.x, d.y),
        };
        if primary <= 0.5 {
            continue;
        }
        let score = primary + 2.0 * perp.abs();
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((i, score));
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Focusable;

    fn f(id: u64, x: f32, y: f32) -> Focusable {
        Focusable {
            id: WidgetId(id),
            rect: Rect::new(x, y, 40.0, 20.0),
            layer: WidgetId::NONE,
            scroll: WidgetId::NONE,
        }
    }

    #[test]
    fn directional_navigation_picks_the_aligned_neighbour() {
        let list = [
            f(1, 0.0, 0.0),
            f(2, 100.0, 0.0),
            f(3, 0.0, 40.0),
            f(4, 100.0, 40.0),
        ];
        assert_eq!(nearest(&list, 0, Dir::Right), Some(1));
        assert_eq!(nearest(&list, 0, Dir::Down), Some(2));
        assert_eq!(nearest(&list, 3, Dir::Up), Some(1));
        assert_eq!(nearest(&list, 3, Dir::Left), Some(2));
        assert_eq!(nearest(&list, 0, Dir::Up), None);
        assert_eq!(nearest(&list, 0, Dir::Left), None);
    }
}
