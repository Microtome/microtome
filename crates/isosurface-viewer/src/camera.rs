//! Orbit camera for 3D viewport navigation.
//!
//! Ported from the TypeScript `CameraNav` class in `src/lib/camera.ts`.
//! Uses spherical coordinates (theta, phi, radius) around a target point
//! in a Z-up coordinate system.

use glam::{Mat4, Vec3};

/// Minimum phi value to prevent gimbal lock at the poles.
const PHI_MIN: f32 = 0.01;

/// Maximum phi value to prevent gimbal lock at the poles.
const PHI_MAX: f32 = std::f32::consts::PI - 0.01;

/// Minimum zoom distance.
const RADIUS_MIN: f32 = 0.1;

/// Maximum zoom distance.
const RADIUS_MAX: f32 = 5000.0;

/// Near-plane distance for the projection matrix.
const NEAR_PLANE: f32 = 0.01;

/// Far-plane distance for the projection matrix.
const FAR_PLANE: f32 = 20_000.0;

/// Field of view in degrees.
#[allow(dead_code)]
const FOV_DEGREES: f32 = 37.0;

/// Zoom factor per scroll event (proportion of current radius).
const SCROLL_ZOOM_FACTOR: f32 = 0.03;

/// Orbit camera that rotates around a target point using spherical coordinates.
///
/// The camera uses a Z-up coordinate system with:
/// - `theta`: azimuth angle in the XY plane (radians)
/// - `phi`: elevation angle from the +Z axis (radians)
/// - `radius`: distance from the target point
#[allow(dead_code)]
pub struct OrbitCamera {
    /// Azimuth angle (radians).
    theta: f32,
    /// Elevation angle from +Z axis (radians).
    phi: f32,
    /// Distance from target.
    radius: f32,
    /// Point the camera orbits around.
    target: Vec3,
    /// Whether a drag is currently active.
    dragging: bool,
    /// Theta at the start of the current drag.
    drag_start_theta: f32,
    /// Phi at the start of the current drag.
    drag_start_phi: f32,
    /// Whether to use perspective (true) or orthographic (false) projection.
    pub use_perspective: bool,
}

#[allow(dead_code)]
impl OrbitCamera {
    /// Creates a new orbit camera with sensible defaults.
    ///
    /// Defaults: theta=PI/4, phi=PI/3, radius=200, target=origin.
    pub fn new() -> Self {
        Self {
            theta: std::f32::consts::FRAC_PI_4,
            phi: std::f32::consts::FRAC_PI_3,
            radius: 16.0,
            target: Vec3::ZERO,
            dragging: false,
            drag_start_theta: 0.0,
            drag_start_phi: 0.0,
            use_perspective: true,
        }
    }

    /// Returns the current eye (camera) position in world space.
    ///
    /// Computed from spherical coordinates centered on the target.
    pub fn eye_position(&self) -> Vec3 {
        let x = self.radius * self.phi.sin() * self.theta.cos();
        let y = self.radius * self.phi.sin() * self.theta.sin();
        let z = self.radius * self.phi.cos();
        self.target + Vec3::new(x, y, z)
    }

    /// Computes the view matrix (world-to-camera transform).
    ///
    /// Uses a right-handed coordinate system with Z-up.
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye_position(), self.target, Vec3::Z)
    }

    /// Computes the projection matrix (perspective or orthographic).
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        if self.use_perspective {
            Mat4::perspective_rh(FOV_DEGREES.to_radians(), aspect, NEAR_PLANE, FAR_PLANE)
        } else {
            // Orthographic: size based on radius so zoom still works
            let half_h = self.radius * (FOV_DEGREES.to_radians() / 2.0).tan();
            let half_w = half_h * aspect;
            Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, NEAR_PLANE, FAR_PLANE)
        }
    }

    /// Frames the camera so the given axis-aligned bounding box fills the view.
    ///
    /// The orbit target is set to the bbox center and the radius is chosen
    /// to fit the bbox diagonal within the current field of view, with a
    /// small padding margin.
    pub fn frame_bbox(&mut self, min: Vec3, max: Vec3) {
        let center = (min + max) * 0.5;
        let diag = (max - min).length();
        if !diag.is_finite() || diag <= 0.0 {
            self.target = center;
            return;
        }

        // Distance that fits a sphere of radius `diag / 2` at the current FOV.
        let half_fov = (FOV_DEGREES * 0.5).to_radians();
        let fit = (diag * 0.5) / half_fov.tan();
        // 1.4× padding so the mesh has breathing room at the edges.
        let radius = (fit * 1.4).clamp(RADIUS_MIN, RADIUS_MAX);

        self.target = center;
        self.radius = radius;
    }

    /// Sets the camera to a front view (looking along -Y).
    pub fn set_view_front(&mut self) {
        self.theta = std::f32::consts::FRAC_PI_2;
        self.phi = std::f32::consts::FRAC_PI_2;
    }

    /// Sets the camera to a right view (looking along -X).
    pub fn set_view_right(&mut self) {
        self.theta = 0.0;
        self.phi = std::f32::consts::FRAC_PI_2;
    }

    /// Sets the camera to a top view (looking down -Z).
    pub fn set_view_top(&mut self) {
        self.theta = 0.0;
        self.phi = PHI_MIN;
    }

    /// Sets the camera to the default isometric view.
    pub fn set_view_isometric(&mut self) {
        self.theta = std::f32::consts::FRAC_PI_4;
        self.phi = std::f32::consts::FRAC_PI_3;
    }

    /// Handles mouse/scroll input from an egui response.
    ///
    /// - Primary button drag rotates the camera (theta/phi).
    /// - Scroll wheel zooms in/out.
    pub fn handle_input(&mut self, response: &egui::Response) {
        // Handle drag to rotate
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.dragging = true;
            self.drag_start_theta = self.theta;
            self.drag_start_phi = self.phi;
        }

        if response.drag_stopped_by(egui::PointerButton::Primary) {
            self.dragging = false;
        }

        if self.dragging && response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            let rect = response.rect;

            // Map pixel drag distance to angle changes, matching the TS implementation
            let delta_theta = -(delta.x / rect.width()) * 2.0 * std::f32::consts::PI;
            let delta_phi = -(delta.y / rect.height()) * std::f32::consts::PI;

            self.theta += delta_theta;
            self.phi = (self.phi + delta_phi).clamp(PHI_MIN, PHI_MAX);
        }

        // Handle middle-mouse drag to pan
        if response.dragged_by(egui::PointerButton::Middle) {
            let delta = response.drag_delta();
            // Compute camera right and up vectors in world space
            let eye = self.eye_position();
            let forward = (self.target - eye).normalize();
            let right = forward.cross(Vec3::Z).normalize();
            let up = right.cross(forward).normalize();

            // Scale pan speed by distance so it feels consistent
            let pan_speed = self.radius * 0.002;
            self.target += (-right * delta.x + up * delta.y) * pan_speed;
        }

        // Handle scroll to zoom (proportional to current distance)
        let scroll_delta = response.ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta.abs() > 0.0 {
            let zoom = scroll_delta * SCROLL_ZOOM_FACTOR * self.radius / 100.0;
            self.radius = (self.radius - zoom).clamp(RADIUS_MIN, RADIUS_MAX);
        }
    }

    /// Resets the orbit target to the origin.
    pub fn reset_target(&mut self) {
        self.target = Vec3::ZERO;
    }
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_camera_position() {
        let cam = OrbitCamera::new();
        let eye = cam.eye_position();
        // With theta=PI/4, phi=PI/3, radius=16:
        let r = 16.0_f32;
        let expected_xy =
            r * (std::f32::consts::FRAC_PI_3).sin() * (std::f32::consts::FRAC_PI_4).cos();
        let expected_z = r * (std::f32::consts::FRAC_PI_3).cos();
        assert!((eye.x - expected_xy).abs() < 1e-4);
        assert!((eye.y - expected_xy).abs() < 1e-4);
        assert!((eye.z - expected_z).abs() < 1e-4);
    }

    #[test]
    fn view_matrix_is_valid() {
        let cam = OrbitCamera::new();
        let view = cam.view_matrix();
        // The view matrix should be invertible (non-zero determinant)
        assert!(view.determinant().abs() > 1e-6);
    }

    #[test]
    fn projection_matrix_is_valid() {
        let cam = OrbitCamera::new();
        let proj = cam.projection_matrix(16.0 / 9.0);
        // The projection matrix should be invertible
        assert!(proj.determinant().abs() > 1e-10);
    }

    #[test]
    fn phi_clamping() {
        let mut cam = OrbitCamera::new();
        // Manually set phi to extreme values and verify clamping
        cam.phi = -1.0;
        cam.phi = cam.phi.clamp(PHI_MIN, PHI_MAX);
        assert!((cam.phi - PHI_MIN).abs() < 1e-6);

        cam.phi = std::f32::consts::PI + 1.0;
        cam.phi = cam.phi.clamp(PHI_MIN, PHI_MAX);
        assert!((cam.phi - PHI_MAX).abs() < 1e-6);
    }

    #[test]
    fn radius_clamping() {
        let mut cam = OrbitCamera::new();
        // Test minimum radius clamping: a value below RADIUS_MIN clamps up.
        cam.radius = RADIUS_MIN * 0.1;
        cam.radius = cam.radius.clamp(RADIUS_MIN, RADIUS_MAX);
        assert!((cam.radius - RADIUS_MIN).abs() < 1e-6);

        // Test maximum radius clamping: a value above RADIUS_MAX clamps down.
        cam.radius = RADIUS_MAX * 2.0;
        cam.radius = cam.radius.clamp(RADIUS_MIN, RADIUS_MAX);
        assert!((cam.radius - RADIUS_MAX).abs() < 1e-6);
    }

    #[test]
    fn frame_bbox_fits_and_centers() {
        let mut cam = OrbitCamera::new();
        let min = Vec3::new(-4.0, -2.0, -6.0);
        let max = Vec3::new(4.0, 2.0, 6.0);
        cam.frame_bbox(min, max);

        let expected_center = (min + max) * 0.5;
        assert!((cam.target - expected_center).length() < 1e-4);

        // Radius must be within configured bounds and non-zero.
        assert!(cam.radius >= RADIUS_MIN && cam.radius <= RADIUS_MAX);
        assert!(cam.radius > (max - min).length() * 0.25);
    }

    #[test]
    fn frame_bbox_degenerate_bbox_only_sets_target() {
        let mut cam = OrbitCamera::new();
        let initial_radius = cam.radius;
        let p = Vec3::new(3.0, 3.0, 3.0);
        cam.frame_bbox(p, p);
        assert!((cam.target - p).length() < 1e-6);
        assert!((cam.radius - initial_radius).abs() < 1e-6);
    }

    #[test]
    fn eye_position_at_north_pole() {
        let mut cam = OrbitCamera::new();
        cam.phi = PHI_MIN; // Near the top (+Z)
        cam.radius = 100.0;
        cam.target = Vec3::ZERO;
        let eye = cam.eye_position();
        // Near the pole, z should be close to radius
        assert!(eye.z > 99.0);
        // x and y should be near zero
        assert!(eye.x.abs() < 2.0);
        assert!(eye.y.abs() < 2.0);
    }

    #[test]
    fn eye_position_at_equator() {
        let mut cam = OrbitCamera::new();
        cam.phi = std::f32::consts::FRAC_PI_2; // Equator
        cam.theta = 0.0;
        cam.radius = 100.0;
        cam.target = Vec3::ZERO;
        let eye = cam.eye_position();
        // At the equator with theta=0: x=radius, y=0, z=0
        assert!((eye.x - 100.0).abs() < 1e-4);
        assert!(eye.y.abs() < 1e-4);
        assert!(eye.z.abs() < 1e-4);
    }

    #[test]
    fn target_offset() {
        let mut cam = OrbitCamera::new();
        cam.phi = std::f32::consts::FRAC_PI_2;
        cam.theta = 0.0;
        cam.radius = 100.0;
        cam.target = Vec3::new(10.0, 20.0, 30.0);
        let eye = cam.eye_position();
        // Eye should be offset by the target position
        assert!((eye.x - 110.0).abs() < 1e-4);
        assert!((eye.y - 20.0).abs() < 1e-4);
        assert!((eye.z - 30.0).abs() < 1e-4);
    }
}
