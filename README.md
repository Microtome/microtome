# Microtome

GPU-accelerated model slicer for DLP-style resin 3D printers. Generates black-and-white bitmap slice images using wgpu compute and render pipelines.

## Features

- Fast GPU slicing via wgpu (intersection-test / slice-extract two-pass algorithm)
- 3D viewport with Phong-shaded mesh rendering, orbit camera, and transform gizmos
- Real-time slice preview with overlay visualization
- STL file loading with automatic winding order correction
- Batch export to ZIP archives of PNG slice images
- Handles overlapping and self-intersecting geometry without boolean pre-processing

## Building

Requires Rust 1.85+ and a GPU with Vulkan, Metal, or DX12 support.

```bash
cargo build --release
cargo run --release
```

## Testing

```bash
cargo nextest run
```

## Project Structure

```
crates/
  microtome-core/   # Slicing engine library (config, mesh, slicer, job export)
  microtome-app/    # Desktop application (eframe/egui, 3D viewport, UI)
```
