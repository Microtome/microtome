//! Error types for the microtome-core library.

/// All errors that can occur within the microtome slicing engine.
#[derive(Debug, thiserror::Error)]
pub enum MicrotomeError {
    /// GPU initialization or device creation failed.
    #[error("GPU initialization failed: {0}")]
    GpuInit(String),

    /// STL file parsing failed.
    #[error("STL parsing failed: {0}")]
    StlParse(String),

    /// Error during the GPU slicing pipeline.
    #[error("Slicing error: {0}")]
    Slicing(String),

    /// Filesystem or general I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// ZIP archive creation or writing failed.
    #[error("ZIP error: {0}")]
    Zip(String),

    /// Image encoding (PNG) failed.
    #[error("Image error: {0}")]
    Image(String),

    /// The slicing job was cancelled by the user.
    #[error("Operation cancelled")]
    Cancelled,
}

/// Convenience type alias for Results using [`MicrotomeError`].
pub type Result<T> = std::result::Result<T, MicrotomeError>;
