/// Microtome core slicing engine library.
///
/// Provides GPU-accelerated slicing of 3D meshes for DLP-style 3D printers.
pub mod config;
pub mod error;
pub mod gpu;
pub mod isosurface;
pub mod job;
pub mod mesh;
pub mod mesh_repair;
pub mod scene;
pub mod slicer;
pub mod units;

pub use config::{PrintJobConfig, PrintVolume, PrinterConfig, Projector, Resin, ZStage};
pub use error::{MicrotomeError, Result};
pub use gpu::GpuContext;
pub use job::{SliceProgress, SlicingJob, run_slicing_job};
pub use mesh::{BoundingBox, MeshData, MeshVertex, PrintMesh};
pub use scene::{PrintVolumeBox, PrinterScene};
pub use slicer::{AdvancedSlicer, SliceMeshBuffers};
pub use units::{LengthUnit, convert_length};
