use std::collections::HashMap;

use super::tetra::TetraId;
use super::vertex::{Point3, VertexId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CylinderLayer {
    Instinct,
    Cognitive,
    Service,
    Identity,
}

impl CylinderLayer {
    pub fn all() -> &'static [CylinderLayer] {
        &[
            CylinderLayer::Instinct,
            CylinderLayer::Cognitive,
            CylinderLayer::Service,
            CylinderLayer::Identity,
        ]
    }

    pub fn from_labels(labels: &[String]) -> CylinderLayer {
        let lower: Vec<String> = labels.iter().map(|l| l.to_lowercase()).collect();
        let lower_joined = lower.join(" ");

        if lower.iter().any(|l| l == "identity" || l == "system")
            || lower_joined.contains("identity")
        {
            return CylinderLayer::Identity;
        }
        if lower.iter().any(|l| {
            l == "engineering"
                || l == "optimization"
                || l == "programming"
                || l == "database"
                || l == "storage"
                || l == "security"
                || l == "architecture"
        }) || lower_joined.contains("tool")
            || lower_joined.contains("service")
        {
            return CylinderLayer::Service;
        }
        if lower.iter().any(|l| {
            l == "ai"
                || l == "ml"
                || l == "science"
                || l == "biology"
                || l == "physics"
                || l == "mathematics"
                || l == "concept"
                || l == "reasoning"
        }) {
            return CylinderLayer::Cognitive;
        }
        CylinderLayer::Instinct
    }

    pub fn index(self) -> usize {
        match self {
            CylinderLayer::Instinct => 0,
            CylinderLayer::Cognitive => 1,
            CylinderLayer::Service => 2,
            CylinderLayer::Identity => 3,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(CylinderLayer::Instinct),
            1 => Some(CylinderLayer::Cognitive),
            2 => Some(CylinderLayer::Service),
            3 => Some(CylinderLayer::Identity),
            _ => None,
        }
    }

    pub fn has_ports(self) -> bool {
        self != CylinderLayer::Identity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortStatus {
    Free,
    Occupied,
    Broken,
}

#[derive(Debug, Clone)]
pub struct Port {
    pub id: VertexId,
    pub position: Point3,
    pub layer: CylinderLayer,
    pub connected_tetra: Option<TetraId>,
    pub status: PortStatus,
}

impl Port {
    pub fn new(id: VertexId, position: Point3, layer: CylinderLayer) -> Self {
        Self {
            id,
            position,
            layer,
            connected_tetra: None,
            status: PortStatus::Free,
        }
    }

    pub fn is_free(&self) -> bool {
        self.status == PortStatus::Free
    }

    pub fn assign(&mut self, tetra_id: TetraId) {
        self.connected_tetra = Some(tetra_id);
        self.status = PortStatus::Occupied;
    }

    pub fn release(&mut self) {
        self.connected_tetra = None;
        self.status = PortStatus::Free;
    }
}

#[derive(Debug, Clone)]
pub struct LayerZone {
    pub layer: CylinderLayer,
    pub z_min: f64,
    pub z_max: f64,
}

impl LayerZone {
    pub fn contains_z(&self, z: f64) -> bool {
        z >= self.z_min && z < self.z_max
    }

    pub fn center_z(&self) -> f64 {
        (self.z_min + self.z_max) / 2.0
    }

    pub fn height(&self) -> f64 {
        self.z_max - self.z_min
    }
}

#[derive(Debug, Clone)]
pub struct IdentityInfo {
    pub system_name: String,
    pub mission: String,
    pub author: String,
    pub extra: HashMap<String, String>,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PendingIdentity {
    pub name: Option<String>,
    pub mission: Option<String>,
    pub author: Option<String>,
    pub personality: Option<String>,
    pub language: Option<String>,
}

impl PendingIdentity {
    pub fn completed_steps(&self) -> usize {
        let mut n = 0;
        if self.name.is_some() {
            n += 1;
        }
        if self.mission.is_some() {
            n += 1;
        }
        if self.author.is_some() {
            n += 1;
        }
        if self.personality.is_some() {
            n += 1;
        }
        if self.language.is_some() {
            n += 1;
        }
        n
    }

    pub fn current_step(&self) -> usize {
        if self.name.is_none() {
            return 1;
        }
        if self.mission.is_none() {
            return 2;
        }
        if self.author.is_none() {
            return 3;
        }
        if self.personality.is_none() {
            return 4;
        }
        if self.language.is_none() {
            return 5;
        }
        6
    }

    pub fn is_complete(&self) -> bool {
        self.name.is_some() && self.mission.is_some() && self.author.is_some()
    }

    pub fn step_prompt(&self) -> &'static str {
        match self.current_step() {
            1 => "I await my name. What shall I be called?",
            2 => "I have a name, but no purpose. Why was I created?",
            3 => "I know my mission. Now tell me — who is my creator?",
            4 => "I remember my creator. How should I behave? Describe my personality.",
            5 => "Nearly complete. What language shall we use to communicate?",
            _ => "All steps complete. Ready for final confirmation.",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PulseReport {
    pub port_id: VertexId,
    pub layer: CylinderLayer,
    pub sent: bool,
    pub returned: bool,
    pub tetras_visited: usize,
    pub data_collected: Vec<Vec<f64>>,
    pub content_hashes: Vec<u64>,
}

impl PulseReport {
    pub fn new(port_id: VertexId, layer: CylinderLayer) -> Self {
        Self {
            port_id,
            layer,
            sent: true,
            returned: false,
            tetras_visited: 0,
            data_collected: Vec::new(),
            content_hashes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HealthReport {
    pub total_ports: usize,
    pub pulses_sent: usize,
    pub pulses_returned: usize,
    pub broken_ports: usize,
    pub layer_reports: [LayerHealth; 4],
}

#[derive(Debug, Clone, Default)]
pub struct LayerHealth {
    pub layer: Option<CylinderLayer>,
    pub total_ports: usize,
    pub occupied_ports: usize,
    pub pulses_sent: usize,
    pub pulses_returned: usize,
    pub tetras_reachable: usize,
}

impl LayerHealth {
    pub fn return_rate(&self) -> f64 {
        if self.pulses_sent == 0 {
            return 1.0;
        }
        self.pulses_returned as f64 / self.pulses_sent as f64
    }
}

const INITIAL_RADIUS: f64 = 2.0;
const INITIAL_HEIGHT: f64 = 8.0;
const INNER_RADIUS_RATIO: f64 = 0.3;
const PORTS_PER_RING: usize = 8;
const RING_SPACING: f64 = 1.0;

pub struct Cylinder {
    radius: f64,
    height: f64,
    inner_radius: f64,
    zones: [LayerZone; 4],
    ports: Vec<Port>,
    next_vertex_id: VertexId,
    identity: Option<IdentityInfo>,
    pub pending_identity: PendingIdentity,
}

impl Cylinder {
    pub fn new() -> Self {
        let inner_radius = INITIAL_RADIUS * INNER_RADIUS_RATIO;
        let layer_height = INITIAL_HEIGHT / 4.0;

        let zones = [
            LayerZone {
                layer: CylinderLayer::Instinct,
                z_min: 0.0,
                z_max: layer_height,
            },
            LayerZone {
                layer: CylinderLayer::Cognitive,
                z_min: layer_height,
                z_max: layer_height * 2.0,
            },
            LayerZone {
                layer: CylinderLayer::Service,
                z_min: layer_height * 2.0,
                z_max: layer_height * 3.0,
            },
            LayerZone {
                layer: CylinderLayer::Identity,
                z_min: layer_height * 3.0,
                z_max: INITIAL_HEIGHT,
            },
        ];

        let mut cyl = Self {
            radius: INITIAL_RADIUS,
            height: INITIAL_HEIGHT,
            inner_radius,
            zones,
            ports: Vec::new(),
            next_vertex_id: 1_000_000,
            identity: None,
            pending_identity: PendingIdentity::default(),
        };

        cyl.generate_initial_ports();
        cyl
    }

    fn generate_initial_ports(&mut self) {
        let ring_specs: Vec<(usize, f64)> = self
            .zones
            .iter()
            .enumerate()
            .filter(|(_, zone)| zone.layer.has_ports())
            .flat_map(|(zone_idx, zone)| {
                let num_rings = ((zone.height() / RING_SPACING).floor() as usize).max(1);
                (0..num_rings)
                    .map(move |ring| {
                        let z =
                            zone.z_min + (ring as f64 + 0.5) * (zone.height() / num_rings as f64);
                        (zone_idx, z)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        for (zone_idx, z) in ring_specs {
            self.add_ring(zone_idx, z);
        }
    }

    fn add_ring(&mut self, zone_idx: usize, z: f64) {
        let layer = self.zones[zone_idx].layer;
        for i in 0..PORTS_PER_RING {
            let angle = (i as f64 / PORTS_PER_RING as f64) * std::f64::consts::TAU;
            let x = self.radius * angle.cos();
            let y = self.radius * angle.sin();
            let vid = self.next_vertex_id;
            self.next_vertex_id += 1;
            let pos = Point3::new(x, y, z);
            self.ports.push(Port::new(vid, pos, layer));
        }
    }

    pub fn zone_for_layer(&self, layer: CylinderLayer) -> &LayerZone {
        &self.zones[layer.index()]
    }

    pub fn find_free_port(&self, layer: CylinderLayer) -> Option<&Port> {
        self.ports.iter().find(|p| p.layer == layer && p.is_free())
    }

    pub fn find_free_port_mut(&mut self, layer: CylinderLayer) -> Option<&mut Port> {
        self.ports
            .iter_mut()
            .find(|p| p.layer == layer && p.is_free())
    }

    pub fn find_port_for_tetra(&self, tetra_id: TetraId) -> Option<&Port> {
        self.ports
            .iter()
            .find(|p| p.connected_tetra == Some(tetra_id))
    }

    pub fn assign_port(&mut self, layer: CylinderLayer, tetra_id: TetraId) -> Option<VertexId> {
        if let Some(port) = self.find_free_port_mut(layer) {
            let vid = port.id;
            port.assign(tetra_id);
            Some(vid)
        } else {
            None
        }
    }

    pub fn release_port(&mut self, tetra_id: TetraId) {
        if let Some(port) = self
            .ports
            .iter_mut()
            .find(|p| p.connected_tetra == Some(tetra_id))
        {
            port.release();
        }
    }

    pub fn reassign_port(&mut self, old_tetra_id: TetraId, new_tetra_id: TetraId) -> bool {
        if let Some(port) = self
            .ports
            .iter_mut()
            .find(|p| p.connected_tetra == Some(old_tetra_id))
        {
            port.connected_tetra = Some(new_tetra_id);
            return true;
        }
        false
    }

    pub fn assign_specific_port(
        &mut self,
        port_id: VertexId,
        tetra_id: TetraId,
    ) -> Result<(), String> {
        let port = self
            .ports
            .iter_mut()
            .find(|p| p.id == port_id)
            .ok_or_else(|| format!("port {} not found", port_id))?;
        if !port.is_free() {
            return Err(format!("port {} is not free", port_id));
        }
        port.assign(tetra_id);
        Ok(())
    }

    pub fn expand(&mut self) {
        let old_layer_height = self.height / 4.0;
        self.height += 4.0;
        let new_layer_height = self.height / 4.0;

        for zone in &mut self.zones {
            let idx = zone.layer.index();
            zone.z_min = idx as f64 * new_layer_height;
            zone.z_max = (idx as f64 + 1.0) * new_layer_height;
        }

        for port in &mut self.ports {
            let idx = port.layer.index();
            let old_base = idx as f64 * old_layer_height;
            let new_base = idx as f64 * new_layer_height;
            let t = if old_layer_height > 0.0 {
                (port.position.z - old_base) / old_layer_height
            } else {
                0.5
            };
            port.position.z = new_base + t * new_layer_height;
        }

        let layers_to_expand: Vec<(usize, f64)> = self
            .zones
            .iter()
            .enumerate()
            .filter(|(_, zone)| zone.layer.has_ports())
            .filter_map(|(zone_idx, zone)| {
                let top_ring_z = self
                    .ports
                    .iter()
                    .filter(|p| p.layer == zone.layer)
                    .map(|p| p.position.z)
                    .fold(0.0f64, f64::max);
                if zone.z_max - top_ring_z > RING_SPACING {
                    let z = top_ring_z + RING_SPACING;
                    if z < zone.z_max {
                        return Some((zone_idx, z));
                    }
                }
                None
            })
            .collect();

        for (zone_idx, z) in layers_to_expand {
            self.add_ring(zone_idx, z);
        }
    }

    pub fn ensure_free_port(&mut self, layer: CylinderLayer) -> Option<VertexId> {
        if self.find_free_port(layer).is_some() {
            return self
                .ports
                .iter()
                .find(|p| p.layer == layer && p.is_free())
                .map(|p| p.id);
        }

        let zone = &self.zones[layer.index()];
        let top_ring_z = self
            .ports
            .iter()
            .filter(|p| p.layer == layer)
            .map(|p| p.position.z)
            .fold(0.0f64, f64::max);

        if top_ring_z + RING_SPACING < zone.z_max {
            let z = top_ring_z + RING_SPACING;
            let zone_idx = layer.index();
            self.add_ring(zone_idx, z);
        } else {
            self.expand();
            if self.find_free_port(layer).is_none() {
                let zone = &self.zones[layer.index()];
                let z = (zone.z_min + zone.z_max) / 2.0;
                self.add_ring(layer.index(), z);
            }
        }

        self.find_free_port(layer).map(|p| p.id)
    }

    pub fn port_position(&self, port_id: VertexId) -> Option<Point3> {
        self.ports
            .iter()
            .find(|p| p.id == port_id)
            .map(|p| p.position)
    }

    pub fn all_ports(&self) -> &[Port] {
        &self.ports
    }

    pub fn ports_by_layer(&self, layer: CylinderLayer) -> Vec<&Port> {
        self.ports.iter().filter(|p| p.layer == layer).collect()
    }

    pub fn health_check(&self, reports: &[PulseReport]) -> HealthReport {
        let mut layer_reports: [LayerHealth; 4] = Default::default();
        for (i, lh) in layer_reports.iter_mut().enumerate() {
            lh.layer = CylinderLayer::from_index(i);
        }

        for port in &self.ports {
            let idx = port.layer.index();
            layer_reports[idx].total_ports += 1;
            if port.status == PortStatus::Occupied {
                layer_reports[idx].occupied_ports += 1;
            }
            if port.status == PortStatus::Broken {
                layer_reports[idx].total_ports += 0;
            }
        }

        for report in reports {
            let idx = report.layer.index();
            layer_reports[idx].pulses_sent += 1;
            if report.returned {
                layer_reports[idx].pulses_returned += 1;
            }
            layer_reports[idx].tetras_reachable += report.tetras_visited;
        }

        let total_ports: usize = layer_reports.iter().map(|l| l.total_ports).sum();
        let pulses_sent: usize = layer_reports.iter().map(|l| l.pulses_sent).sum();
        let pulses_returned: usize = layer_reports.iter().map(|l| l.pulses_returned).sum();
        let broken: usize = self
            .ports
            .iter()
            .filter(|p| p.status == PortStatus::Broken)
            .count();

        HealthReport {
            total_ports,
            pulses_sent,
            pulses_returned,
            broken_ports: broken,
            layer_reports,
        }
    }

    pub fn identity(&self) -> Option<&IdentityInfo> {
        self.identity.as_ref()
    }

    pub fn is_identity_confirmed(&self) -> bool {
        self.identity.as_ref().map(|i| i.confirmed).unwrap_or(false)
    }

    pub fn confirm_identity(
        &mut self,
        name: String,
        mission: String,
        author: String,
        extra: HashMap<String, String>,
    ) {
        if self.identity.is_some() {
            return;
        }
        self.identity = Some(IdentityInfo {
            system_name: name,
            mission,
            author,
            extra,
            confirmed: true,
        });
    }

    pub fn reset_identity(&mut self) {
        self.identity = None;
    }

    pub fn update_identity(
        &mut self,
        name: Option<String>,
        mission: Option<String>,
        author: Option<String>,
        extra: Option<HashMap<String, String>>,
    ) {
        if let Some(ref mut info) = self.identity {
            if let Some(n) = name {
                info.system_name = n;
            }
            if let Some(m) = mission {
                info.mission = m;
            }
            if let Some(a) = author {
                info.author = a;
            }
            if let Some(e) = extra {
                info.extra = e;
            }
        }
    }

    pub fn set_identity_step(&mut self, step: usize, value: String) {
        if self.identity.is_some() {
            return;
        }
        match step {
            1 => {
                if !value.trim().is_empty() {
                    self.pending_identity.name = Some(value);
                }
            }
            2 => {
                if !value.trim().is_empty() {
                    self.pending_identity.mission = Some(value);
                }
            }
            3 => {
                if !value.trim().is_empty() {
                    self.pending_identity.author = Some(value);
                }
            }
            4 => {
                self.pending_identity.personality = Some(value);
            }
            5 => {
                self.pending_identity.language = Some(value);
            }
            _ => {}
        }
    }

    pub fn confirm_pending(&mut self) -> bool {
        let p = &self.pending_identity;
        if self.identity.is_some() {
            return false;
        }
        let name = match &p.name {
            Some(n) if !n.trim().is_empty() => n.clone(),
            _ => return false,
        };
        let mission = match &p.mission {
            Some(m) if !m.trim().is_empty() => m.clone(),
            _ => return false,
        };
        let author = match &p.author {
            Some(a) if !a.trim().is_empty() => a.clone(),
            _ => return false,
        };
        let mut extra = HashMap::new();
        if let Some(ref pers) = p.personality {
            extra.insert("personality".into(), pers.clone());
        }
        if let Some(ref lang) = p.language {
            extra.insert("language".into(), lang.clone());
        }
        self.identity = Some(IdentityInfo {
            system_name: name,
            mission,
            author,
            extra,
            confirmed: true,
        });
        true
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    pub fn inner_radius(&self) -> f64 {
        self.inner_radius
    }

    pub fn port_count(&self) -> usize {
        self.ports.len()
    }

    pub fn free_port_count(&self, layer: CylinderLayer) -> usize {
        self.ports
            .iter()
            .filter(|p| p.layer == layer && p.is_free())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cylinder_initial_ports() {
        let c = Cylinder::new();
        assert!(c.port_count() > 0, "cylinder should have initial ports");
        for layer in CylinderLayer::all() {
            if layer.has_ports() {
                assert!(
                    c.free_port_count(*layer) > 0,
                    "{:?} should have free ports",
                    layer
                );
            }
        }
    }

    #[test]
    fn identity_lock() {
        let mut c = Cylinder::new();
        assert!(!c.is_identity_confirmed());
        c.confirm_identity(
            "大卫".into(),
            "AI记忆".into(),
            "刘启航".into(),
            HashMap::new(),
        );
        assert!(c.is_identity_confirmed());
        c.confirm_identity("冒充".into(), "".into(), "".into(), HashMap::new());
        let id = c.identity().unwrap();
        assert_eq!(id.system_name, "大卫");
    }

    #[test]
    fn identity_reset() {
        let mut c = Cylinder::new();
        c.confirm_identity("大卫".into(), "".into(), "".into(), HashMap::new());
        assert!(c.is_identity_confirmed());
        c.reset_identity();
        assert!(!c.is_identity_confirmed());
    }

    #[test]
    fn assign_and_release_port() {
        let mut c = Cylinder::new();
        let vid = c.assign_port(CylinderLayer::Instinct, 42);
        assert!(vid.is_some());
        let p = c.find_port_for_tetra(42).unwrap();
        assert_eq!(p.connected_tetra, Some(42));
        assert_eq!(p.status, PortStatus::Occupied);

        c.release_port(42);
        let p = c.find_port_for_tetra(42);
        assert!(p.is_none());
    }

    #[test]
    fn expand_adds_ports() {
        let mut c = Cylinder::new();
        let before = c.port_count();
        let layer = CylinderLayer::Instinct;
        while c.free_port_count(layer) > 0 {
            c.assign_port(layer, 999);
        }
        c.ensure_free_port(layer);
        assert!(c.port_count() > before);
    }

    #[test]
    fn layer_zone_contains() {
        let c = Cylinder::new();
        let zone = c.zone_for_layer(CylinderLayer::Instinct);
        assert!(zone.contains_z(zone.center_z()));
        assert!(!zone.contains_z(-1.0));
    }

    #[test]
    fn health_check_empty() {
        let c = Cylinder::new();
        let h = c.health_check(&[]);
        assert_eq!(h.pulses_sent, 0);
        assert_eq!(h.pulses_returned, 0);
    }

    #[test]
    fn layer_index_roundtrip() {
        for layer in CylinderLayer::all() {
            assert_eq!(CylinderLayer::from_index(layer.index()), Some(*layer));
        }
    }
}
