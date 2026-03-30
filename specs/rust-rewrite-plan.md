# Microtome Rust Rewrite Plan

## Context

Microtome is a GPU-accelerated model slicer for DLP-style 3D printers. It currently exists as a TypeScript/Three.js/WebGL web application that loads STL files, visualizes them in 3D, slices them at Z-heights using GPU shader operations, and exports ZIP archives of PNG slice images for 3D printing.

The goal is to rewrite Microtome as a fully native Rust desktop application with no web technologies, using modern GPU APIs and a native GUI toolkit.

### Technology Choices

| Concern | Current (TypeScript) | Target (Rust) |
|---------|---------------------|---------------|
| Language | TypeScript | Rust |
| 3D Rendering | Three.js / WebGL | wgpu (Vulkan/Metal/DX12/OpenGL) |
| GUI | HTML/CSS (Materialize) + vanilla JS | egui + eframe |
| Shader Language | GLSL | WGSL |
| Math | Three.js vectors/matrices | glam |
| STL Parsing | Three.js STLLoader | stl_io |
| Image Output | Canvas toDataURL | image crate (PNG) |
| ZIP Output | jszip | zip crate |
| File Dialogs | HTML `<input type="file">` | rfd |
| Build System | Parcel/Rollup/tsc | Cargo |

---

## Workspace Structure

```
microtome-rs/
├── Cargo.toml                          # Workspace root
├── crates/
│   ├── microtome-core/                 # Library crate (slicing engine)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # Public API re-exports
│   │       ├── config.rs               # PrinterConfig, PrintJobConfig structs
│   │       ├── units.rs                # LengthUnit enum, conversion functions
│   │       ├── mesh.rs                 # STL loading, MeshData, PrintMesh, volume calc
│   │       ├── scene.rs                # PrinterScene, PrintVolumeBox
│   │       ├── gpu.rs                  # GpuContext: wgpu device/queue initialization
│   │       ├── slicer.rs              # AdvancedSlicer (GPU slicing pipeline)
│   │       ├── job.rs                  # SlicingJob (batch export to ZIP)
│   │       ├── error.rs               # MicrotomeError, Result type alias
│   │       └── shaders/
│   │           ├── intersection.wgsl   # Front/back face counting (render pass)
│   │           ├── slice_extract.wgsl  # Inside/outside determination (render pass)
│   │           ├── erode_dilate.wgsl   # Morphological ops (compute shader)
│   │           ├── boolean_ops.wgsl    # OR/XOR operations (compute shader)
│   │           └── overhang.wgsl       # Overhang angle visualization (fragment)
│   └── microtome-app/                  # Binary crate (desktop application)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs                 # eframe::run_native entry point
│           ├── app.rs                  # MicrotomeApp: impl eframe::App
│           ├── viewport.rs             # 3D viewport via egui PaintCallback
│           ├── viewport_renderer.rs    # wgpu Phong + wireframe render pipelines
│           ├── slice_preview.rs        # 2D slice preview display
│           ├── camera.rs               # OrbitCamera (spherical coordinates)
│           ├── picking.rs              # Raycasting for mesh selection
│           ├── shaders/
│           │   ├── phong.wgsl          # Phong lighting (vertex + fragment)
│           │   └── line.wgsl           # Wireframe line rendering
│           └── ui/
│               ├── mod.rs
│               ├── panels.rs           # Side panel, bottom bar layouts
│               ├── config_editor.rs    # Printer/job config editing widgets
│               └── file_dialogs.rs     # STL loading, ZIP export via rfd
```

---

## Dependencies

### Workspace `Cargo.toml`

```toml
[workspace]
resolver = "2"
members = ["crates/microtome-core", "crates/microtome-app"]

[workspace.dependencies]
wgpu = "26"
egui = "0.31"
eframe = { version = "0.31", default-features = false, features = ["wgpu"] }
egui-wgpu = "0.31"
glam = { version = "0.29", features = ["bytemuck"] }
bytemuck = { version = "1", features = ["derive"] }
image = { version = "0.25", default-features = false, features = ["png"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
thiserror = "2"
anyhow = "1"
```

### microtome-core

```toml
[dependencies]
wgpu.workspace = true
glam.workspace = true
bytemuck.workspace = true
image.workspace = true
serde.workspace = true
serde_json.workspace = true
log.workspace = true
thiserror.workspace = true
zip = { version = "2", default-features = false, features = ["deflate"] }
stl_io = "0.8"
pollster = "0.4"
```

### microtome-app

```toml
[dependencies]
microtome-core = { path = "../microtome-core" }
eframe.workspace = true
egui.workspace = true
egui-wgpu.workspace = true
wgpu.workspace = true
glam.workspace = true
bytemuck.workspace = true
log.workspace = true
env_logger = "0.11"
anyhow.workspace = true
rfd = "0.15"
pollster = "0.4"
```

---

## Source File Mapping

| Original TypeScript | Rust Target | Notes |
|---|---|---|
| `src/lib/config.ts` | `microtome-core/src/config.rs` | 6 interfaces → 6 structs with Serde |
| `src/lib/units.ts` | `microtome-core/src/units.rs` | LengthUnit enum + convert_length fn |
| `src/lib/common.ts` | Inlined into users | glam constants (Vec3::Z, etc.) |
| `src/lib/materials.ts` | Split into slicer.rs + viewport_renderer.rs | Shader uniforms become wgpu bind groups |
| `src/lib/printer.ts` | `microtome-core/src/mesh.rs` + `scene.rs` | PrintMesh, PrinterScene, volume calc |
| `src/lib/camera.ts` | `microtome-app/src/camera.rs` | OrbitCamera with egui input handling |
| `src/lib/slicer.ts` | `microtome-core/src/slicer.rs` | Hybrid render-pass + compute shader pipeline |
| `src/lib/job.ts` | `microtome-core/src/job.rs` | Background thread with progress channel |
| `src/lib/shaders/*.glsl` | `microtome-core/src/shaders/*.wgsl` | GLSL → WGSL translation |
| `src/app/main.ts` | `microtome-app/src/main.rs` + `app.rs` | eframe entry point |
| `src/app/printerVolumeView.ts` | `microtome-app/src/viewport.rs` + `viewport_renderer.rs` | egui PaintCallback |
| `src/app/slicePreview.ts` | `microtome-app/src/slice_preview.rs` | CPU readback to egui texture |

---

## GPU Pipeline Design

### Architecture Decision: Hybrid Render-Pass + Compute Shader

The original WebGL approach uses fragment shaders and render targets for all operations. The Rust rewrite uses a hybrid approach:

- **Render passes** for intersection test and slice extraction (these are rasterization-bound)
- **Compute shaders** for erosion/dilation and boolean operations (these are image processing)

**Rationale**: The intersection test fundamentally requires rasterizing 3D triangles onto a 2D plane with specific blending modes. The GPU rasterizer handles this natively and efficiently. Reimplementing rasterization in a compute shader would be slower and more complex. Conversely, erosion/dilation and boolean ops are pure image processing with no geometric input, making them natural compute workloads.

### Slicing Pipeline (per-layer)

```
Step 1: Intersection Test (Render Pass)
┌─────────────────────────────────────────────────┐
│ Render all meshes with orthographic camera       │
│ looking down Z-axis.                             │
│                                                   │
│ Blending: Additive (src=One, dst=One)            │
│ Depth test: Disabled                              │
│ Cull mode: None (both faces)                     │
│                                                   │
│ Fragment shader:                                  │
│   - Discard fragments below Z cutoff             │
│   - Front faces → R channel += 1/256             │
│   - Back faces → G channel += 1/256              │
│                                                   │
│ Output: temp2 texture (RGBA8Unorm)               │
└─────────────────────────────────────────────────┘
                    ↓
Step 2: Slice Extraction (Render Pass)
┌─────────────────────────────────────────────────┐
│ Render same meshes, sampling temp2.              │
│                                                   │
│ Fragment shader:                                  │
│   - shouldBeWhite = (green - red) * 255          │
│   - Front face + shouldBeWhite > 0 → WHITE      │
│   - Front face + shouldBeWhite <= 0 → BLACK     │
│   - Back face → always WHITE                     │
│                                                   │
│ Output: mask texture                              │
└─────────────────────────────────────────────────┘
                    ↓
Step 3: Morphological Operations (Compute Shader) [if needed]
┌─────────────────────────────────────────────────┐
│ For raft layers (z <= raft_thickness):           │
│   Dilate mask by raft_offset pixels              │
│                                                   │
│ For shell inset (shell_erode > 0):               │
│   Erode mask by shell_erode pixels               │
│                                                   │
│ Circular structuring element (radius up to 10px) │
│ Multi-pass for larger radii                      │
│ Workgroup size: 8x8                              │
│                                                   │
│ Input: mask texture (TEXTURE_BINDING)            │
│ Output: scratch texture (STORAGE_BINDING)        │
└─────────────────────────────────────────────────┘
                    ↓
Step 4: CPU Readback (for PNG encoding)
┌─────────────────────────────────────────────────┐
│ Copy final texture to staging buffer             │
│ Map staging buffer to CPU                        │
│ Encode as PNG via image crate                    │
│ Add to ZIP archive                               │
└─────────────────────────────────────────────────┘
```

### Render Target Configuration

All intermediate textures use:
- Format: `Rgba8Unorm`
- Filter: Nearest
- Usage: `RENDER_ATTACHMENT | TEXTURE_BINDING | STORAGE_BINDING | COPY_SRC`
- Dimensions: Match projector resolution (e.g., 2560x1920)

### Orthographic Camera for Slicing

```rust
// Camera looks down from above the print volume
let eye = Vec3::new(0.0, 0.0, volume_height + 1.0);
let center = Vec3::new(0.0, 0.0, -1000000.0);
let up = Vec3::Y;
let view = Mat4::look_at_rh(eye, center, up);

// Orthographic projection covering the print volume
// wgpu uses [0,1] depth range (not [-1,1] like OpenGL)
let proj = Mat4::orthographic_rh(left, right, bottom, top, near, far);
```

### WGSL Shader Ports

#### intersection.wgsl (from intersection_shader_frag.glsl)

```wgsl
@fragment
fn fs_intersection(in: FragInput) -> @location(0) vec4<f32> {
    let z_cutoff = 1.0 - slice.cutoff;
    if (in.frag_coord.z < z_cutoff) { discard; }
    if (in.front_facing) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0 / 256.0);
    } else {
        return vec4<f32>(0.0, 1.0, 0.0, 1.0 / 256.0);
    }
}
```

#### slice_extract.wgsl (from slice_shader_frag.glsl)

```wgsl
@fragment
fn fs_slice(in: FragInput) -> @location(0) vec4<f32> {
    let uv = in.frag_coord.xy / vec2<f32>(view_width, view_height);
    let color = textureSample(intersection_tex, samp, uv);
    let should_be_white = (color.g - color.r) * 255.0;
    let z_cutoff = 1.0 - slice.cutoff;
    if (in.frag_coord.z < z_cutoff) { discard; }
    if (in.front_facing) {
        if (should_be_white > 0.0) { return vec4(1.0); }
        else { return vec4(0.0, 0.0, 0.0, 1.0); }
    } else {
        return vec4(1.0);
    }
}
```

#### erode_dilate.wgsl (from erode_dilate_frag.glsl → compute shader)

```wgsl
@compute @workgroup_size(8, 8)
fn cs_erode_dilate(@builtin(global_invocation_id) gid: vec3<u32>) {
    // Same circular structuring element algorithm
    // Uses textureLoad/textureStore instead of texture2D
    // Radius up to 10px, multi-pass for larger
}
```

#### boolean_ops.wgsl (from or_shader_frag.glsl + xor_shader_frag.glsl → compute)

```wgsl
@compute @workgroup_size(8, 8)
fn cs_or(...) { /* threshold at 0.9, output max */ }

@compute @workgroup_size(8, 8)
fn cs_xor(...) { /* (A OR B) AND NOT (A AND B) */ }
```

#### overhang.wgsl (from overhang_shader_frag.glsl)

```wgsl
@fragment
fn fs_overhang(in: VertexOutput) -> @location(0) vec4<f32> {
    let dot_g = dot(in.world_normal, vec3(0.0, 0.0, -1.0));
    let t = (dot_g - cos_angle_rad) / (1.0 - cos_angle_rad);
    return vec4(1.0, 1.0 - t, 1.0 - t, 1.0);
}
```

---

## Application UI Design

### Layout

```
┌──────────────┬──────────────────────────────────────────────┐
│              │                                               │
│  Controls    │         3D Viewport          │  Slice Preview │
│  Panel       │    (egui PaintCallback       │  (egui texture │
│              │     with wgpu Phong          │   from GPU     │
│  - Load STL  │     rendering)              │   readback)    │
│  - Printer   │                              │                │
│    Config    │                              │                │
│  - Job       │                              │                │
│    Config    │                              │                │
│  - Overhang  │                              │                │
│    Angle     │                              │                │
│  - Export    │                              │                │
│    to ZIP    │                              │                │
│              │                              │                │
├──────────────┴──────────────────────────────┴────────────────┤
│  Slice Z: [====slider====]  24.0 mm     [Progress: ██░ 67%] │
└──────────────────────────────────────────────────────────────┘
```

### 3D Viewport

- Uses `egui::PaintCallback` with a custom `CallbackTrait` implementation
- Renders via a `ViewportRenderer` stored in egui-wgpu's `CallbackResources`
- Phong lighting: ambient (#777777) + sky directional (#AACCFF, intensity 0.65, from +Z) + ground directional (#775533, intensity 0.45, from -Z)
- Wireframe print volume box with colored axis lines (R=X, G=Y, B=Z)
- Perspective camera: 37 deg FOV, Z-up

### Orbit Camera

Ported from `src/lib/camera.ts`. Spherical coordinate system:

- Mouse drag → rotate (theta = azimuth, phi = elevation)
- Scroll wheel → zoom (constrained 5.0 to 1000.0 units)
- Phi clamped to (0, pi) to prevent gimbal lock
- egui `Response` provides drag delta and scroll delta

### Slice Preview

- CPU readback approach: read slice texture pixels via staging buffer, upload as `egui::TextureHandle`
- Only updates when `slice_z` changes (not every frame)
- Renders at reduced resolution for interactive preview, full resolution for export

### Mesh Picking

- CPU raycasting: cast ray from mouse position through perspective camera
- Test ray-triangle intersection against all mesh triangles
- Selected mesh renders with cyan material (#00cfcf), unselected with gray (#cfcfcf)

---

## Key Technical Decisions

### 1. GpuContext Sharing

The `GpuContext` supports both standalone creation (for headless/tests) and wrapping eframe's wgpu device/queue. In the app, the device and queue come from eframe's render state:

```rust
impl GpuContext {
    pub async fn new_standalone() -> Result<Self> { /* create own device */ }
    pub fn from_existing(device: Arc<Device>, queue: Arc<Queue>) -> Self { /* wrap */ }
}
```

### 2. Batch Slicing on Background Thread

The slicing job runs on a separate thread with progress reporting via `std::sync::mpsc::channel`. The app polls for completion each frame. The slicer needs its own `GpuContext` (or shared with synchronization).

```rust
// In job.rs
pub fn start_slicing_job(
    gpu: GpuContext,
    scene: &PrinterScene,
    printer_config: &PrinterConfig,
    job_config: &PrintJobConfig,
    progress_tx: mpsc::Sender<f32>,
) -> Result<Vec<u8>> { /* returns ZIP bytes */ }
```

### 3. Coordinate System

Z-up throughout, matching the original. Print platform is the XY plane at Z=0, build direction is +Z. glam's `look_at_rh` works with any up direction.

### 4. Error Handling

- `microtome-core`: `thiserror` with typed `MicrotomeError` enum (GpuInit, StlParse, Slicing, Io, Zip, Image, Cancelled)
- `microtome-app`: `anyhow` for ergonomic error propagation with context

---

## Implementation Phases

### Phase 1: Foundation Types (no GPU needed)

**Files**: `units.rs`, `config.rs`, `error.rs`

- Port `LengthUnit` enum with Micron/Millimeter/Centimeter/Inch
- Port `convert_length` function
- Port all config structs (PrinterConfig, PrintJobConfig, etc.) with Serde derive
- Define `MicrotomeError` enum and `Result<T>` type alias

**Test**: Unit tests for conversions, serde round-trips, layer height calculation.

**Reference files**:
- `/var/home/djoyce/git/microtome/src/lib/units.ts`
- `/var/home/djoyce/git/microtome/src/lib/config.ts`

### Phase 2: Mesh and Scene (no GPU needed)

**Files**: `mesh.rs`, `scene.rs`

- STL loading via `stl_io` → `MeshData` struct (vertices, indices, bbox, volume)
- `MeshVertex` as `#[repr(C)]` with bytemuck for GPU upload
- Volume calculation using signed tetrahedron method (port `_calculateVolume`)
- `PrintMesh` with transform, position, rotation, scale
- `PrinterScene` with `PrintVolumeBox` and `Vec<PrintMesh>`

**Test**: Volume calc on known cube (1mm^3), bbox correctness, STL loading from embedded test data.

**Reference files**:
- `/var/home/djoyce/git/microtome/src/lib/printer.ts` (lines 177-230)

### Phase 3: GPU Context and Slicing Pipeline

**Files**: `gpu.rs`, `slicer.rs`, all WGSL shaders

This is the most complex phase. Port the `AdvancedSlicer` with:

1. `GpuContext` with standalone and from-existing constructors
2. Create intersection render pipeline (additive blending, no depth test, double-sided)
3. Create slice extraction render pipeline (no blending, double-sided)
4. Create erode/dilate compute pipeline (8x8 workgroups)
5. Create boolean ops compute pipelines (OR, XOR)
6. Allocate render targets (mask, scratch, temp1, temp2) as `Rgba8Unorm` textures
7. Implement `slice_at(z)` method orchestrating the full pipeline
8. Implement `slice_at_to_png(z)` with staging buffer readback + PNG encoding

**Key porting considerations**:
- Three.js uses [-1,1] depth range; wgpu uses [0,1]. `glam::Mat4::orthographic_rh` handles this.
- The `cutoff` uniform normalizes Z to [0,1] via `(FAR_Z_PADDING + z) / (FAR_Z_PADDING + height)`.
- Fragment shaders use `@builtin(front_facing)` instead of `gl_FrontFacing`.
- Compute shaders use `textureLoad`/`textureStore` instead of `texture2D`.

**Reference files**:
- `/var/home/djoyce/git/microtome/src/lib/slicer.ts` (entire file, especially lines 172-330)
- All files in `/var/home/djoyce/git/microtome/src/lib/shaders/`
- `/var/home/djoyce/git/microtome/src/lib/materials.ts`

### Phase 4: Batch Slicing Job

**Files**: `job.rs`

- Iterate Z from 0 to max height by layer step
- Call `slicer.slice_at_to_png(z)` for each layer
- Pack PNGs into ZIP with sequential filenames (`00000000.png`, etc.)
- Include `slice-config.json` in ZIP
- Progress reporting via `mpsc::Sender<f32>`
- Cancellation via `Arc<AtomicBool>`

**Reference files**:
- `/var/home/djoyce/git/microtome/src/lib/job.ts`

### Phase 5: App Scaffold

**Files**: `main.rs`, `app.rs`, `camera.rs`

- eframe entry point with `NativeOptions` (wgpu renderer, 1280x800 window)
- `MicrotomeApp` struct holding scene, config, camera, selection state
- Extract wgpu device/queue from eframe's `CreationContext`
- Basic egui layout: side panel + central panel + bottom bar
- `OrbitCamera` with spherical coordinates, mouse drag rotation, scroll zoom

**Reference files**:
- `/var/home/djoyce/git/microtome/src/app/main.ts`
- `/var/home/djoyce/git/microtome/src/lib/camera.ts`

### Phase 6: 3D Viewport Rendering

**Files**: `viewport.rs`, `viewport_renderer.rs`, `shaders/phong.wgsl`, `shaders/line.wgsl`

- `ViewportRenderer` stored in egui-wgpu `CallbackResources`
- Phong render pipeline for meshes (ambient + 2 directional lights)
- Line render pipeline for print volume wireframe + axis lines
- Overhang visualization render pipeline (port overhang shader)
- `ViewportPaintCallback` implementing `egui_wgpu::CallbackTrait`
- Depth buffer for proper occlusion

**Reference files**:
- `/var/home/djoyce/git/microtome/src/app/printerVolumeView.ts`
- `/var/home/djoyce/git/microtome/src/lib/shaders/overhang_shader_frag.glsl`

### Phase 7: Slice Preview

**Files**: `slice_preview.rs`

- Create `AdvancedSlicer` instance for preview
- On `slice_z` change: run slicer, readback to CPU, upload as `egui::TextureHandle`
- Display in right column of central panel
- Show current Z height and layer info

**Reference files**:
- `/var/home/djoyce/git/microtome/src/app/slicePreview.ts`

### Phase 8: UI Panels and File I/O

**Files**: `ui/panels.rs`, `ui/config_editor.rs`, `ui/file_dialogs.rs`

- STL file loading via `rfd::FileDialog`
- Printer config editor (volume, projector resolution, z-stage)
- Job config editor (layer height, exposure time, raft settings)
- Overhang angle slider (0-90 degrees)
- Object transform controls (position, rotation, scale) when mesh selected
- Export-to-ZIP button triggering background slicing job
- Progress bar during slicing

### Phase 9: Integration and Polish

- Wire up all components end-to-end
- Handle window resize for viewport and slice preview
- Error dialogs for GPU failures, bad STL files, etc.
- Keyboard shortcuts (Delete for removing selected mesh, etc.)
- Menu bar with File > Open STL, File > Export ZIP

---

## Phase Dependency Graph

```
Phase 1 (Foundation)
  ↓
Phase 2 (Mesh/Scene)
  ↓           ↓
Phase 3      Phase 5
(GPU Slicer) (App Scaffold)
  ↓           ↓
Phase 4      Phase 6
(Job)        (3D Viewport)
  ↓           ↓
  ↓          Phase 7
  ↓          (Slice Preview)
  ↓           ↓
  └─→ Phase 8 (UI Panels)
        ↓
      Phase 9 (Integration)
```

Phases 3 and 5 can proceed in parallel. Phase 6 depends on both 3 and 5. Phase 8 depends on 4, 6, and 7.

---

## Verification

### Unit Tests (no GPU)
- `units.rs`: Round-trip conversions, known values (1 inch = 25.4mm)
- `config.rs`: Serde serialize/deserialize, `layer_height_mm()` correctness
- `mesh.rs`: Volume of unit cube = 1mm^3, bounding box correctness, STL parse from embedded bytes

### GPU Integration Tests
- Empty scene slice → all black
- Cube slice at midpoint → filled rectangle
- Sphere slice at equator → approximately circular
- Full slicing job → valid ZIP with numbered PNGs + config JSON
- Erode/dilate on known pattern → expected morphological result

### Visual Regression Tests
- Save reference PNG slices of known scenes
- Compare new output per-pixel with tolerance threshold

### Manual Testing Checklist
- [ ] Load binary STL file
- [ ] Load ASCII STL file
- [ ] Orbit camera (drag to rotate, scroll to zoom)
- [ ] Pick mesh (click to select, color changes to cyan)
- [ ] Slice preview updates when moving Z slider
- [ ] Overhang visualization toggles with angle slider
- [ ] Export to ZIP produces valid archive with PNG layers
- [ ] Window resize keeps viewport and preview proportional
- [ ] Multiple meshes in scene (overlapping geometry handled correctly)
