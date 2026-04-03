//! Quadratic Error Function (QEF) solver for dual contouring.
//!
//! Solves for the optimal vertex position that minimizes the sum of squared
//! distances to a set of tangent planes (hermite data: point + normal pairs).
//! Uses a Jacobi SVD on the 3×3 normal equation matrix.

use glam::{Mat3, Vec3};

/// Number of Jacobi rotation sweeps for the SVD.
const SVD_NUM_SWEEPS: usize = 5;

/// Tolerance for pseudoinverse singular value cutoff.
const TINY_NUMBER: f32 = 1.0e-4;

/// Quadratic Error Function solver for isosurface vertex placement.
///
/// Accumulates hermite data (intersection point + surface normal pairs)
/// and solves for the position that minimizes the sum of squared distances
/// to all tangent planes.
#[derive(Debug, Clone)]
pub struct QefSolver {
    /// Normal equation matrix A^T A (symmetric, only upper triangle used).
    ata: Mat3,
    /// Normal equation vector A^T b.
    atb: Vec3,
    /// Diagonal of A^T c (for covariance computation).
    diag_atc: Vec3,
    /// b^T b scalar.
    btb: f32,
    /// Diagonal of c^T c (for covariance computation).
    diag_ctc: Vec3,
    /// Sum of all sample positions (for mass point computation).
    mass_point_sum: Vec3,
    /// Sum of all sample normals (for roughness computation).
    average_normal_sum: Vec3,
    /// Surface roughness metric (1 - |avg_normal| / count).
    roughness: f32,
    /// Number of hermite samples added.
    point_count: i32,
}

impl Default for QefSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl QefSolver {
    /// Creates a new empty QEF solver.
    pub fn new() -> Self {
        Self {
            ata: Mat3::ZERO,
            atb: Vec3::ZERO,
            diag_atc: Vec3::ZERO,
            btb: 0.0,
            diag_ctc: Vec3::ZERO,
            mass_point_sum: Vec3::ZERO,
            average_normal_sum: Vec3::ZERO,
            roughness: 0.0,
            point_count: 0,
        }
    }

    /// Resets the solver to its initial empty state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Copies all data from another solver.
    pub fn set(&mut self, other: &QefSolver) {
        self.clone_from(other);
        self.cal_roughness();
    }

    /// Accumulates another solver's data into this one.
    pub fn combine(&mut self, other: &QefSolver) {
        self.ata = add_symmetric(self.ata, other.ata);
        self.atb += other.atb;
        self.diag_atc += other.diag_atc;
        self.btb += other.btb;
        self.diag_ctc += other.diag_ctc;
        self.mass_point_sum += other.mass_point_sum;
        self.point_count += other.point_count;
        self.average_normal_sum += other.average_normal_sum;
        self.cal_roughness();
    }

    /// Subtracts another solver's data from this one.
    pub fn separate(&mut self, other: &QefSolver) {
        self.ata = sub_symmetric(self.ata, other.ata);
        self.atb -= other.atb;
        self.btb -= other.btb;
        self.mass_point_sum -= other.mass_point_sum;
        self.point_count -= other.point_count;
        self.average_normal_sum -= other.average_normal_sum;
        self.cal_roughness();
    }

    /// Adds a hermite data sample (surface intersection point + normal).
    pub fn add(&mut self, p: Vec3, n: Vec3) {
        // Accumulate A^T A (symmetric: only store upper triangle)
        let col0 = self.ata.col(0);
        let col1 = self.ata.col(1);
        let col2 = self.ata.col(2);

        self.ata = Mat3::from_cols(
            Vec3::new(col0.x + n.x * n.x, col0.y + n.x * n.y, col0.z + n.x * n.z),
            Vec3::new(col1.x, col1.y + n.y * n.y, col1.z + n.y * n.z),
            Vec3::new(col2.x, col2.y, col2.z + n.z * n.z),
        );

        let dotp = p.dot(n);
        let c = p * n;
        self.atb += n * dotp;
        self.diag_atc += n * c;
        self.btb += dotp * dotp;
        self.diag_ctc += c * c;
        self.point_count += 1;
        self.mass_point_sum += p;
        self.average_normal_sum += n;
    }

    /// Solves the QEF, returning the optimal position and the error.
    ///
    /// The solve uses a mass-point-centered coordinate system for numerical
    /// stability, then applies a Lagrange multiplier via SVD pseudoinverse.
    pub fn solve(&mut self) -> (Vec3, f32) {
        if self.point_count <= 0 {
            return (Vec3::ZERO, 0.0);
        }
        self.cal_roughness();
        let mass_point = self.mass_point_sum / self.point_count as f32;
        let shifted_atb = self.atb - svd_vmul_sym(self.ata, mass_point);
        let mut hermite_p = svd_solve_ata_atb(self.ata, shifted_atb);
        hermite_p += mass_point;
        let error = qef_calc_error(self.ata, hermite_p, self.atb, self.btb);
        (hermite_p, error)
    }

    /// Computes the roughness metric (1 - |avg_normal| / count).
    pub fn cal_roughness(&mut self) {
        if self.point_count > 0 {
            self.roughness = 1.0 - self.average_normal_sum.length() / self.point_count as f32;
        }
    }

    /// Returns the QEF error at a given position.
    pub fn get_error_at(&self, p: Vec3) -> f32 {
        qef_calc_error(self.ata, p, self.atb, self.btb)
    }

    /// Returns the QEF error at the A^T b point (default).
    pub fn get_error(&self) -> f32 {
        qef_calc_error(self.ata, self.atb, self.atb, self.btb)
    }

    /// Returns the covariance at a given position.
    pub fn get_variance(&self, p: Vec3) -> Vec3 {
        qef_calc_co_variance(self.ata, p, self.diag_atc, self.diag_ctc)
    }

    /// Returns the current point count.
    pub fn point_count(&self) -> i32 {
        self.point_count
    }

    /// Returns the roughness metric.
    pub fn roughness(&self) -> f32 {
        self.roughness
    }

    /// Returns the mass point (average of all sample positions).
    pub fn mass_point(&self) -> Vec3 {
        if self.point_count > 0 {
            self.mass_point_sum / self.point_count as f32
        } else {
            Vec3::ZERO
        }
    }
}

// ---------------------------------------------------------------------------
// SVD helpers (Jacobi eigenvalue solver for 3x3 symmetric matrices)
// ---------------------------------------------------------------------------

/// Multiplies a symmetric 3x3 matrix by a vector.
/// Only uses the upper triangle of `a`.
fn svd_vmul_sym(a: Mat3, v: Vec3) -> Vec3 {
    let a00 = a.col(0).x;
    let a01 = a.col(0).y;
    let a02 = a.col(0).z;
    let a11 = a.col(1).y;
    let a12 = a.col(1).z;
    let a22 = a.col(2).z;
    Vec3::new(
        a00 * v.x + a01 * v.y + a02 * v.z,
        a01 * v.x + a11 * v.y + a12 * v.z,
        a02 * v.x + a12 * v.y + a22 * v.z,
    )
}

/// Computes the QEF error: x^T ATA x - 2 x^T ATb + btb.
fn qef_calc_error(ata: Mat3, x: Vec3, atb: Vec3, btb: f32) -> f32 {
    let atax = svd_vmul_sym(ata, x);
    x.dot(atax) - 2.0 * x.dot(atb) + btb
}

/// Computes the covariance at a point.
fn qef_calc_co_variance(ata: Mat3, x: Vec3, diag_atc: Vec3, diag_ctc: Vec3) -> Vec3 {
    let diag_ata = Vec3::new(ata.col(0).x, ata.col(1).y, ata.col(2).z);
    x * diag_ata * x - 2.0 * (x * diag_atc) + diag_ctc
}

/// Applies a Givens rotation in the XY plane.
fn svd_rotate_xy(x: &mut f32, y: &mut f32, c: f32, s: f32) {
    let u = *x;
    let v = *y;
    *x = c * u - s * v;
    *y = s * u + c * v;
}

/// Applies a Givens rotation for a quadratic form.
fn svd_rotateq_xy(x: &mut f32, y: &mut f32, a: f32, c: f32, s: f32) {
    let cc = c * c;
    let ss = s * s;
    let mx = 2.0 * c * s * a;
    let u = *x;
    let v = *y;
    *x = cc * u - mx + ss * v;
    *y = ss * u + mx + cc * v;
}

/// Safe reciprocal for pseudoinverse (returns 0 if near-singular).
fn svd_invdet(x: f32, tol: f32) -> f32 {
    if x.abs() < tol || (1.0 / x).abs() < tol {
        0.0
    } else {
        1.0 / x
    }
}

/// Computes the pseudoinverse from SVD eigenvalues and eigenvectors.
///
/// Given V and sigma from V^T A V = diag(sigma), computes V diag(1/sigma) V^T.
fn svd_pseudoinverse(sigma: Vec3, v: Mat3) -> Mat3 {
    let d0 = svd_invdet(sigma.x, TINY_NUMBER);
    let d1 = svd_invdet(sigma.y, TINY_NUMBER);
    let d2 = svd_invdet(sigma.z, TINY_NUMBER);

    // v is column-major in glam: v.col(j) = j-th column
    let v0 = v.col(0); // column 0
    let v1 = v.col(1); // column 1
    let v2 = v.col(2); // column 2

    // Compute V * diag(d) * V^T element by element
    // o[row][col] = sum_k v[row][k] * dk * v[col][k]
    // In glam column-major: o.col(col)[row]
    let col0 = Vec3::new(
        v0.x * d0 * v0.x + v1.x * d1 * v1.x + v2.x * d2 * v2.x,
        v0.x * d0 * v0.y + v1.x * d1 * v1.y + v2.x * d2 * v2.y,
        v0.x * d0 * v0.z + v1.x * d1 * v1.z + v2.x * d2 * v2.z,
    );
    let col1 = Vec3::new(
        v0.y * d0 * v0.x + v1.y * d1 * v1.x + v2.y * d2 * v2.x,
        v0.y * d0 * v0.y + v1.y * d1 * v1.y + v2.y * d2 * v2.y,
        v0.y * d0 * v0.z + v1.y * d1 * v1.z + v2.y * d2 * v2.z,
    );
    let col2 = Vec3::new(
        v0.z * d0 * v0.x + v1.z * d1 * v1.x + v2.z * d2 * v2.x,
        v0.z * d0 * v0.y + v1.z * d1 * v1.y + v2.z * d2 * v2.y,
        v0.z * d0 * v0.z + v1.z * d1 * v1.z + v2.z * d2 * v2.z,
    );

    Mat3::from_cols(col0, col1, col2)
}

/// Computes Givens coefficients for a symmetric 2x2 rotation.
fn givens_coeffs_sym(a_pp: f32, a_pq: f32, a_qq: f32) -> (f32, f32) {
    if a_pq == 0.0 {
        return (1.0, 0.0);
    }
    let tau = (a_qq - a_pp) / (2.0 * a_pq);
    let stt = (1.0 + tau * tau).sqrt();
    let tan = 1.0 / if tau >= 0.0 { tau + stt } else { tau - stt };
    let c = 1.0 / (1.0 + tan * tan).sqrt();
    let s = tan * c;
    (c, s)
}

/// Helper to get a mutable reference to a matrix element by (row, col).
///
/// glam Mat3 is column-major: mat.col(col)[row].
/// This helper reads/writes via columns.
fn mat3_get(m: &Mat3, row: usize, col: usize) -> f32 {
    m.col(col)[row]
}

fn mat3_set(m: &mut Mat3, row: usize, col: usize, val: f32) {
    match col {
        0 => {
            let mut c = m.col(0);
            c[row] = val;
            *m = Mat3::from_cols(c, m.col(1), m.col(2));
        }
        1 => {
            let mut c = m.col(1);
            c[row] = val;
            *m = Mat3::from_cols(m.col(0), c, m.col(2));
        }
        2 => {
            let mut c = m.col(2);
            c[row] = val;
            *m = Mat3::from_cols(m.col(0), m.col(1), c);
        }
        _ => {}
    }
}

/// Applies one Jacobi rotation to diagonalize the (a, b) element.
fn svd_rotate(vtav: &mut Mat3, v: &mut Mat3, a: usize, b: usize) {
    if mat3_get(vtav, a, b) == 0.0 {
        return;
    }

    let (c, s) = givens_coeffs_sym(
        mat3_get(vtav, a, a),
        mat3_get(vtav, a, b),
        mat3_get(vtav, b, b),
    );

    let mut x = mat3_get(vtav, a, a);
    let mut y = mat3_get(vtav, b, b);
    svd_rotateq_xy(&mut x, &mut y, mat3_get(vtav, a, b), c, s);
    mat3_set(vtav, a, a, x);
    mat3_set(vtav, b, b, y);

    x = mat3_get(vtav, 0, 3 - b);
    y = mat3_get(vtav, 1 - a, 2);
    svd_rotate_xy(&mut x, &mut y, c, s);
    mat3_set(vtav, 0, 3 - b, x);
    mat3_set(vtav, 1 - a, 2, y);

    mat3_set(vtav, a, b, 0.0);

    // Rotate columns of V
    for row in 0..3 {
        x = mat3_get(v, row, a);
        y = mat3_get(v, row, b);
        svd_rotate_xy(&mut x, &mut y, c, s);
        mat3_set(v, row, a, x);
        mat3_set(v, row, b, y);
    }
}

/// Solves the symmetric eigenvalue problem via Jacobi rotations.
fn svd_solve_sym(mut vtav: Mat3) -> (Vec3, Mat3) {
    let mut v = Mat3::IDENTITY;
    for _ in 0..SVD_NUM_SWEEPS {
        svd_rotate(&mut vtav, &mut v, 0, 1);
        svd_rotate(&mut vtav, &mut v, 0, 2);
        svd_rotate(&mut vtav, &mut v, 1, 2);
    }
    let sigma = Vec3::new(
        mat3_get(&vtav, 0, 0),
        mat3_get(&vtav, 1, 1),
        mat3_get(&vtav, 2, 2),
    );
    (sigma, v)
}

/// Solves A^T A x = A^T b via SVD pseudoinverse.
fn svd_solve_ata_atb(ata: Mat3, atb: Vec3) -> Vec3 {
    let (sigma, v) = svd_solve_sym(ata);
    let vinv = svd_pseudoinverse(sigma, v);
    vinv * atb
}

/// Adds two symmetric matrices (only upper triangle).
fn add_symmetric(a: Mat3, b: Mat3) -> Mat3 {
    Mat3::from_cols(
        Vec3::new(
            a.col(0).x + b.col(0).x,
            a.col(0).y + b.col(0).y,
            a.col(0).z + b.col(0).z,
        ),
        Vec3::new(a.col(1).x, a.col(1).y + b.col(1).y, a.col(1).z + b.col(1).z),
        Vec3::new(a.col(2).x, a.col(2).y, a.col(2).z + b.col(2).z),
    )
}

/// Subtracts two symmetric matrices (only upper triangle).
fn sub_symmetric(a: Mat3, b: Mat3) -> Mat3 {
    Mat3::from_cols(
        Vec3::new(
            a.col(0).x - b.col(0).x,
            a.col(0).y - b.col(0).y,
            a.col(0).z - b.col(0).z,
        ),
        Vec3::new(a.col(1).x, a.col(1).y - b.col(1).y, a.col(1).z - b.col(1).z),
        Vec3::new(a.col(2).x, a.col(2).y, a.col(2).z - b.col(2).z),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_solver_returns_zero() {
        let mut solver = QefSolver::new();
        let (pos, err) = solver.solve();
        assert_eq!(pos, Vec3::ZERO);
        assert_eq!(err, 0.0);
    }

    #[test]
    fn single_plane_solve() {
        // A single plane at z=1 with normal (0,0,1).
        // The QEF minimum is at the plane point.
        let mut solver = QefSolver::new();
        solver.add(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
        let (pos, err) = solver.solve();
        assert!((pos.z - 1.0).abs() < 1e-3, "pos.z = {}", pos.z);
        assert!(err < 1e-3, "err = {err}");
    }

    #[test]
    fn three_orthogonal_planes() {
        // Three orthogonal planes intersecting at (1, 2, 3)
        let mut solver = QefSolver::new();
        solver.add(Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        solver.add(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        solver.add(Vec3::new(0.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 1.0));
        let (pos, err) = solver.solve();
        assert!((pos.x - 1.0).abs() < 1e-3, "pos.x = {}", pos.x);
        assert!((pos.y - 2.0).abs() < 1e-3, "pos.y = {}", pos.y);
        assert!((pos.z - 3.0).abs() < 1e-3, "pos.z = {}", pos.z);
        assert!(err < 1e-3, "err = {err}");
    }

    #[test]
    fn combine_separate_round_trip() {
        let mut a = QefSolver::new();
        a.add(Vec3::new(1.0, 0.0, 0.0), Vec3::X);
        a.add(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);

        let mut b = QefSolver::new();
        b.add(Vec3::new(0.0, 0.0, 1.0), Vec3::Z);

        let mut combined = a.clone();
        combined.combine(&b);
        combined.separate(&b);

        // After combine+separate, should match original 'a'
        let (pos_a, _) = a.solve();
        let (pos_c, _) = combined.solve();
        assert!((pos_a - pos_c).length() < 1e-3);
    }

    #[test]
    fn get_error_at_solution_is_low() {
        let mut solver = QefSolver::new();
        solver.add(Vec3::new(1.0, 0.0, 0.0), Vec3::X);
        solver.add(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
        solver.add(Vec3::new(0.0, 0.0, 1.0), Vec3::Z);
        let (pos, _) = solver.solve();
        let err = solver.get_error_at(pos);
        assert!(err < 1e-2, "err = {err}");
    }
}
