//! The renderer's error type.

/// Errors from creating the renderer, its resources and its frames.
#[derive(Debug, thiserror::Error)]
pub enum GfxError {
    /// No GPU adapter matched the request; the machine has no usable
    /// Vulkan, Metal or DX12 driver.
    #[error("no GPU adapter: {0}")]
    NoAdapter(String),
    /// The adapter refused to create a device.
    #[error("cannot create GPU device: {0}")]
    RequestDevice(String),
    /// The window system refused to create a drawing surface.
    #[error("cannot create window surface: {0}")]
    CreateSurface(String),
    /// The adapter cannot present to the window's surface.
    #[error("the GPU adapter cannot present to this window")]
    SurfaceUnsupported,
    /// A game shader failed to compile or link; the message is naga's,
    /// with line numbers counted in the game's own source.
    #[error("shader error:\n{0}")]
    Shader(String),
    /// Encoded image bytes could not be decoded.
    #[error("cannot decode image: {0}")]
    Image(#[from] image::ImageError),
    /// Pixel data did not match the stated size.
    #[error("pixel data is {got} bytes, expected {expected} for the given size")]
    PixelDataSize {
        /// The byte count the size requires.
        expected: usize,
        /// The byte count supplied.
        got: usize,
    },
    /// A width or height was zero or larger than the device allows.
    #[error("invalid size {width}x{height} (the device allows up to {max})")]
    InvalidSize {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
        /// The device's largest texture dimension.
        max: u32,
    },
    /// A region lay outside the texture.
    #[error("region {x},{y} {width}x{height} lies outside the {tex_width}x{tex_height} texture")]
    RegionOutOfBounds {
        /// Region left edge in texels.
        x: u32,
        /// Region top edge in texels.
        y: u32,
        /// Region width in texels.
        width: u32,
        /// Region height in texels.
        height: u32,
        /// Texture width in texels.
        tex_width: u32,
        /// Texture height in texels.
        tex_height: u32,
    },
    /// A uniform value's size differed from the size the shader was
    /// created with.
    #[error("uniform block is {got} bytes, the shader was created with {expected}")]
    UniformSize {
        /// The size the shader expects.
        expected: usize,
        /// The size supplied.
        got: usize,
    },
    /// Reading pixels back from the GPU failed.
    #[error("cannot read pixels back: {0}")]
    Readback(String),
    /// Images did not fit an atlas of the largest allowed size.
    #[error("atlas: {0}")]
    Atlas(String),
    /// An Aseprite file could not be read or used.
    #[error("aseprite: {0}")]
    Aseprite(String),
    /// A Tiled map could not be turned into a drawable level.
    #[error("tiled level: {0}")]
    TiledLevel(String),
    /// Font bytes held no font face.
    #[error("font data holds no usable face")]
    NoFontFace,
    /// A KTX2 file could not be read.
    #[error("cannot read KTX2 file: {0}")]
    Ktx2(#[from] yowie_ktx2::Error),
    /// The image or texture format is not supported by this GPU or call.
    #[error("unsupported texture format: {0}")]
    UnsupportedFormat(String),
    /// The call is not allowed while a frame is being recorded, or needs
    /// one.
    #[error("{0}")]
    FrameState(&'static str),
}

/// Results from the renderer.
pub type Result<T, E = GfxError> = std::result::Result<T, E>;
