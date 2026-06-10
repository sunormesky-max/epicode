use super::tetra::TetraId;

pub type PulseId = u64;

#[derive(Debug, Clone)]
pub struct Pulse {
    pub id: PulseId,
    pub origin: TetraId,
    pub ttl: u32,
}

#[derive(Debug, Clone, Default)]
pub struct PulseData {
    pub visited_tetras: Vec<TetraId>,
    pub collected_content_hashes: Vec<u64>,
    pub path_length: usize,
    pub discoveries: Vec<(TetraId, TetraId, f64)>,
}

#[derive(Debug, Clone)]
pub struct PulseResult {
    pub pulse_id: PulseId,
    pub origin: TetraId,
    pub reached_target: bool,
    pub data: PulseData,
    pub energy_cost: f64,
}
