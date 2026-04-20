// Quadratic Error Function (QEF) solver for dual contouring.
//
// Ported line-by-line from KdtreeISO-master/src/KdtreeISO/lib/Qef.cpp
// and KdtreeISO-master/src/KdtreeISO/include/Qef.h
//
// Uses [[f32; 3]; 3] for matrices indexed as [col][row] to match
// GLM's column-major mat3x3 convention exactly.

use glam::Vec3;

// #define SVD_NUM_SWEEPS 5
const SVD_NUM_SWEEPS: usize = 5;

// Floor for the SVD pseudoinverse: any singular value smaller than
// `MIN_SVD_FLOOR` in absolute terms is treated as zero, regardless of
// the relative threshold below.
const MIN_SVD_FLOOR: f32 = 1.0e-12;

/// Relative-to-`σ_max` truncation threshold for the SVD pseudoinverse.
/// The C++ source used an absolute `Tiny_Number = 1e-4`, which is
/// well-behaved for clean SDF inputs (normals at corners are exactly
/// the gradient — `ATA` is well-conditioned) but breaks badly for
/// mesh-derived normals: each constraint is a per-triangle face
/// normal, so a curved feature rasterised onto the grid produces an
/// `ATA` with several genuinely-tiny but-not-zero singular values.
/// An absolute threshold leaves those singular values un-truncated;
/// the pseudoinverse then amplifies their inverse into the QEF
/// solution, throwing vertices far past the surface — visible as
/// severe chips and spikes on gear teeth and other curved-but-
/// detailed features.
///
/// `1e-2` (relative to `σ_max`) is a conservative truncation: it
/// scales naturally with the matrix size and reins in the worst
/// pseudoinverse blow-ups without flattening lightly-curved regions
/// the way the more aggressive `0.1` setting did.
const SVD_RELATIVE_TOL: f32 = 1.0e-2;

/// 3x3 matrix stored as `[col][row]` to match GLM's `mat3x3[col][row]`.
type Mat3x3 = [[f32; 3]; 3];

/// Returns a zero 3x3 matrix (matches `glm::mat4(0.0)` → `glm::mat3x3`).
fn mat3_zero() -> Mat3x3 {
    [[0.0; 3]; 3]
}

/// Returns a 3x3 identity matrix.
fn mat3_identity() -> Mat3x3 {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

/// Matrix-vector multiply: `m * v` (matches GLM `mat3 * vec3`).
fn mat3_mul_vec3(m: &Mat3x3, v: Vec3) -> Vec3 {
    // result[row] = sum_col m[col][row] * v[col]
    Vec3::new(
        m[0][0] * v.x + m[1][0] * v.y + m[2][0] * v.z,
        m[0][1] * v.x + m[1][1] * v.y + m[2][1] * v.z,
        m[0][2] * v.x + m[1][2] * v.y + m[2][2] * v.z,
    )
}

// ============================================================================
// Qef.h — struct QefSolver
// ============================================================================

// struct QefSolver {
//   glm::mat3x3 ATA;
//   glm::fvec3 ATb;
//   glm::fvec3 diag_ATc;
//   float btb;
//   glm::fvec3 diag_ctc;
//   glm::fvec3 massPointSum;
//   glm::fvec3 averageNormalSum;
//   float roughness;
//   int pointCount;
//   QefSolver()
//     : ATA(glm::mat4(0.0)),
//       ATb(glm::fvec3(0.0)),
//       diag_ATc(0.0),
//       btb(0.f),
//       diag_ctc(glm::fvec3(0.f)),
//       massPointSum(glm::fvec3(0.f)),
//       averageNormalSum(glm::fvec3(0.f)),
//       roughness(0.f),
//       pointCount(0) {}
// };
#[derive(Debug, Clone)]
pub struct QefSolver {
    ata: Mat3x3,
    atb: Vec3,
    diag_atc: Vec3,
    btb: f32,
    diag_ctc: Vec3,
    mass_point_sum: Vec3,
    average_normal_sum: Vec3,
    roughness: f32,
    point_count: i32,
}

impl Default for QefSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl QefSolver {
    pub fn new() -> Self {
        Self {
            ata: mat3_zero(),
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

    // ========================================================================
    // Qef.cpp — void QefSolver::reset()
    // ========================================================================

    // void QefSolver::reset() {
    //   ATA = glm::mat4(0.f);
    //   ATb = glm::fvec3(0.f);
    //   btb = 0.f;
    //   massPointSum = glm::fvec3(0.f);
    //   averageNormalSum = glm::fvec3(0.f);
    //   pointCount = 0;
    // }
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    // ========================================================================
    // Qef.cpp — void QefSolver::set(const QefSolver &other)
    // ========================================================================

    // void QefSolver::set(const QefSolver &other) {
    //   ATA[0][0] = other.ATA[0][0];
    //   ATA[1][1] = other.ATA[1][1];
    //   ATA[2][2] = other.ATA[2][2];
    //   ATA[0][1] = other.ATA[0][1];
    //   ATA[0][2] = other.ATA[0][2];
    //   ATA[1][2] = other.ATA[1][2];
    //   ATb = other.ATb;
    //   btb = other.btb;
    //   massPointSum = other.massPointSum;
    //   averageNormalSum = other.averageNormalSum;
    //   pointCount = other.pointCount;
    //   calRoughness();
    // }
    pub fn set(&mut self, other: &QefSolver) {
        self.ata[0][0] = other.ata[0][0];
        self.ata[1][1] = other.ata[1][1];
        self.ata[2][2] = other.ata[2][2];
        self.ata[0][1] = other.ata[0][1];
        self.ata[0][2] = other.ata[0][2];
        self.ata[1][2] = other.ata[1][2];
        self.atb = other.atb;
        self.btb = other.btb;
        self.mass_point_sum = other.mass_point_sum;
        self.average_normal_sum = other.average_normal_sum;
        self.point_count = other.point_count;
        self.cal_roughness();
    }

    // ========================================================================
    // Qef.cpp — void QefSolver::combine(const QefSolver &other)
    // ========================================================================

    // void QefSolver::combine(const QefSolver &other) {
    //   ATA[0][0] += other.ATA[0][0];
    //   ATA[1][1] += other.ATA[1][1];
    //   ATA[2][2] += other.ATA[2][2];
    //   ATA[0][1] += other.ATA[0][1];
    //   ATA[0][2] += other.ATA[0][2];
    //   ATA[1][2] += other.ATA[1][2];
    //   ATb += other.ATb;
    //   diag_ATc += other.diag_ATc;
    //   btb += other.btb;
    //   diag_ctc += other.diag_ctc;
    //   massPointSum += other.massPointSum;
    //   pointCount += other.pointCount;
    //   averageNormalSum += other.averageNormalSum;
    //   calRoughness();
    // }
    pub fn combine(&mut self, other: &QefSolver) {
        self.ata[0][0] += other.ata[0][0];
        self.ata[1][1] += other.ata[1][1];
        self.ata[2][2] += other.ata[2][2];
        self.ata[0][1] += other.ata[0][1];
        self.ata[0][2] += other.ata[0][2];
        self.ata[1][2] += other.ata[1][2];
        self.atb += other.atb;
        self.diag_atc += other.diag_atc;
        self.btb += other.btb;
        self.diag_ctc += other.diag_ctc;
        self.mass_point_sum += other.mass_point_sum;
        self.point_count += other.point_count;
        self.average_normal_sum += other.average_normal_sum;
        self.cal_roughness();
    }

    // ========================================================================
    // Qef.cpp — void QefSolver::separate(const QefSolver &other)
    // ========================================================================

    // void QefSolver::separate(const QefSolver &other) {
    //   ATA[0][0] -= other.ATA[0][0];
    //   ATA[1][1] -= other.ATA[1][1];
    //   ATA[2][2] -= other.ATA[2][2];
    //   ATA[0][1] -= other.ATA[0][1];
    //   ATA[0][2] -= other.ATA[0][2];
    //   ATA[1][2] -= other.ATA[1][2];
    //   ATb -= other.ATb;
    //   btb -= other.btb;
    //   massPointSum -= other.massPointSum;
    //   pointCount -= other.pointCount;
    //   averageNormalSum -= other.averageNormalSum;
    //   calRoughness();
    // }
    pub fn separate(&mut self, other: &QefSolver) {
        self.ata[0][0] -= other.ata[0][0];
        self.ata[1][1] -= other.ata[1][1];
        self.ata[2][2] -= other.ata[2][2];
        self.ata[0][1] -= other.ata[0][1];
        self.ata[0][2] -= other.ata[0][2];
        self.ata[1][2] -= other.ata[1][2];
        self.atb -= other.atb;
        self.btb -= other.btb;
        self.mass_point_sum -= other.mass_point_sum;
        self.point_count -= other.point_count;
        self.average_normal_sum -= other.average_normal_sum;
        self.cal_roughness();
    }

    // ========================================================================
    // Qef.cpp — void QefSolver::add(const glm::fvec3 &p, const glm::fvec3 &n)
    // ========================================================================

    // void QefSolver::add(const glm::fvec3 &p, const glm::fvec3 &n) {
    //   ATA[0][0] += n.x * n.x;
    //   ATA[0][1] += n.x * n.y;
    //   ATA[0][2] += n.x * n.z;
    //   ATA[1][1] += n.y * n.y;
    //   ATA[1][2] += n.y * n.z;
    //   ATA[2][2] += n.z * n.z;
    //   float dotp = glm::dot(p, n);
    //   glm::fvec3 c = p * n;
    //   ATb += n * dotp;
    //   diag_ATc += n * c;
    //   btb += dotp * dotp;
    //   diag_ctc += c * c;
    //   pointCount++;
    //   massPointSum += p;
    //   averageNormalSum += n;
    // }
    pub fn add(&mut self, p: Vec3, n: Vec3) {
        self.ata[0][0] += n.x * n.x;
        self.ata[0][1] += n.x * n.y;
        self.ata[0][2] += n.x * n.z;
        self.ata[1][1] += n.y * n.y;
        self.ata[1][2] += n.y * n.z;
        self.ata[2][2] += n.z * n.z;
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

    // ========================================================================
    // Qef.cpp — void QefSolver::solve(...)
    // ========================================================================

    // void QefSolver::solve(glm::fvec3 &hermiteP, float &error) {
    //   if (pointCount > 0) {
    //     calRoughness();
    //     glm::fvec3 massPoint = massPointSum / (float)pointCount;
    //     glm::fvec3 _ATb = ATb - svd_vmul_sym(ATA, massPoint);
    //     hermiteP = svd_solve_ATA_ATb(ATA, _ATb);
    //     hermiteP += massPoint;
    //     error = qef_calc_error(ATA, hermiteP, ATb, btb);
    //     assert(!isnan(hermiteP.x));
    //   }
    // }
    pub fn solve(&mut self, hermite_p: &mut Vec3, error: &mut f32) {
        // C++: when pointCount==0, hermiteP and error are LEFT UNCHANGED.
        if self.point_count <= 0 {
            return;
        }
        self.cal_roughness();
        let mass_point = self.mass_point_sum / self.point_count as f32;
        let shifted_atb = self.atb - svd_vmul_sym(&self.ata, mass_point);
        *hermite_p = svd_solve_ata_atb(&self.ata, shifted_atb);
        *hermite_p += mass_point;
        *error = qef_calc_error(&self.ata, *hermite_p, self.atb, self.btb);
    }

    // ========================================================================
    // Qef.cpp — void QefSolver::calRoughness()
    // ========================================================================

    // void QefSolver::calRoughness() {
    //   roughness = 1.f - glm::length(averageNormalSum) / (float)pointCount;
    // }
    pub fn cal_roughness(&mut self) {
        if self.point_count > 0 {
            self.roughness = 1.0 - self.average_normal_sum.length() / self.point_count as f32;
        }
    }

    // ========================================================================
    // Qef.cpp — float QefSolver::getError(const glm::fvec3 &p)
    // ========================================================================

    // float QefSolver::getError(const glm::fvec3 &p) {
    //   return qef_calc_error(ATA, p, ATb, btb);
    // }
    pub fn get_error_at(&self, p: Vec3) -> f32 {
        qef_calc_error(&self.ata, p, self.atb, self.btb)
    }

    // float QefSolver::getError() {
    //   return qef_calc_error(ATA, ATb, ATb, btb);
    // }
    pub fn get_error(&self) -> f32 {
        qef_calc_error(&self.ata, self.atb, self.atb, self.btb)
    }

    // ========================================================================
    // Qef.cpp — glm::fvec3 QefSolver::getVariance(const glm::fvec3 &p)
    // ========================================================================

    // glm::fvec3 QefSolver::getVariance(const glm::fvec3 &p) {
    //   auto v = qef_calc_co_variance(ATA, p, diag_ATc, diag_ctc);
    //   return v;
    // }
    pub fn get_variance(&self, p: Vec3) -> Vec3 {
        qef_calc_co_variance(&self.ata, p, self.diag_atc, self.diag_ctc)
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

// ============================================================================
// Qef.cpp — SVD helper functions
// ============================================================================

// glm::fvec3 svd_vmul_sym(const glm::mat3x3 &a, const glm::fvec3 &v) {
//   return glm::fvec3(
//     (a[0][0] * v.x) + (a[0][1] * v.y) + (a[0][2] * v.z),
//     (a[0][1] * v.x) + (a[1][1] * v.y) + (a[1][2] * v.z),
//     (a[0][2] * v.x) + (a[1][2] * v.y) + (a[2][2] * v.z));
// }
fn svd_vmul_sym(a: &Mat3x3, v: Vec3) -> Vec3 {
    Vec3::new(
        (a[0][0] * v.x) + (a[0][1] * v.y) + (a[0][2] * v.z),
        (a[0][1] * v.x) + (a[1][1] * v.y) + (a[1][2] * v.z),
        (a[0][2] * v.x) + (a[1][2] * v.y) + (a[2][2] * v.z),
    )
}

// float qef_calc_error(const glm::mat3x3 &ATA, const glm::fvec3 &x,
//                      const glm::fvec3 &ATb, const float btb) {
//   glm::fvec3 atax = svd_vmul_sym(ATA, x);
//   return glm::dot(x, atax) - 2 * glm::dot(x, ATb) + btb;
// }
fn qef_calc_error(ata: &Mat3x3, x: Vec3, atb: Vec3, btb: f32) -> f32 {
    let atax = svd_vmul_sym(ata, x);
    x.dot(atax) - 2.0 * x.dot(atb) + btb
}

// glm::fvec3 qef_calc_co_variance(const glm::mat3x3 &ATA, const glm::fvec3 &x,
//                                  const glm::fvec3 &diag_ATc, const glm::fvec3 &diag_ctc) {
//   return x * diag(ATA) * x - 2.f * (x * diag_ATc) + diag_ctc;
// }
fn qef_calc_co_variance(ata: &Mat3x3, x: Vec3, diag_atc: Vec3, diag_ctc: Vec3) -> Vec3 {
    // diag(ATA) = Vec3(ATA[0][0], ATA[1][1], ATA[2][2])
    let diag_ata = Vec3::new(ata[0][0], ata[1][1], ata[2][2]);
    x * diag_ata * x - 2.0 * (x * diag_atc) + diag_ctc
}

// void svd_rotate_xy(float &x, float &y, float c, float s) {
//   float u = x;
//   float v = y;
//   x = c * u - s * v;
//   y = s * u + c * v;
// }
fn svd_rotate_xy(x: &mut f32, y: &mut f32, c: f32, s: f32) {
    let u = *x;
    let v = *y;
    *x = c * u - s * v;
    *y = s * u + c * v;
}

// void svd_rotateq_xy(float &x, float &y, float a, float c, float s) {
//   float cc = c * c;
//   float ss = s * s;
//   float mx = 2.0f * c * s * a;
//   float u = x;
//   float v = y;
//   x = cc * u - mx + ss * v;
//   y = ss * u + mx + cc * v;
// }
fn svd_rotateq_xy(x: &mut f32, y: &mut f32, a: f32, c: f32, s: f32) {
    let cc = c * c;
    let ss = s * s;
    let mx = 2.0 * c * s * a;
    let u = *x;
    let v = *y;
    *x = cc * u - mx + ss * v;
    *y = ss * u + mx + cc * v;
}

/// Pseudoinverse helper: returns `1/x`, or 0 when `|x|` falls below
/// `tol`. The C++ source also short-circuits to 0 when `|1/x| < tol`
/// (i.e. `|x| > 1/tol`); we keep that guard at `MIN_SVD_FLOOR` so
/// extremely large singular values (whose inverses would underflow)
/// don't pollute the result.
fn svd_invdet(x: f32, tol: f32) -> f32 {
    if x.abs() < tol || (1.0 / x).abs() < MIN_SVD_FLOOR {
        0.0
    } else {
        1.0 / x
    }
}

// void svd_pseudoinverse(glm::mat3x3 &o, const glm::fvec3 &sigma, const glm::mat3x3 &v) {
//   float d0 = svd_invdet(sigma[0], Tiny_Number);
//   float d1 = svd_invdet(sigma[1], Tiny_Number);
//   float d2 = svd_invdet(sigma[2], Tiny_Number);
//   o = glm::mat3(v[0][0] * d0 * v[0][0] + v[0][1] * d1 * v[0][1] + v[0][2] * d2 * v[0][2],
//                 v[0][0] * d0 * v[1][0] + v[0][1] * d1 * v[1][1] + v[0][2] * d2 * v[1][2],
//                 v[0][0] * d0 * v[2][0] + v[0][1] * d1 * v[2][1] + v[0][2] * d2 * v[2][2],
//                 v[1][0] * d0 * v[0][0] + v[1][1] * d1 * v[0][1] + v[1][2] * d2 * v[0][2],
//                 v[1][0] * d0 * v[1][0] + v[1][1] * d1 * v[1][1] + v[1][2] * d2 * v[1][2],
//                 v[1][0] * d0 * v[2][0] + v[1][1] * d1 * v[2][1] + v[1][2] * d2 * v[2][2],
//                 v[2][0] * d0 * v[0][0] + v[2][1] * d1 * v[0][1] + v[2][2] * d2 * v[0][2],
//                 v[2][0] * d0 * v[1][0] + v[2][1] * d1 * v[1][1] + v[2][2] * d2 * v[1][2],
//                 v[2][0] * d0 * v[2][0] + v[2][1] * d1 * v[2][1] + v[2][2] * d2 * v[2][2]);
// }
fn svd_pseudoinverse(sigma: Vec3, v: &Mat3x3) -> Mat3x3 {
    let sigma_max = sigma.x.abs().max(sigma.y.abs()).max(sigma.z.abs());
    let tol = (sigma_max * SVD_RELATIVE_TOL).max(MIN_SVD_FLOOR);
    let d0 = svd_invdet(sigma.x, tol);
    let d1 = svd_invdet(sigma.y, tol);
    let d2 = svd_invdet(sigma.z, tol);
    // glm::mat3 constructor takes args in column-major order:
    // (col0_row0, col0_row1, col0_row2, col1_row0, col1_row1, ...)
    // Our Mat3x3 is [col][row], so we fill it directly.
    [
        // column 0
        [
            v[0][0] * d0 * v[0][0] + v[0][1] * d1 * v[0][1] + v[0][2] * d2 * v[0][2],
            v[0][0] * d0 * v[1][0] + v[0][1] * d1 * v[1][1] + v[0][2] * d2 * v[1][2],
            v[0][0] * d0 * v[2][0] + v[0][1] * d1 * v[2][1] + v[0][2] * d2 * v[2][2],
        ],
        // column 1
        [
            v[1][0] * d0 * v[0][0] + v[1][1] * d1 * v[0][1] + v[1][2] * d2 * v[0][2],
            v[1][0] * d0 * v[1][0] + v[1][1] * d1 * v[1][1] + v[1][2] * d2 * v[1][2],
            v[1][0] * d0 * v[2][0] + v[1][1] * d1 * v[2][1] + v[1][2] * d2 * v[2][2],
        ],
        // column 2
        [
            v[2][0] * d0 * v[0][0] + v[2][1] * d1 * v[0][1] + v[2][2] * d2 * v[0][2],
            v[2][0] * d0 * v[1][0] + v[2][1] * d1 * v[1][1] + v[2][2] * d2 * v[1][2],
            v[2][0] * d0 * v[2][0] + v[2][1] * d1 * v[2][1] + v[2][2] * d2 * v[2][2],
        ],
    ]
}

// void givens_coeffs_sym(float a_pp, float a_pq, float a_qq, float &c, float &s) {
//   if (a_pq == 0.0f) {
//     c = 1.0f;
//     s = 0.0f;
//     return;
//   }
//   float tau = (a_qq - a_pp) / (2.0f * a_pq);
//   float stt = sqrt(1.0f + tau * tau);
//   float tan = 1.0f / (tau >= 0.0f ? tau + stt : tau - stt);
//   c = 1.0f / sqrt(1.0f + tan * tan);
//   s = tan * c;
// }
fn givens_coeffs_sym(a_pp: f32, a_pq: f32, a_qq: f32) -> (f32, f32) {
    if a_pq == 0.0 {
        return (1.0, 0.0);
    }
    let tau = (a_qq - a_pp) / (2.0 * a_pq);
    // C++ calls sqrt() (double precision) on float args — the float
    // promotes to f64, sqrt is computed in f64, then truncated back
    // to f32. We replicate this to match the reference exactly.
    let stt = ((1.0_f64 + (tau as f64) * (tau as f64)).sqrt()) as f32;
    let tan = 1.0 / if tau >= 0.0 { tau + stt } else { tau - stt };
    let c = ((1.0_f64 + (tan as f64) * (tan as f64)).sqrt()) as f32;
    let c = 1.0 / c;
    let s = tan * c;
    (c, s)
}

// void svd_rotate(glm::mat3x3 &vtav, glm::mat3x3 &v, int a, int b) {
//   if (vtav[a][b] == 0.0)
//     return;
//   float c = 0.f, s = 0.f;
//   givens_coeffs_sym(vtav[a][a], vtav[a][b], vtav[b][b], c, s);
//   float x, y;
//   x = vtav[a][a];
//   y = vtav[b][b];
//   svd_rotateq_xy(x, y, vtav[a][b], c, s);
//   vtav[a][a] = x;
//   vtav[b][b] = y;
//   x = vtav[0][3 - b];
//   y = vtav[1 - a][2];
//   svd_rotate_xy(x, y, c, s);
//   vtav[0][3 - b] = x;
//   vtav[1 - a][2] = y;
//   vtav[a][b] = 0.0f;
//   x = v[0][a]; y = v[0][b];
//   svd_rotate_xy(x, y, c, s);
//   v[0][a] = x; v[0][b] = y;
//   x = v[1][a]; y = v[1][b];
//   svd_rotate_xy(x, y, c, s);
//   v[1][a] = x; v[1][b] = y;
//   x = v[2][a]; y = v[2][b];
//   svd_rotate_xy(x, y, c, s);
//   v[2][a] = x; v[2][b] = y;
// }
fn svd_rotate(vtav: &mut Mat3x3, v: &mut Mat3x3, a: usize, b: usize) {
    if vtav[a][b] == 0.0 {
        return;
    }

    let (c, s) = givens_coeffs_sym(vtav[a][a], vtav[a][b], vtav[b][b]);

    let mut x = vtav[a][a];
    let mut y = vtav[b][b];
    svd_rotateq_xy(&mut x, &mut y, vtav[a][b], c, s);
    vtav[a][a] = x;
    vtav[b][b] = y;

    x = vtav[0][3 - b];
    y = vtav[1 - a][2];
    svd_rotate_xy(&mut x, &mut y, c, s);
    vtav[0][3 - b] = x;
    vtav[1 - a][2] = y;

    vtav[a][b] = 0.0;

    x = v[0][a];
    y = v[0][b];
    svd_rotate_xy(&mut x, &mut y, c, s);
    v[0][a] = x;
    v[0][b] = y;

    x = v[1][a];
    y = v[1][b];
    svd_rotate_xy(&mut x, &mut y, c, s);
    v[1][a] = x;
    v[1][b] = y;

    x = v[2][a];
    y = v[2][b];
    svd_rotate_xy(&mut x, &mut y, c, s);
    v[2][a] = x;
    v[2][b] = y;
}

// void svd_solve_sym(glm::mat3x3 vtav, glm::fvec3 &sigma, glm::mat3x3 &v) {
//   for (int i = 0; i < SVD_NUM_SWEEPS; ++i) {
//     svd_rotate(vtav, v, 0, 1);
//     svd_rotate(vtav, v, 0, 2);
//     svd_rotate(vtav, v, 1, 2);
//   }
//   sigma = glm::fvec3(vtav[0][0], vtav[1][1], vtav[2][2]);
// }
fn svd_solve_sym(mut vtav: Mat3x3) -> (Vec3, Mat3x3) {
    let mut v = mat3_identity();
    for _ in 0..SVD_NUM_SWEEPS {
        svd_rotate(&mut vtav, &mut v, 0, 1);
        svd_rotate(&mut vtav, &mut v, 0, 2);
        svd_rotate(&mut vtav, &mut v, 1, 2);
    }
    let sigma = Vec3::new(vtav[0][0], vtav[1][1], vtav[2][2]);
    (sigma, v)
}

// glm::fvec3 svd_solve_ATA_ATb(const glm::mat3x3 &ATA, const glm::fvec3 &ATb) {
//   glm::mat3x3 V;
//   glm::fvec3 sigma;
//   svd_solve_sym(ATA, sigma, V);
//   glm::mat3x3 Vinv;
//   svd_pseudoinverse(Vinv, sigma, V);
//   glm::fvec3 x = Vinv * ATb;
//   return x;
// }
fn svd_solve_ata_atb(ata: &Mat3x3, atb: Vec3) -> Vec3 {
    let (sigma, v) = svd_solve_sym(*ata);
    let vinv = svd_pseudoinverse(sigma, &v);
    mat3_mul_vec3(&vinv, atb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_solver_leaves_unchanged() {
        // C++: solve() does nothing when pointCount==0
        let mut solver = QefSolver::new();
        let mut pos = Vec3::new(99.0, 99.0, 99.0);
        let mut err = -1.0_f32;
        solver.solve(&mut pos, &mut err);
        // pos and err should be LEFT UNCHANGED
        assert_eq!(pos, Vec3::new(99.0, 99.0, 99.0));
        assert_eq!(err, -1.0);
    }

    #[test]
    fn single_plane_solve() {
        let mut solver = QefSolver::new();
        solver.add(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
        let mut pos = Vec3::ZERO;
        let mut err = -1.0_f32;
        solver.solve(&mut pos, &mut err);
        assert!((pos.z - 1.0).abs() < 1e-3, "pos.z = {}", pos.z);
        assert!(err < 1e-3, "err = {err}");
    }

    #[test]
    fn three_orthogonal_planes() {
        let mut solver = QefSolver::new();
        solver.add(Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        solver.add(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        solver.add(Vec3::new(0.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 1.0));
        let mut pos = Vec3::ZERO;
        let mut err = -1.0_f32;
        solver.solve(&mut pos, &mut err);
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

        let mut pos_a = Vec3::ZERO;
        let mut err_a = 0.0_f32;
        a.solve(&mut pos_a, &mut err_a);
        let mut pos_c = Vec3::ZERO;
        let mut err_c = 0.0_f32;
        combined.solve(&mut pos_c, &mut err_c);
        assert!((pos_a - pos_c).length() < 1e-3);
    }

    #[test]
    fn get_error_at_solution_is_low() {
        let mut solver = QefSolver::new();
        solver.add(Vec3::new(1.0, 0.0, 0.0), Vec3::X);
        solver.add(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
        solver.add(Vec3::new(0.0, 0.0, 1.0), Vec3::Z);
        let mut pos = Vec3::ZERO;
        let mut err = 0.0_f32;
        solver.solve(&mut pos, &mut err);
        let err = solver.get_error_at(pos);
        assert!(err < 1e-2, "err = {err}");
    }

    #[test]
    fn diagonal_planes_solve() {
        let mut solver = QefSolver::new();
        let n1 = Vec3::new(1.0, 1.0, 0.0).normalize();
        let n2 = Vec3::new(0.0, 1.0, 1.0).normalize();
        let n3 = Vec3::new(1.0, 0.0, 1.0).normalize();
        let target = Vec3::new(1.0, 1.0, 1.0);
        solver.add(target, n1);
        solver.add(target, n2);
        solver.add(target, n3);
        let mut pos = Vec3::ZERO;
        let mut err = -1.0_f32;
        solver.solve(&mut pos, &mut err);
        assert!(
            (pos - target).length() < 0.1,
            "pos = {pos:?}, expected near {target:?}"
        );
        assert!(err < 0.1, "err = {err}");
    }

    #[test]
    fn radial_normals_solve() {
        let mut solver = QefSolver::new();
        let radius = 3.0_f32;
        for angle_deg in &[0.0_f32, 90.0, 180.0, 270.0] {
            let angle = angle_deg.to_radians();
            let p = Vec3::new(radius * angle.cos(), radius * angle.sin(), 0.0);
            let n = Vec3::new(angle.cos(), angle.sin(), 0.0);
            solver.add(p, n);
        }
        let mut pos = Vec3::ZERO;
        let mut _err = 0.0_f32;
        solver.solve(&mut pos, &mut _err);
        assert!(pos.length() < 0.5, "pos = {pos:?}, expected near origin");
    }
}
