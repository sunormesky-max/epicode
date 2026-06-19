pub type VertexId = u64;

/// Two vertices within this distance are treated as the same point in space.
pub const VERTEX_MERGE_EPSILON: f64 = 0.05;

/// A point in 3-dimensional continuous space.
///
/// This is the fundamental unit of spatial existence.
/// All geometric operations begin here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Point3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// Euclidean distance between two points.
    pub fn distance_to(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Squared distance (avoids sqrt when only comparison is needed).
    pub fn distance_sq(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx * dx + dy * dy + dz * dz
    }

    /// The midpoint between this point and another.
    pub fn midpoint(&self, other: &Self) -> Self {
        Self {
            x: (self.x + other.x) / 2.0,
            y: (self.y + other.y) / 2.0,
            z: (self.z + other.z) / 2.0,
        }
    }

    /// Centroid (mean) of a set of points.
    pub fn centroid(points: &[&Point3]) -> Self {
        if points.is_empty() {
            return Self::zero();
        }
        let n = points.len() as f64;
        let sum = points.iter().fold((0.0, 0.0, 0.0), |(sx, sy, sz), p| {
            (sx + p.x, sy + p.y, sz + p.z)
        });
        Self {
            x: sum.0 / n,
            y: sum.1 / n,
            z: sum.2 / n,
        }
    }

    /// Translate this point by a vector.
    pub fn translate(&self, dx: f64, dy: f64, dz: f64) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            z: self.z + dz,
        }
    }
}

/// A vertex — a point in space with a unique identity.
///
/// Multiple tetrahedrons may reference the same vertex (shared points).
#[derive(Debug, Clone)]
pub struct Vertex {
    pub id: VertexId,
    pub position: Point3,
}

impl Vertex {
    pub fn new(id: VertexId, position: Point3) -> Self {
        Self { id, position }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_simple() {
        let a = Point3::new(0.0, 0.0, 0.0);
        let b = Point3::new(3.0, 4.0, 0.0);
        assert!((a.distance_to(&b) - 5.0).abs() < 1e-10);
    }

    #[test]
    fn distance_sq_identity() {
        let a = Point3::new(1.0, 2.0, 3.0);
        let b = Point3::new(4.0, 6.0, 8.0);
        let d = a.distance_to(&b);
        assert!((d * d - a.distance_sq(&b)).abs() < 1e-10);
    }

    #[test]
    fn distance_zero() {
        let a = Point3::new(1.0, 2.0, 3.0);
        assert!(a.distance_to(&a) < 1e-10);
    }

    #[test]
    fn midpoint() {
        let a = Point3::new(0.0, 0.0, 0.0);
        let b = Point3::new(2.0, 2.0, 2.0);
        let m = a.midpoint(&b);
        assert!((m.x - 1.0).abs() < 1e-10);
        assert!((m.y - 1.0).abs() < 1e-10);
        assert!((m.z - 1.0).abs() < 1e-10);
    }

    #[test]
    fn centroid_of_four() {
        let a = Point3::new(0.0, 0.0, 0.0);
        let b = Point3::new(4.0, 0.0, 0.0);
        let c = Point3::new(0.0, 4.0, 0.0);
        let d = Point3::new(0.0, 0.0, 4.0);
        let ctr = Point3::centroid(&[&a, &b, &c, &d]);
        assert!((ctr.x - 1.0).abs() < 1e-10);
        assert!((ctr.y - 1.0).abs() < 1e-10);
        assert!((ctr.z - 1.0).abs() < 1e-10);
    }

    #[test]
    fn translate() {
        let a = Point3::new(1.0, 2.0, 3.0);
        let b = a.translate(-1.0, 0.0, 1.0);
        assert!((b.x - 0.0).abs() < 1e-10);
        assert!((b.y - 2.0).abs() < 1e-10);
        assert!((b.z - 4.0).abs() < 1e-10);
    }
}
