use super::vertex::{Point3, VertexId};

pub type TetraId = u64;

pub const VERTEX_COUNT: usize = 4;
pub const EDGE_COUNT: usize = 6;
pub const FACE_COUNT: usize = 4;

/// Fixed edge length for every tetrahedron in this space.
/// The space is a uniform grid built on this constant.
pub const EDGE_LENGTH: f64 = 1.0;

const SHAPE_EPSILON: f64 = 1e-10;

/// Canonical vertex offsets from the tetrahedron center.
/// All 6 edges equal EDGE_LENGTH. Verified by construction:
///
///   v0 = ( a/2,    0,  -a/(2\u{221a}2) )
///   v1 = (-a/2,    0,  -a/(2\u{221a}2) )
///   v2 = (   0,  a/2,   a/(2\u{221a}2) )
///   v3 = (   0, -a/2,   a/(2\u{221a}2) )
///
/// where a = EDGE_LENGTH, a/(2\u{221a}2) = a \u{221a}2 / 4 \u{2248} 0.353553...
const fn canonical_offsets() -> [Point3; 4] {
    let half = 0.5;
    let z = 0.3535533905932738;
    [
        Point3 {
            x: half,
            y: 0.0,
            z: -z,
        },
        Point3 {
            x: -half,
            y: 0.0,
            z: -z,
        },
        Point3 {
            x: 0.0,
            y: half,
            z: z,
        },
        Point3 {
            x: 0.0,
            y: -half,
            z: z,
        },
    ]
}

const EDGES: [(usize, usize); EDGE_COUNT] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

const FACES: [[usize; 3]; FACE_COUNT] = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryPayload {
    pub content: String,
    pub content_hash: u64,
    pub labels: Vec<String>,
    pub timestamp: i64,
    pub aliases: Vec<String>,
    #[serde(default)]
    pub embedding: Vec<f64>,
    #[serde(default = "default_importance")]
    pub importance: f64,
    #[serde(default)]
    pub enforced: bool,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub access_count: u32,
    #[serde(default)]
    pub memory_type: Option<String>,
}

fn default_importance() -> f64 {
    1.0
}

impl Default for MemoryPayload {
    fn default() -> Self {
        Self {
            content: String::new(),
            content_hash: 0,
            labels: vec![],
            timestamp: 0,
            aliases: vec![],
            embedding: vec![],
            importance: 1.0,
            enforced: false,
            rationale: None,
            access_count: 0,
            memory_type: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tetrahedron {
    pub id: TetraId,
    pub vertex_ids: [VertexId; VERTEX_COUNT],
    pub core: Point3,
    pub data: MemoryPayload,
    pub mass: f64,
}

impl Tetrahedron {
    /// Compute the 4 vertex positions for a tetrahedron centered at `center`.
    pub fn compute_vertices(center: Point3) -> [Point3; VERTEX_COUNT] {
        let offsets = canonical_offsets();
        [
            Point3::new(
                center.x + offsets[0].x,
                center.y + offsets[0].y,
                center.z + offsets[0].z,
            ),
            Point3::new(
                center.x + offsets[1].x,
                center.y + offsets[1].y,
                center.z + offsets[1].z,
            ),
            Point3::new(
                center.x + offsets[2].x,
                center.y + offsets[2].y,
                center.z + offsets[2].z,
            ),
            Point3::new(
                center.x + offsets[3].x,
                center.y + offsets[3].y,
                center.z + offsets[3].z,
            ),
        ]
    }

    pub fn edges() -> &'static [(usize, usize); EDGE_COUNT] {
        &EDGES
    }

    pub fn faces() -> &'static [[usize; 3]; FACE_COUNT] {
        &FACES
    }

    /// Get a vertex id by index (0..3).
    pub fn vertex(&self, index: usize) -> VertexId {
        self.vertex_ids[index]
    }

    /// Validate that 4 positions form a regular tetrahedron with edge = EDGE_LENGTH.
    pub fn validate_shape(positions: &[Point3; VERTEX_COUNT]) -> bool {
        for &(i, j) in Self::edges() {
            if (positions[i].distance_to(&positions[j]) - EDGE_LENGTH).abs() > SHAPE_EPSILON {
                return false;
            }
        }
        true
    }

    /// Verify that the computed centroid matches the core point.
    pub fn verify_core(positions: &[Point3; VERTEX_COUNT], core: Point3) -> bool {
        let refs: [&Point3; 4] = [&positions[0], &positions[1], &positions[2], &positions[3]];
        Point3::centroid(&refs).distance_to(&core) < SHAPE_EPSILON
    }

    /// The volume of a regular tetrahedron: V = a\u{b3}\u{221a}2 / 12.
    pub fn volume() -> f64 {
        EDGE_LENGTH.powi(3) * std::f64::consts::SQRT_2 / 12.0
    }

    /// Distance from center to any vertex: a\u{221a}6 / 4.
    pub fn center_to_vertex() -> f64 {
        EDGE_LENGTH * (6.0_f64).sqrt() / 4.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions(center: Point3) -> [Point3; 4] {
        Tetrahedron::compute_vertices(center)
    }

    #[test]
    fn all_six_edges_equal() {
        let pos = positions(Point3::zero());
        for &(i, j) in Tetrahedron::edges() {
            let d = pos[i].distance_to(&pos[j]);
            assert!(
                (d - EDGE_LENGTH).abs() < 1e-10,
                "edge ({},{}) = {}, expected {}",
                i,
                j,
                d,
                EDGE_LENGTH
            );
        }
    }

    #[test]
    fn validate_regular() {
        assert!(Tetrahedron::validate_shape(&positions(Point3::zero())));
    }

    #[test]
    fn validate_deformed() {
        let mut pos = positions(Point3::zero());
        pos[0].x += 0.5;
        assert!(!Tetrahedron::validate_shape(&pos));
    }

    #[test]
    fn core_at_centroid() {
        let c = Point3::new(5.0, -3.0, 7.0);
        assert!(Tetrahedron::verify_core(&positions(c), c));
    }

    #[test]
    fn vertex_distance_from_center() {
        let expected = Tetrahedron::center_to_vertex();
        let pos = positions(Point3::zero());
        for p in &pos {
            let d = p.distance_to(&Point3::zero());
            assert!(
                (d - expected).abs() < 1e-10,
                "vertex-center dist = {}, expected {}",
                d,
                expected
            );
        }
    }

    #[test]
    fn faces_are_equilateral() {
        let pos = positions(Point3::zero());
        for &face in Tetrahedron::faces() {
            let d01 = pos[face[0]].distance_to(&pos[face[1]]);
            let d12 = pos[face[1]].distance_to(&pos[face[2]]);
            let d20 = pos[face[2]].distance_to(&pos[face[0]]);
            assert!((d01 - EDGE_LENGTH).abs() < 1e-10);
            assert!((d12 - EDGE_LENGTH).abs() < 1e-10);
            assert!((d20 - EDGE_LENGTH).abs() < 1e-10);
        }
    }

    #[test]
    fn volume_correct() {
        let expected = EDGE_LENGTH.powi(3) * std::f64::consts::SQRT_2 / 12.0;
        assert!((expected - 0.1178511301977579).abs() < 1e-10);
    }

    #[test]
    fn translate_preserves_shape() {
        let c = Point3::new(10.0, 20.0, 30.0);
        let pos = positions(c);
        assert!(Tetrahedron::validate_shape(&pos));
        assert!(Tetrahedron::verify_core(&pos, c));
    }

    #[test]
    fn edges_count() {
        assert_eq!(Tetrahedron::edges().len(), 6);
    }

    #[test]
    fn faces_count() {
        assert_eq!(Tetrahedron::faces().len(), 4);
    }

    #[test]
    fn volume_known_value() {
        assert!((Tetrahedron::volume() - 0.1178511301977579).abs() < 1e-10);
    }
}
