//! Command-line argument parsing shared between the interactive viewer
//! (which only reads `--headless` to decide which mode to run) and the
//! headless renderer (which honours every argument).

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use glam::Vec3;

use microtome_core::isosurface::SignMode;

use crate::app::Structure;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Isosurface viewer (interactive eframe app, or `--headless` PNG renderer).",
    long_about = None,
)]
pub struct Args {
    /// Skip the GUI: build the mesh, render one frame, write a PNG, and exit.
    #[arg(long)]
    pub headless: bool,

    /// Mesh file to remesh (.obj or .stl). When absent, the default
    /// SDF scene (cube minus cylinder) is rendered.
    #[arg(long)]
    pub mesh: Option<PathBuf>,

    /// PNG output path. Defaults to `/tmp/microtome-view-<unix_ts>.png`.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Image width in pixels.
    #[arg(long, default_value_t = 1280)]
    pub width: u32,
    /// Image height in pixels.
    #[arg(long, default_value_t = 720)]
    pub height: u32,

    /// Camera azimuth in radians.
    #[arg(long, allow_hyphen_values = true, default_value_t = std::f32::consts::FRAC_PI_4)]
    pub theta: f32,
    /// Camera elevation from +Z in radians.
    #[arg(long, allow_hyphen_values = true, default_value_t = std::f32::consts::FRAC_PI_3)]
    pub phi: f32,
    /// Camera orbit radius (distance from target).
    #[arg(long, allow_hyphen_values = true, default_value_t = 16.0)]
    pub radius: f32,
    /// Orbit target as `x,y,z` in world space.
    #[arg(long, allow_hyphen_values = true, value_parser = parse_vec3, default_value = "0,0,0")]
    pub target: Vec3,

    /// Octree depth.
    #[arg(long, default_value_t = 8)]
    pub depth: u32,
    /// QEF error threshold for octree simplification.
    #[arg(long, allow_hyphen_values = true, default_value_t = 1e-2)]
    pub threshold: f32,
    /// Acceleration structure used to extract the mesh.
    #[arg(long, value_enum, default_value_t = StructureArg::Octree)]
    pub structure: StructureArg,
    /// Corner-sign generation mode for `ScannedMeshField` (only matters
    /// when `--mesh` is set; the default scene uses an analytic SDF).
    #[arg(long, value_enum, default_value_t = SignModeArg::Gwn)]
    pub sign_mode: SignModeArg,

    /// Render the wireframe overlay instead of solid Phong-shaded faces.
    #[arg(long)]
    pub wireframe: bool,

    /// In wireframe mode, how to handle back-facing geometry. `plain`
    /// shows every line; `filled` paints a background-colour fill
    /// behind the wires so back lines are occluded; `cull-back`
    /// drops back-facing triangles entirely. No effect on the solid
    /// Phong path.
    #[arg(long, value_enum, default_value_t = WireframeModeArg::Plain)]
    pub wireframe_mode: WireframeModeArg,

    /// Skip remeshing — render the input `--mesh` file directly. Useful
    /// for comparing the source against the DC output without rebuilding.
    #[arg(long)]
    pub original: bool,
}

/// CLI surface for [`Structure`]. Kept separate so the `clap` derive
/// only sees a plain enum.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum StructureArg {
    Octree,
    Kdtree,
    KdtreeV2,
}

impl From<StructureArg> for Structure {
    fn from(s: StructureArg) -> Self {
        match s {
            StructureArg::Octree => Self::Octree,
            StructureArg::Kdtree => Self::KdTree,
            StructureArg::KdtreeV2 => Self::KdTreeV2,
        }
    }
}

impl From<Structure> for StructureArg {
    fn from(s: Structure) -> Self {
        match s {
            Structure::Octree => Self::Octree,
            Structure::KdTree => Self::Kdtree,
            Structure::KdTreeV2 => Self::KdtreeV2,
        }
    }
}

impl StructureArg {
    /// Lowercase identifier used when emitting CLI commands from the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Octree => "octree",
            Self::Kdtree => "kdtree",
            Self::KdtreeV2 => "kdtree-v2",
        }
    }
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum SignModeArg {
    Gwn,
    Flood,
    Polymender,
}

impl From<SignModeArg> for SignMode {
    fn from(s: SignModeArg) -> Self {
        match s {
            SignModeArg::Gwn => Self::Gwn,
            SignModeArg::Flood => Self::FloodFill,
            SignModeArg::Polymender => Self::Polymender,
        }
    }
}

impl From<SignMode> for SignModeArg {
    fn from(s: SignMode) -> Self {
        match s {
            SignMode::Gwn => Self::Gwn,
            SignMode::FloodFill => Self::Flood,
            SignMode::Polymender => Self::Polymender,
        }
    }
}

impl SignModeArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gwn => "gwn",
            Self::Flood => "flood",
            Self::Polymender => "polymender",
        }
    }
}

/// How the wireframe overlay handles back-facing geometry.
#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum WireframeModeArg {
    /// Draw every wire (front and back). Default.
    Plain,
    /// Paint a background-colour fill behind the wires so back lines
    /// are occluded by front geometry.
    Filled,
    /// Drop back-facing triangles entirely (front-only wires).
    CullBack,
}

impl WireframeModeArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Filled => "filled",
            Self::CullBack => "cull-back",
        }
    }
}

/// Parses a `x,y,z` triple into a `Vec3`.
fn parse_vec3(s: &str) -> Result<Vec3, String> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(format!("expected `x,y,z`, got `{s}`"));
    }
    let x: f32 = parts[0].parse().map_err(|e| format!("bad x: {e}"))?;
    let y: f32 = parts[1].parse().map_err(|e| format!("bad y: {e}"))?;
    let z: f32 = parts[2].parse().map_err(|e| format!("bad z: {e}"))?;
    Ok(Vec3::new(x, y, z))
}

/// Returns `/tmp/microtome-view-<unix_ts>.png`.
pub fn default_output_path() -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    PathBuf::from(format!("/tmp/microtome-view-{ts}.png"))
}
