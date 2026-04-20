//! Headless render path for the isosurface viewer.
//!
//! Spins up a windowless wgpu device, builds the same mesh the
//! interactive app would build, drives [`ViewportRenderer`] to render a
//! single frame to its offscreen target, reads the pixels back, and
//! writes a PNG. Used for scripted renders / regression captures.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use glam::Vec3;
use wgpu::util::DeviceExt;

use microtome_core::isosurface::{IsoMesh, ScannedMeshField};
use microtome_core::{MeshData, MicrotomeError};

use crate::app::{IsosurfaceApp, Structure};
use crate::camera::OrbitCamera;
use crate::cli::{Args, default_output_path};
use crate::renderer::{MeshBuffers, OFFSCREEN_FORMAT, ViewportRenderer};

/// Builds the mesh from `args` and writes a PNG of the rendered frame.
pub fn run(args: &Args) -> Result<()> {
    let structure: Structure = args.structure.into();

    // ---- Build the isosurface mesh ----------------------------------
    let (mesh, frame_bbox) = build_mesh(args, structure)?;
    let mut iso_mesh = mesh.ok_or_else(|| {
        anyhow!("isosurface build produced no geometry — check --depth/--threshold and --mesh")
    })?;
    iso_mesh.generate_flat_normals();
    let mesh_data = iso_mesh.to_mesh_data();
    if mesh_data.vertices.is_empty() || mesh_data.indices.is_empty() {
        return Err(anyhow!("built mesh has no vertices/indices"));
    }

    // ---- Camera -----------------------------------------------------
    let mut camera = OrbitCamera::new();
    let target = if args.target == Vec3::ZERO {
        // Default target is origin, but if the model has a meaningful
        // bbox we want to look at it.
        frame_bbox
            .map(|(mn, mx)| (mn + mx) * 0.5)
            .unwrap_or(Vec3::ZERO)
    } else {
        args.target
    };
    camera.set_state(args.theta, args.phi, args.radius, target);

    // ---- Initialise windowless wgpu --------------------------------
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .context("no suitable wgpu adapter for headless rendering")?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("microtome_headless_device"),
        required_features: wgpu::Features::POLYGON_MODE_LINE,
        required_limits: wgpu::Limits::default(),
        ..Default::default()
    }))
    .context("failed to acquire wgpu device for headless rendering")?;

    // ---- Upload mesh ------------------------------------------------
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("headless_vertex_buffer"),
        contents: bytemuck::cast_slice(&mesh_data.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("headless_index_buffer"),
        contents: bytemuck::cast_slice(&mesh_data.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let mesh_buffers = Arc::new(MeshBuffers {
        vertex_buffer,
        index_buffer,
        index_count: mesh_data.indices.len() as u32,
    });

    // ---- Render -----------------------------------------------------
    let mut renderer = ViewportRenderer::new(&device, OFFSCREEN_FORMAT);
    let aspect = (args.width as f32) / (args.height.max(1) as f32);
    let view = camera.view_matrix();
    let proj = camera.projection_matrix(aspect);
    let view_proj = proj * view;

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("headless_render_encoder"),
    });
    renderer.render_offscreen(
        &device,
        &queue,
        &mut encoder,
        args.width,
        args.height,
        view_proj,
        Some(&mesh_buffers),
        args.wireframe,
    );
    queue.submit(Some(encoder.finish()));

    // ---- Read back & write PNG -------------------------------------
    let pixels = renderer.read_offscreen_pixels(&device, &queue)?;
    let (out_w, out_h) = renderer.offscreen_size();
    let img = image::RgbaImage::from_raw(out_w, out_h, pixels)
        .ok_or_else(|| anyhow!("pixel buffer too small for {out_w}×{out_h}"))?;

    let output_path = args.output.clone().unwrap_or_else(default_output_path);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output dir {}", parent.display()))?;
    }
    img.save(&output_path)
        .with_context(|| format!("failed to write PNG to {}", output_path.display()))?;

    println!(
        "wrote {} × {} → {} ({} triangles)",
        out_w,
        out_h,
        output_path.display(),
        mesh_data.indices.len() / 3
    );
    Ok(())
}

/// World-space bounding box of the source mesh (when one was loaded),
/// used to default the camera target to the model's centre.
type SourceBbox = Option<(Vec3, Vec3)>;

/// Builds the requested isosurface mesh. Returns `(mesh, optional_bbox)`
/// — bbox is set when a file was loaded so the caller can default the
/// camera target to the model's centre.
fn build_mesh(args: &Args, structure: Structure) -> Result<(Option<IsoMesh>, SourceBbox)> {
    if let Some(path) = &args.mesh {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let mesh: MeshData = match ext.as_str() {
            "obj" => MeshData::from_obj(path)?,
            "stl" => {
                let file = std::fs::File::open(path).map_err(MicrotomeError::Io)?;
                let mut reader = std::io::BufReader::new(file);
                MeshData::from_stl(&mut reader)?
            }
            other => return Err(anyhow!("unsupported mesh extension `.{other}`")),
        };
        if mesh.indices.is_empty() || mesh.vertices.is_empty() {
            return Err(anyhow!("loaded mesh has no geometry"));
        }
        let (min_code, unit_size) =
            IsosurfaceApp::loaded_mesh_bounds(mesh.bbox.min, mesh.bbox.max, args.depth);
        let size_code = 1_i32 << (args.depth - 1);
        let field = ScannedMeshField::from_mesh(
            &mesh,
            min_code,
            size_code,
            unit_size,
            args.sign_mode.into(),
        );
        let iso = IsosurfaceApp::build_mesh(
            &field,
            min_code,
            args.depth,
            structure,
            args.threshold,
            unit_size,
        );
        Ok((iso, Some((mesh.bbox.min, mesh.bbox.max))))
    } else {
        let (min_code, unit_size) = IsosurfaceApp::default_scene_bounds(args.depth);
        let field = IsosurfaceApp::build_default_scene();
        let iso = IsosurfaceApp::build_mesh(
            field.as_ref(),
            min_code,
            args.depth,
            structure,
            args.threshold,
            unit_size,
        );
        Ok((iso, None))
    }
}
