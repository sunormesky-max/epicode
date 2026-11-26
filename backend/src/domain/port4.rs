//! Port4 — the (layer, era-band) lattice anchors with the occupancy state
//! machine ported from the 3D cylinder (`cylinder.rs` L451-543, S4 #398 M2b).
//!
//! The 3D cylinder gives each layer a ring of ports and grows by `expand()`.
//! The 4D lattice needs neither: every (layer, band) crossing has a
//! DETERMINISTIC core (`geo4::port_core`), instantiated lazily on first
//! seeding request, and the w axis extends itself as the space ages. The
//! occupancy semantics — one cluster one port, sentinel-safe specific
//! variants, reservation rollback — are ported verbatim.
//!
//! Borrow protocol when (layer, band) is occupied (porting map §4.3):
//! 1. free port already registered at the address → take it;
//! 2. else a free port at (layer, band ± 1) whose band centre is within 0.5
//!    of the requesting cluster's centroid w (era-compatible adjacency);
//! 3. else spawn a DERIVED port at the same address with the xy angle
//!    advanced 0.05 rad per derivation step — same (layer, band), slightly
//!    rotated, far beyond the ε-merge threshold (chord ≈ 0.1 ≫ 0.05), so
//!    derived ports never merge with the base port.

use std::collections::HashMap;

use super::cylinder::CylinderLayer;
use super::geo4::{band_center, port_core, Vec4};
use super::space4::PentaId;
use super::vertex::VertexId;

/// Ports live in a reserved vertex-id namespace, disjoint from ordinary
/// vertices (the production convention, `space.rs` port vids ≥ 1_000_000).
pub const PORT_VID_BASE: VertexId = 1_000_000;

/// Angle advance per derived port (radians) — chord ≈ 2·R·sin(0.025) ≈ 0.1.
const DERIVED_ANGLE_STEP: f64 = 0.05;

/// Era-compatibility gate for the ±1 band borrow.
const BORROW_W_TOLERANCE: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortStatus {
    Free,
    Occupied,
}

/// One lattice anchor. `derived` counts derivation steps at the same
/// address (0 = the canonical `geo4::port_core` position).
#[derive(Clone, Debug)]
pub struct Port4 {
    pub vid: VertexId,
    pub layer: u8,
    pub band: i64,
    pub derived: u32,
    pub position: Vec4,
    pub connected_cell: Option<PentaId>,
    pub status: PortStatus,
}

impl Port4 {
    pub fn is_free(&self) -> bool {
        self.status == PortStatus::Free
    }

    fn assign(&mut self, cell: PentaId) {
        self.connected_cell = Some(cell);
        self.status = PortStatus::Occupied;
    }

    fn release(&mut self) {
        self.connected_cell = None;
        self.status = PortStatus::Free;
    }
}

/// The 6 × W port lattice: deterministic addresses, lazy instantiation,
/// derived ports per address on saturation.
pub struct PortLattice {
    ports: HashMap<(u8, i64), Vec<Port4>>,
    next_vid: VertexId,
}

impl PortLattice {
    pub fn new() -> Self {
        PortLattice {
            ports: HashMap::new(),
            next_vid: PORT_VID_BASE,
        }
    }

    /// Deterministic core of the k-th derivation at (layer, band): the
    /// canonical position rotated by k · 0.05 rad in xy (radius, z, w fixed).
    fn derived_position(layer: CylinderLayer, band: i64, derived: u32) -> Vec4 {
        let base = port_core(layer, band);
        let idx = layer.index() as f64;
        let angle = idx * (std::f64::consts::PI / 3.0)
            + (band as f64) * 0.61
            + derived as f64 * DERIVED_ANGLE_STEP;
        let (s, c) = angle.sin_cos();
        Vec4::new(
            super::geo4::PORT_RADIUS * c,
            super::geo4::PORT_RADIUS * s,
            base.z,
            base.w,
        )
    }

    fn spawn(&mut self, layer: CylinderLayer, band: i64, derived: u32) -> Port4 {
        let port = Port4 {
            vid: self.next_vid,
            layer: layer.index() as u8,
            band,
            derived,
            position: Self::derived_position(layer, band, derived),
            connected_cell: None,
            status: PortStatus::Free,
        };
        self.next_vid += 1;
        self.ports
            .entry((port.layer, band))
            .or_default()
            .push(port.clone());
        port
    }

    /// A free port at exactly (layer, band): the canonical one on first
    /// request (lazy instantiation — no pre-generated rings), a derived one
    /// if the address exists but is saturated. Always returns a FREE port.
    pub fn ensure_port4(&mut self, layer: CylinderLayer, band: i64) -> VertexId {
        let key = (layer.index() as u8, band);
        if let Some(vid) = self.free_at(key) {
            return vid;
        }
        let derived = self.ports.get(&key).map_or(0, |v| v.len() as u32);
        self.spawn(layer, band, derived).vid
    }

    fn free_at(&self, key: (u8, i64)) -> Option<VertexId> {
        self.ports
            .get(&key)
            .and_then(|v| v.iter().find(|p| p.is_free()))
            .map(|p| p.vid)
    }

    fn reserve(&mut self, vid: VertexId, cell: PentaId) {
        if let Some(p) = self.find_mut(vid) {
            p.assign(cell);
        }
    }

    /// The full borrow protocol (porting map §4.3), in order:
    /// 1. free port already registered at (layer, band) — or its unspawned
    ///    canonical slot;
    /// 2. era-compatible ±1 band adjacency (|band centre − w_centroid| <
    ///    0.5), spawning the adjacent canonical if that band is unspawned;
    /// 3. a DERIVED port at the same address, xy angle advanced 0.05 rad —
    ///    far beyond the ε-merge threshold, so it never merges with the base.
    pub fn ensure_port4_for(
        &mut self,
        layer: CylinderLayer,
        band: i64,
        w_centroid: Option<f64>,
        cell: PentaId,
    ) -> VertexId {
        let key = (layer.index() as u8, band);
        // 1. exact address: registered free port, or spawn the canonical.
        if let Some(vid) = self.free_at(key) {
            self.reserve(vid, cell);
            return vid;
        }
        if !self.ports.contains_key(&key) {
            let vid = self.spawn(layer, band, 0).vid;
            self.reserve(vid, cell);
            return vid;
        }
        // 2. era-compatible ±1 band borrow.
        if let Some(w) = w_centroid {
            for b in [band - 1, band + 1] {
                if (band_center(b) - w).abs() >= BORROW_W_TOLERANCE {
                    continue;
                }
                let bkey = (layer.index() as u8, b);
                let vid = match self.free_at(bkey) {
                    Some(vid) => vid,
                    None if !self.ports.contains_key(&bkey) => self.spawn(layer, b, 0).vid,
                    None => continue, // spawned but saturated
                };
                self.reserve(vid, cell);
                return vid;
            }
        }
        // 3. derived port at the same address.
        let derived = self.ports.get(&key).map_or(0, |v| v.len() as u32);
        let vid = self.spawn(layer, band, derived).vid;
        self.reserve(vid, cell);
        vid
    }

    /// Reserve a free port at (layer, band) for `cell` (the plain assign).
    /// Spawns the canonical port on first use of the address; on a saturated
    /// address returns `None` — derivation is reserved for the explicit
    /// borrow protocol (`ensure_port4_for`), mirroring the production
    /// `assign_port`/`ensure_free_port` split.
    pub fn assign_port4(
        &mut self,
        layer: CylinderLayer,
        band: i64,
        cell: PentaId,
    ) -> Option<VertexId> {
        let key = (layer.index() as u8, band);
        let vid = match self.free_at(key) {
            Some(vid) => vid,
            None if !self.ports.contains_key(&key) => self.spawn(layer, band, 0).vid,
            None => return None,
        };
        self.reserve(vid, cell);
        Some(vid)
    }

    /// Find a port by vertex id.
    pub fn find(&self, vid: VertexId) -> Option<&Port4> {
        self.ports.values().flatten().find(|p| p.vid == vid)
    }

    fn find_mut(&mut self, vid: VertexId) -> Option<&mut Port4> {
        self.ports.values_mut().flatten().find(|p| p.vid == vid)
    }

    /// The port currently held by `cell`, if any.
    pub fn port_of_cell(&self, cell: PentaId) -> Option<&Port4> {
        self.ports
            .values()
            .flatten()
            .find(|p| p.connected_cell == Some(cell))
    }

    /// Release the port held by `cell` (plain release).
    pub fn release_port4(&mut self, cell: PentaId) {
        if let Some(p) = self
            .ports
            .values_mut()
            .flatten()
            .find(|p| p.connected_cell == Some(cell))
        {
            p.release();
        }
    }

    /// Move a port from one cell to another (plain reassign).
    pub fn reassign_port4(&mut self, old_cell: PentaId, new_cell: PentaId) -> bool {
        if let Some(p) = self
            .ports
            .values_mut()
            .flatten()
            .find(|p| p.connected_cell == Some(old_cell))
        {
            p.connected_cell = Some(new_cell);
            return true;
        }
        false
    }

    /// Reserve one specific port only when it is currently free.
    pub fn assign_specific_port4(&mut self, vid: VertexId, cell: PentaId) -> Result<(), String> {
        let port = self
            .find_mut(vid)
            .ok_or_else(|| format!("port {vid} not found"))?;
        if !port.is_free() {
            return Err(format!("port {vid} is not free"));
        }
        port.assign(cell);
        Ok(())
    }

    /// Transfer one reserved port only when its current owner is the
    /// expected one — several in-flight placements can share a sentinel
    /// owner, so lookup by owner alone is ambiguous (ported verbatim).
    pub fn reassign_specific_port4(
        &mut self,
        vid: VertexId,
        expected_cell: PentaId,
        new_cell: PentaId,
    ) -> Result<(), String> {
        let port = self
            .find_mut(vid)
            .ok_or_else(|| format!("port {vid} not found"))?;
        if port.status != PortStatus::Occupied || port.connected_cell != Some(expected_cell) {
            return Err(format!(
                "port {vid} is not reserved by cell {expected_cell}"
            ));
        }
        port.assign(new_cell);
        Ok(())
    }

    /// Release only the reservation for this exact port and expected owner.
    pub fn release_specific_port4(
        &mut self,
        vid: VertexId,
        expected_cell: PentaId,
    ) -> Result<(), String> {
        let port = self
            .find_mut(vid)
            .ok_or_else(|| format!("port {vid} not found"))?;
        if port.status != PortStatus::Occupied || port.connected_cell != Some(expected_cell) {
            return Err(format!(
                "port {vid} is not reserved by cell {expected_cell}"
            ));
        }
        port.release();
        Ok(())
    }

    /// All ports (reporting / restore paths).
    pub fn all_ports(&self) -> impl Iterator<Item = &Port4> {
        self.ports.values().flatten()
    }

    /// Number of registered ports.
    pub fn len(&self) -> usize {
        self.ports.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for PortLattice {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_instantiation_and_vid_namespace() {
        let mut lat = PortLattice::new();
        assert!(lat.is_empty());
        let vid = lat.ensure_port4(CylinderLayer::Cognitive, 3);
        assert!(vid >= PORT_VID_BASE, "ports live in the reserved namespace");
        assert_eq!(lat.len(), 1);
        // Same address, still free → same vid, no duplicate port.
        assert_eq!(lat.ensure_port4(CylinderLayer::Cognitive, 3), vid);
        assert_eq!(lat.len(), 1);
        // Deterministic canonical position.
        let p = lat.find(vid).unwrap();
        assert_eq!(p.derived, 0);
        assert!((p.position.w - band_center(3)).abs() < 1e-12);
    }

    #[test]
    fn assign_release_roundtrip() {
        let mut lat = PortLattice::new();
        let vid = lat
            .assign_port4(CylinderLayer::Relation, 2, 42)
            .expect("free canonical port");
        assert_eq!(lat.port_of_cell(42).unwrap().vid, vid);
        // Address now occupied for others…
        assert!(lat.assign_port4(CylinderLayer::Relation, 2, 43).is_none());
        // …release frees it again.
        lat.release_port4(42);
        assert!(lat.assign_port4(CylinderLayer::Relation, 2, 43).is_some());
    }

    #[test]
    fn specific_variants_check_owner() {
        let mut lat = PortLattice::new();
        let vid = lat.assign_port4(CylinderLayer::Identity, 1, 7).unwrap();
        // Wrong owner must be rejected…
        assert!(lat.release_specific_port4(vid, 999).is_err());
        assert!(lat.reassign_specific_port4(vid, 999, 1000).is_err());
        // …right owner succeeds.
        assert!(lat.reassign_specific_port4(vid, 7, 8).is_ok());
        assert_eq!(lat.port_of_cell(8).unwrap().vid, vid);
        assert!(lat.release_specific_port4(vid, 8).is_ok());
        assert!(lat.port_of_cell(8).is_none());
        // Assign-specific on an occupied port must fail.
        lat.assign_port4(CylinderLayer::Identity, 1, 9);
        assert!(lat.assign_specific_port4(vid, 10).is_err());
    }

    #[test]
    fn borrow_protocol_spawns_derived_and_uses_adjacent_band() {
        let mut lat = PortLattice::new();
        // Occupied canonical at (Cognitive, 3)…
        let v1 = lat.assign_port4(CylinderLayer::Cognitive, 3, 1).unwrap();
        // …request for the same address with an era-compatible centroid
        // (w=3.5 → band 3): no free port there, ±1 bands gate on |centre−w|.
        let v2 = lat.ensure_port4_for(CylinderLayer::Cognitive, 3, Some(3.5), 2);
        assert_ne!(v1, v2, "a derived port must be a distinct anchor");
        let p2 = lat.find(v2).unwrap();
        assert_eq!(p2.derived, 1, "first derivation at the address");
        assert_eq!(p2.band, 3, "derived stays at the same address");
        // Never ε-merges with the canonical position.
        let base = lat.find(v1).unwrap().position;
        assert!(Vec4::aniso_distance(base, p2.position, 1.0) > 0.05);
        // Era-INcompatible centroid: adjacent bands are gated off, so the
        // answer is still a same-address derivation.
        let v3 = lat.ensure_port4_for(CylinderLayer::Cognitive, 3, Some(9.9), 3);
        assert_eq!(lat.find(v3).unwrap().band, 3);

        // Era-compatible empty adjacent band gets borrowed instead: request
        // at a saturated (Service, 5) with centroid w = 4.5 → band 4 centre
        // 4.5 within tolerance, band 5 centre 5.5 is 1.0 away (gated).
        lat.assign_port4(CylinderLayer::Service, 5, 10);
        let v4 = lat.ensure_port4_for(CylinderLayer::Service, 5, Some(4.5), 11);
        assert_eq!(lat.find(v4).unwrap().band, 4, "adjacent band borrowed");
    }

    #[test]
    fn ports_never_merge_across_addresses() {
        let mut lat = PortLattice::new();
        let mut vids = Vec::new();
        for layer in CylinderLayer::all() {
            for band in -2_i64..=4 {
                vids.push(lat.ensure_port4(*layer, band));
            }
        }
        for i in 0..vids.len() {
            for j in i + 1..vids.len() {
                let a = lat.find(vids[i]).unwrap().position;
                let b = lat.find(vids[j]).unwrap().position;
                assert!(
                    Vec4::aniso_distance(a, b, 1.0) > 1.0,
                    "lattice anchors must stay well separated"
                );
            }
        }
    }
}
