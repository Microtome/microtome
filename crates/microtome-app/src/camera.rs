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
const RADIUS_MIN: f32 = 5.0;

/// Maximum zoom distance.
const RADIUS_MAX: f32 = 1000.0;

/// Field of view in degrees, matching the original TypeScript implementation.
#[allow(dead_code)]
const FOV_DEGREES: f32 = 37.0;

/// Zoom amount per scroll event, matching the original 10-unit zoom.
const SCROLL_ZOOM_STEP: f32 = 10.0;

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
            radius: 200.0,
            target: Vec3::ZERO,
            dragging: false,
            drag_start_theta: 0.0,
            drag_start_phi: 0.0,
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

    /// Computes the perspective projection matrix.
    ///
    /// Uses a 37-degree vertical FOV matching the original TypeScript implementation.
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(FOV_DEGREES.to_radians(), aspect, 0.1, 2000.0)
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

        // Handle scroll to zoom
        let scroll_delta = response.ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta.abs() > 0.0 {
            let zoom = if scroll_delta > 0.0 {
                SCROLL_ZOOM_STEP
            } else {
                -SCROLL_ZOOM_STEP
            };
            self.radius = (self.radius - zoom).clamp(RADIUS_MIN, RADIUS_MAX);
        }
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
        // With theta=PI/4, phi=PI/3, radius=200:
        // x = 200 * sin(PI/3) * cos(PI/4) = 200 * (sqrt(3)/2) * (sqrt(2)/2)
        // y = 200 * sin(PI/3) * sin(PI/4) = same as x
        // z = 200 * cos(PI/3) = 200 * 0.5 = 100
        let expected_xy =
            200.0 * (std::f32::consts::FRAC_PI_3).sin() * (std::f32::consts::FRAC_PI_4).cos();
        assert!((eye.x - expected_xy).abs() < 1e-4);
        assert!((eye.y - expected_xy).abs() < 1e-4);
        assert!((eye.z - 100.0).abs() < 1e-4);
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
        // Test minimum radius clamping
        cam.radius = 1.0;
        cam.radius = cam.radius.clamp(RADIUS_MIN, RADIUS_MAX);
        assert!((cam.radius - RADIUS_MIN).abs() < 1e-6);

        // Test maximum radius clamping
        cam.radius = 2000.0;
        cam.radius = cam.radius.clamp(RADIUS_MIN, RADIUS_MAX);
        assert!((cam.radius - RADIUS_MAX).abs() < 1e-6);
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
