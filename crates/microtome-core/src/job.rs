//! Batch slicing job that produces a ZIP archive of PNG slice images.
//!
//! Ported from the TypeScript `HeadlessToZipSlicerJob` in `src/lib/job.ts`.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use glam;
use serde::Serialize;
use wgpu::util::DeviceExt;

use crate::config::{PrintJobConfig, PrinterConfig};
use crate::error::{MicrotomeError, Result};
use crate::gpu::GpuContext;
use crate::scene::PrinterScene;
use crate::slicer::{AdvancedSlicer, SliceMeshBuffers};

/// Progress update from the slicing job.
pub enum SliceProgress {
    /// Progress fraction (0.0 to 1.0).
    Progress(f32),
    /// Job completed with ZIP bytes.
    Complete(Vec<u8>),
    /// Job failed with error.
    Failed(String),
}

/// Progress and cancellation interface for a slicing job.
pub struct SlicingJob {
    /// Channel sender for progress updates.
    pub progress_tx: mpsc::Sender<SliceProgress>,
    /// Shared flag that can be set to cancel the job.
    pub cancel_flag: Arc<AtomicBool>,
}

impl SlicingJob {
    /// Creates a new `SlicingJob` with the given progress channel and cancel flag.
    pub fn new(progress_tx: mpsc::Sender<SliceProgress>, cancel_flag: Arc<AtomicBool>) -> Self {
        Self {
            progress_tx,
            cancel_flag,
        }
    }

    /// Returns `true` if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::Relaxed)
    }

    /// Requests cancellation of the job.
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }
}

/// Summary metadata written into the ZIP as `slice-config.json`.
#[derive(Debug, Clone, Serialize)]
struct SliceConfig {
    /// Name of the printer used.
    printer_name: String,
    /// Total number of layers sliced.
    layer_count: u32,
    /// Layer height in millimeters.
    layer_height_mm: f64,
    /// Horizontal resolution of the projector in pixels.
    x_res_px: u32,
    /// Vertical resolution of the projector in pixels.
    y_res_px: u32,
}

/// Runs the full slicing pipeline, producing a ZIP archive of PNG slice images.
///
/// This function is meant to be called on a background thread.
/// It creates its own `GpuContext` for headless rendering.
///
/// Progress updates are sent via `progress_tx`. The `cancel_flag` is checked
/// between layers to allow early termination.
///
/// Returns ZIP bytes on success.
pub fn run_slicing_job(
    scene: &PrinterScene,
    printer_config: &PrinterConfig,
    job_config: &PrintJobConfig,
    progress_tx: mpsc::Sender<SliceProgress>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<Vec<u8>> {
    // Step 1: Create standalone GPU context
    let gpu = pollster::block_on(GpuContext::new_standalone())?;

    // Step 2: Create slicer at projector resolution
    let width = printer_config.projector.x_res_px;
    let height = printer_config.projector.y_res_px;
    let slicer = AdvancedSlicer::new(&gpu, width, height)?;

    // Step 3: Upload mesh data to GPU buffers
    let mesh_buffers: Vec<SliceMeshBuffers> = scene
        .meshes
        .iter()
        .map(|print_mesh| {
            // Bake the mesh transform into vertices since the slicer
            // shaders don't use a model matrix.
            let model = glam::Mat4::from_scale_rotation_translation(
                print_mesh.scale,
                glam::Quat::from_euler(
                    glam::EulerRot::XYZ,
                    print_mesh.rotation.x,
                    print_mesh.rotation.y,
                    print_mesh.rotation.z,
                ),
                print_mesh.position,
            );

            let transformed_vertices: Vec<crate::mesh::MeshVertex> = print_mesh
                .mesh_data
                .vertices
                .iter()
                .map(|v| {
                    let pos = glam::Vec3::from(v.position);
                    let norm = glam::Vec3::from(v.normal);
                    let new_pos = model.transform_point3(pos);
                    let new_norm = model.transform_vector3(norm).normalize_or_zero();
                    crate::mesh::MeshVertex {
                        position: new_pos.into(),
                        normal: new_norm.into(),
                    }
                })
                .collect();

            let vertex_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh-vertex-buffer"),
                    contents: bytemuck::cast_slice(&transformed_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

            let index_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh-index-buffer"),
                    contents: bytemuck::cast_slice(&print_mesh.mesh_data.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

            SliceMeshBuffers {
                vertex_buffer,
                index_buffer,
                index_count: print_mesh.mesh_data.indices.len() as u32,
            }
        })
        .collect();

    // Step 4: Calculate layer count and iteration parameters
    let layer_height = job_config.layer_height_mm();
    let volume_height = printer_config.volume.height_mm;
    let total_layers = ((volume_height / layer_height).ceil()) as u32;

    // Step 5-6: Create ZIP and iterate layers
    let cursor = std::io::Cursor::new(Vec::<u8>::new());
    let mut zip_writer = zip::ZipWriter::new(cursor);
    let zip_options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut layer_num: u32 = 0;
    let mut z = layer_height;

    while z <= volume_height {
        // Check for cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(MicrotomeError::Cancelled);
        }

        // Slice at this z height
        slicer.slice_at(
            z as f32,
            printer_config.volume.width_mm as f32,
            printer_config.volume.depth_mm as f32,
            volume_height as f32,
            &mesh_buffers,
        )?;

        // Read the slice to PNG
        let mut png_data: Vec<u8> = Vec::new();
        slicer.read_slice_to_png(&mut png_data)?;

        // Write PNG to ZIP with zero-padded filename
        let filename = format!("{:08}.png", layer_num);
        zip_writer
            .start_file(&filename, zip_options)
            .map_err(|e| MicrotomeError::Zip(e.to_string()))?;
        zip_writer
            .write_all(&png_data)
            .map_err(|e| MicrotomeError::Zip(e.to_string()))?;

        layer_num += 1;

        // Send progress update
        let progress = layer_num as f32 / total_layers as f32;
        let _ = progress_tx.send(SliceProgress::Progress(progress));

        z += layer_height;
    }

    // Step 7: Add slice-config.json
    let slice_config = SliceConfig {
        printer_name: printer_config.name.clone(),
        layer_count: layer_num,
        layer_height_mm: layer_height,
        x_res_px: width,
        y_res_px: height,
    };
    let config_json = serde_json::to_string_pretty(&slice_config)
        .map_err(|e| MicrotomeError::Slicing(format!("Failed to serialize slice config: {e}")))?;

    zip_writer
        .start_file("slice-config.json", zip_options)
        .map_err(|e| MicrotomeError::Zip(e.to_string()))?;
    zip_writer
        .write_all(config_json.as_bytes())
        .map_err(|e| MicrotomeError::Zip(e.to_string()))?;

    // Step 8: Finish ZIP and send completion
    let finished_cursor = zip_writer
        .finish()
        .map_err(|e| MicrotomeError::Zip(e.to_string()))?;

    let zip_bytes = finished_cursor.into_inner();
    let _ = progress_tx.send(SliceProgress::Complete(zip_bytes.clone()));

    Ok(zip_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_config_serialization() {
        let config = SliceConfig {
            printer_name: "Test Printer".to_string(),
            layer_count: 3000,
            layer_height_mm: 0.05,
            x_res_px: 2560,
            y_res_px: 1440,
        };

        let json = serde_json::to_string_pretty(&config);
        assert!(json.is_ok());

        let json_str = json.unwrap_or_default();
        assert!(json_str.contains("\"printer_name\": \"Test Printer\""));
        assert!(json_str.contains("\"layer_count\": 3000"));
        assert!(json_str.contains("\"layer_height_mm\": 0.05"));
        assert!(json_str.contains("\"x_res_px\": 2560"));
        assert!(json_str.contains("\"y_res_px\": 1440"));
    }

    #[test]
    fn slicing_job_cancel_flag() {
        let (tx, _rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let job = SlicingJob::new(tx, Arc::clone(&cancel));

        assert!(!job.is_cancelled());
        job.cancel();
        assert!(job.is_cancelled());
    }
}
