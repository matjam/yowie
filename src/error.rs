//! The engine's error type and the result type game callbacks return.

/// Any error a game callback returns. `?` converts every error type that
/// implements `std::error::Error + Send + Sync` into it, and so does
/// `.into()` on a `String` or `&str`.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// What game callbacks return: `Ok` to carry on, or any error to stop the
/// loop and have [`run`](crate::run) return it.
pub type GameResult<T = ()> = std::result::Result<T, BoxError>;

/// Errors that end [`run`](crate::run).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The renderer failed.
    #[error("graphics: {0}")]
    Gfx(#[from] yowie_gfx::GfxError),
    /// The window system's event loop failed.
    #[error("event loop: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    /// The window could not be created.
    #[error("cannot create window: {0}")]
    CreateWindow(String),
    /// A game callback returned an error.
    #[error("game: {0}")]
    Game(BoxError),
    /// A window control (cursor grab, clipboard) is not available.
    #[error("window: {0}")]
    Window(String),
    /// A command-line argument the engine reads had a bad value.
    #[error("bad argument: {0}")]
    Argument(String),
    /// The `--shot` image could not be written.
    #[error("cannot write screenshot {path}: {source}")]
    Screenshot {
        /// The file that was being written.
        path: String,
        /// Why writing failed.
        source: image::ImageError,
    },
}

/// Results from the engine.
pub type Result<T, E = Error> = std::result::Result<T, E>;
