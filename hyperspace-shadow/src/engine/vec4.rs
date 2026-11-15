//! Vectors / points in 4-dimensional continuous Euclidean space E⁴.
//!
//! Mirrors epicode's `domain::vertex::Point3` (`D:\epicode\epicode-main\backend\src\domain\vertex.rs`)
//! with one extra coordinate `w`. In this model `w` carries the bitemporal axis
//! (log₂-compressed absolute time), so `xyz` keeps the semantic/label topology
//! of the current 3D space while `w` stratifies memories into eras.

/// A point/vector in E⁴. `Point4` is an alias — same arithmetic, different intent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

pub type Point4 = Vec4;

impl Vec4 {
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    pub const fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        }
    }

    pub fn add(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x + o.x, self.y + o.y, self.z + o.z, self.w + o.w)
    }

    pub fn sub(self, o: Vec4) -> Vec4 {
        Vec4::new(self.x - o.x, self.y - o.y, self.z - o.z, self.w - o.w)
    }

    pub fn scale(self, s: f64) -> Vec4 {
        Vec4::new(self.x * s, self.y * s, self.z * s, self.w * s)
    }

    pub fn dot(self, o: Vec4) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }

    pub fn length(self) -> f64 {
        self.length_sq().sqrt()
    }

    pub fn distance(a: Vec4, b: Vec4) -> f64 {
        a.sub(b).length()
    }

    pub fn distance_sq(a: Vec4, b: Vec4) -> f64 {
        a.sub(b).length_sq()
    }

    /// Linear interpolation, `t = 0` → self, `t = 1` → `o`.
    pub fn lerp(self, o: Vec4, t: f64) -> Vec4 {
        self.add(o.sub(self).scale(t))
    }

    /// Anisotropic distance used by the 4D ε-merge:
    /// `dκ(a,b)² = |axyz − bxyz|² + (κ · (aw − bw))²`.
    ///
    /// `κ = 0` collapses the metric onto the xyz subspace — the merge behaviour
    /// of epicode's current 3D space, exactly. `κ = 1` is the fully coupled
    /// 4D metric. Intermediate κ is the "time-coupling dial" of this research.
    pub fn aniso_distance_sq(a: Vec4, b: Vec4, kappa: f64) -> f64 {
        let d = a.sub(b);
        d.x * d.x + d.y * d.y + d.z * d.z + kappa * kappa * d.w * d.w
    }

    pub fn aniso_distance(a: Vec4, b: Vec4, kappa: f64) -> f64 {
        Self::aniso_distance_sq(a, b, kappa).sqrt()
    }

    /// Euclidean projection onto the xyz subspace (drop w).
    pub fn xyz(self) -> (f64, f64, f64) {
        (self.x, self.y, self.z)
    }
}
