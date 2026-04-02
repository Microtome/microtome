//! Helper methods on MicrotomeApp for mesh management and job control.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use glam::{Mat4, Quat};
use microtome_core::{MeshData, PrintMesh, PrinterScene, SliceProgress, run_slicing_job};
use wgpu::util::DeviceExt;

use crate::viewport_renderer::MeshBuffers;

use super::state::{GpuMesh, MicrotomeApp};

impl MicrotomeApp {
    /// Uploads a loaded mesh's vertex and index data to the GPU.
    pub(super) fn upload_mesh(
        &mut self,
        render_state: &egui_wgpu::RenderState,
        mesh_data: &MeshData,
    ) {
        let vertex_buffer =
            render_state
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh_vertices"),
                    contents: bytemuck::cast_slice(&mesh_data.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

        let index_buffer =
            render_state
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh_indices"),
                    contents: bytemuck::cast_slice(&mesh_data.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        self.gpu_meshes.push(GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count: mesh_data.indices.len() as u32,
        });
    }

    /// Builds the model matrix for a [`PrintMesh`] from its position, rotation, and scale.
    pub(super) fn model_matrix(mesh: &PrintMesh) -> Mat4 {
        let translation = Mat4::from_translation(mesh.position);
        let rotation = Mat4::from_quat(Quat::from_euler(
            glam::EulerRot::XYZ,
            mesh.rotation.x,
            mesh.rotation.y,
            mesh.rotation.z,
        ));
        let scale = Mat4::from_scale(mesh.scale);
        translation * rotation * scale
    }

    /// Builds the list of [`MeshBuffers`] for the current frame.
    pub(super) fn collect_mesh_buffers(&self) -> Vec<MeshBuffers> {
        self.gpu_meshes
            .iter()
            .zip(self.scene.meshes.iter())
            .map(|(gpu, scene_mesh)| MeshBuffers {
                vertex_buffer: gpu.vertex_buffer.clone(),
                index_buffer: gpu.index_buffer.clone(),
                index_count: gpu.index_count,
                model_matrix: Self::model_matrix(scene_mesh),
            })
            .collect()
    }

    /// Polls the progress receiver for slicing job updates.
    ///
    /// Handles progress fraction updates, job completion (writing ZIP to disk),
    /// and job failure/cancellation.
    pub(super) fn poll_slicing_progress(&mut self) {
        let should_clear = if let Some(rx) = &self.progress_rx {
            let mut should_clear = false;
            // Drain all available messages
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SliceProgress::Progress(p) => {
                        self.slicing_progress = Some(p);
                    }
                    SliceProgress::Complete(zip_bytes) => {
                        if let Some(path) = &self.export_path {
                            if let Err(e) = std::fs::write(path, &zip_bytes) {
                                log::error!("Failed to write ZIP: {e}");
                            } else {
                                log::info!("Exported ZIP to: {}", path.display());
                            }
                        }
                        should_clear = true;
                        break;
                    }
                    SliceProgress::Failed(msg) => {
                        log::error!("Slicing job failed: {msg}");
                        should_clear = true;
                        break;
                    }
                }
            }
            should_clear
        } else {
            false
        };

        if should_clear {
            self.slicing_progress = None;
            self.progress_rx = None;
            self.export_path = None;
            self.cancel_flag = None;
        }
    }

    /// Starts a slicing job on a background thread.
    ///
    /// Constructs a fresh [`PrinterScene`] from the current config and meshes,
    /// then spawns the job in a background thread.
    pub(super) fn start_slicing_job(&mut self) {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));

        // Build a fresh scene for the background thread (PrinterScene is not Clone)
        let mut scene = PrinterScene::from_config(&self.printer_config);
        for mesh in &self.scene.meshes {
            scene.add_mesh(mesh.clone());
        }
        scene.set_overhang_angle_degrees(self.overhang_angle_degrees as f64);

        let printer_config = self.printer_config.clone();
        let job_config = self.job_config.clone();
        let cancel_clone = Arc::clone(&cancel);
        let tx_clone = tx.clone();

        std::thread::spawn(move || {
            if let Err(e) =
                run_slicing_job(&scene, &printer_config, &job_config, tx_clone, cancel_clone)
            {
                let _ = tx.send(SliceProgress::Failed(e.to_string()));
            }
        });

        self.slicing_progress = Some(0.0);
        self.progress_rx = Some(rx);
        self.cancel_flag = Some(cancel);
    }
}
