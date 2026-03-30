/// Microtome core slicing engine library.
///
/// Provides GPU-accelerated slicing of 3D meshes for DLP-style 3D printers.
pub mod config;
pub mod error;
pub mod units;

pub use config::{PrintJobConfig, PrintVolume, PrinterConfig, Projector, Resin, ZStage};
pub use error::{MicrotomeError, Result};
pub use units::{LengthUnit, convert_length};
