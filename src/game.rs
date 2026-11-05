//! The callbacks a game implements, and the events it can observe.

use std::path::PathBuf;

use yowie_input::InputEvent;

use crate::{Context, GameResult};

/// Something that happened to the window, delivered to [`Game::event`]
/// as it arrives, before the next update.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Keyboard, pointer, text or gamepad input, already applied to
    /// [`Context::input`](crate::Context::input).
    Input(InputEvent),
    /// The window's drawable area changed size or density.
    Resized {
        /// New width in pixels.
        width: u32,
        /// New height in pixels.
        height: u32,
        /// Pixels per logical point.
        scale_factor: f32,
    },
    /// The window gained (true) or lost (false) keyboard focus.
    Focused(bool),
    /// The window became hidden (true) or visible again (false). A hidden
    /// window does not draw.
    Occluded(bool),
    /// A file is being dragged over the window. One event arrives for each
    /// file in the drag.
    FileHovered(PathBuf),
    /// The drag that sent [`Event::FileHovered`] left the window without
    /// dropping.
    FileHoverCancelled,
    /// A file was dropped on the window. One event arrives for each file.
    FileDropped(PathBuf),
    /// The player asked to close the window. Without
    /// [`Config::handle_close`](crate::Config::handle_close) the loop ends
    /// after this event.
    CloseRequested,
}

/// A game: built once by [`init`](Game::init), then updated at the loop's
/// rate and drawn once per frame.
///
/// Every callback returns a [`GameResult`]; returning an error ends
/// [`run`](crate::run), which returns it.
///
/// ```no_run
/// use yowie::{Config, Context, Game, GameResult};
///
/// struct Hello;
///
/// impl Game for Hello {
///     fn init(_ctx: &mut Context) -> GameResult<Self> {
///         Ok(Hello)
///     }
///
///     fn draw(&mut self, ctx: &mut Context) -> GameResult {
///         let font = ctx.gfx.default_font();
///         let style = yowie::gfx::TextStyle::new(32.0);
///         ctx.gfx.draw_text(&font, "Hello", yowie::vec2(20.0, 20.0), &style);
///         Ok(())
///     }
/// }
///
/// fn main() -> Result<(), yowie::Error> {
///     yowie::run::<Hello>(Config::new("Hello"))
/// }
/// ```
pub trait Game: Sized + 'static {
    /// Builds the game once the window and renderer exist. Load textures,
    /// fonts and shaders here.
    fn init(ctx: &mut Context) -> GameResult<Self>;

    /// Advances the game by one update of `ctx.time.delta()` seconds. Input
    /// edges cover the time since the previous update.
    fn update(&mut self, ctx: &mut Context) -> GameResult {
        let _ = ctx;
        Ok(())
    }

    /// Queues the frame's drawing. Input edges cover the whole frame.
    fn draw(&mut self, ctx: &mut Context) -> GameResult;

    /// Observes a window or input event as it arrives.
    fn event(&mut self, ctx: &mut Context, event: &Event) -> GameResult {
        let _ = (ctx, event);
        Ok(())
    }
}
