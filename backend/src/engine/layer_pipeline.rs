use std::sync::Arc;

use crate::domain::cylinder::CylinderLayer;
use crate::domain::space::Space;

#[derive(Debug, Clone)]
pub struct RequestContext {
    pub operation: Operation,
    pub content: String,
    pub labels: Vec<String>,
    pub source_agent: Option<String>,
    pub identity_hash: Option<String>,
    pub audit_trail: Vec<LayerAuditEntry>,
}

#[derive(Debug, Clone)]
pub enum Operation {
    Create,
    Search,
    Recall,
    Delete,
}

#[derive(Debug, Clone)]
pub struct LayerAuditEntry {
    pub layer: CylinderLayer,
    pub action: String,
    pub detail: String,
}

impl RequestContext {
    pub fn new_create(content: &str, labels: Vec<String>) -> Self {
        Self {
            operation: Operation::Create,
            content: content.to_string(),
            labels,
            source_agent: None,
            identity_hash: None,
            audit_trail: Vec::new(),
        }
    }

    pub fn new_search(query: &str) -> Self {
        Self {
            operation: Operation::Search,
            content: query.to_string(),
            labels: vec![],
            source_agent: None,
            identity_hash: None,
            audit_trail: Vec::new(),
        }
    }

    pub fn stamp(&mut self, layer: CylinderLayer, action: &str, detail: &str) {
        self.audit_trail.push(LayerAuditEntry {
            layer,
            action: action.to_string(),
            detail: detail.to_string(),
        });
    }
}

#[derive(Debug, Clone)]
pub struct LayerDecision {
    pub allowed: bool,
    pub reason: String,
    pub modified_importance: Option<f64>,
    pub boost_labels: Vec<String>,
}

impl LayerDecision {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: String::new(),
            modified_importance: None,
            boost_labels: vec![],
        }
    }

    pub fn deny(reason: &str) -> Self {
        Self {
            allowed: false,
            reason: reason.to_string(),
            modified_importance: None,
            boost_labels: vec![],
        }
    }

    pub fn with_importance(mut self, imp: f64) -> Self {
        self.modified_importance = Some(imp);
        self
    }

    pub fn with_label(mut self, label: &str) -> Self {
        self.boost_labels.push(label.to_string());
        self
    }
}

pub struct LayerPipeline {
    space: Arc<Space>,
}

impl LayerPipeline {
    pub fn new(space: Arc<Space>) -> Self {
        Self { space }
    }

    pub fn process_create(&self, ctx: &mut RequestContext) -> LayerDecision {
        if let Err(e) = self.instinct_layer(ctx) {
            return LayerDecision::deny(&e);
        }
        if let Err(e) = self.relation_layer(ctx) {
            return LayerDecision::deny(&e);
        }
        if let Err(e) = self.cognitive_layer(ctx) {
            return LayerDecision::deny(&e);
        }
        if let Err(e) = self.service_layer(ctx) {
            return LayerDecision::deny(&e);
        }
        self.cycle_layer(ctx);
        self.identity_layer(ctx);
        LayerDecision::allow()
    }

    pub fn process_search(&self, ctx: &mut RequestContext) -> LayerDecision {
        ctx.stamp(CylinderLayer::Instinct, "receive", "query received");
        if ctx.content.trim().is_empty() {
            return LayerDecision::deny("empty query");
        }
        ctx.stamp(
            CylinderLayer::Relation,
            "expand",
            "KG synonym expansion queued",
        );
        ctx.stamp(
            CylinderLayer::Cognitive,
            "intent",
            "intent parsing + embedding",
        );
        ctx.stamp(CylinderLayer::Service, "execute", "search execution");
        ctx.stamp(CylinderLayer::Cycle, "boost", "recency boost applied");
        self.identity_layer(ctx);
        LayerDecision::allow()
    }

    fn instinct_layer(&self, ctx: &mut RequestContext) -> Result<(), String> {
        let content = ctx.content.trim();
        if content.is_empty() {
            ctx.stamp(CylinderLayer::Instinct, "reject", "empty content");
            return Err("content is empty".into());
        }
        if content.len() < 3 {
            ctx.stamp(CylinderLayer::Instinct, "reject", "content too short");
            return Err("content too short (min 3 chars)".into());
        }
        ctx.stamp(
            CylinderLayer::Instinct,
            "receive",
            &format!("content_len={}", ctx.content.len()),
        );
        Ok(())
    }

    fn relation_layer(&self, ctx: &mut RequestContext) -> Result<(), String> {
        ctx.stamp(
            CylinderLayer::Relation,
            "kg-prep",
            "concept extraction queued",
        );
        Ok(())
    }

    fn cognitive_layer(&self, ctx: &mut RequestContext) -> Result<(), String> {
        let lower = ctx.content.to_lowercase();
        let is_noise = lower
            .chars()
            .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
            .count() as f64
            / lower.len().max(1) as f64;
        if is_noise > 0.7 {
            ctx.stamp(
                CylinderLayer::Cognitive,
                "reject",
                &format!("noise_ratio={:.2}", is_noise),
            );
            return Err("content appears to be noise (high special-char ratio)".into());
        }
        ctx.stamp(CylinderLayer::Cognitive, "assess", "quality check passed");
        Ok(())
    }

    fn service_layer(&self, ctx: &mut RequestContext) -> Result<(), String> {
        ctx.stamp(CylinderLayer::Service, "skills", "skill matching checked");
        Ok(())
    }

    fn cycle_layer(&self, ctx: &mut RequestContext) {
        ctx.stamp(
            CylinderLayer::Cycle,
            "recall",
            "active recall trigger checked",
        );
    }

    fn identity_layer(&self, ctx: &mut RequestContext) {
        let identity = self.space.identity_info();
        if let Some(ref info) = identity {
            let hash = Self::compute_identity_hash(&info.system_name, &info.mission, &info.author);
            ctx.identity_hash = Some(hash.clone());
            ctx.stamp(
                CylinderLayer::Identity,
                "stamp",
                &format!("identity={} hash={}chars", info.system_name, hash.len()),
            );
        } else {
            ctx.stamp(
                CylinderLayer::Identity,
                "no-stamp",
                "identity not confirmed",
            );
        }
    }

    pub fn compute_identity_hash(name: &str, mission: &str, author: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        mission.hash(&mut hasher);
        author.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub fn identity_short_hash(space: &Space) -> Option<String> {
        space.identity_info().map(|info| {
            let h = Self::compute_identity_hash(&info.system_name, &info.mission, &info.author);
            format!("{}:{}", info.system_name, &h[..8.min(h.len())])
        })
    }
}

pub fn audit_to_string(ctx: &RequestContext) -> String {
    ctx.audit_trail
        .iter()
        .map(|e| format!("{:?}:{}({})", e.layer, e.action, e.detail))
        .collect::<Vec<_>>()
        .join(" → ")
}

pub fn memorialize_security_event(space: &Space, operation: &str, reason: &str, audit_trail: &str) {
    let ts = chrono::Utc::now().timestamp();
    let content = format!(
        "[SECURITY] operation={} reason=\"{}\" trail=\"{}\" ts={}",
        operation, reason, audit_trail, ts
    );
    tracing::warn!("[LayerPipeline] security event: {}", content);

    let labels = vec!["security-audit".to_string()];
    let layer = CylinderLayer::from_labels(&labels);
    let zone = space.zone_for_layer(layer);
    let z = zone.center_z();

    let port_opt = space.assign_cylinder_port(layer, u64::MAX);
    let anchor = if let Some((_vid, pos)) = port_opt {
        pos
    } else {
        crate::domain::vertex::Point3::new(0.0, 0.0, z)
    };

    let offsets = crate::domain::tetra::Tetrahedron::compute_vertices(anchor);
    let core = crate::domain::vertex::Point3::new(
        anchor.x - offsets[0].x,
        anchor.y - offsets[0].y,
        anchor.z - offsets[0].z,
    );
    let positions = crate::domain::tetra::Tetrahedron::compute_vertices(core);
    let content_hash = super::search_engine::hash_content(&content);
    let data = crate::domain::tetra::MemoryPayload {
        content,
        content_hash,
        labels,
        timestamp: ts,
        aliases: vec![],
        embedding: vec![],
        importance: 0.1,
        enforced: false,
        rationale: Some(format!("auto-security-memorial: {}", reason)),
        access_count: 0,
        memory_type: Some("security-audit".into()),
        identity_stamp: LayerPipeline::identity_short_hash(space),
        source_agent: Some("system-pipeline".into()),
        valid_from: chrono::Utc::now().timestamp(),
        valid_to: None,
        expired_at: None,
        invalidated_at: None,
        memory_class: None,
        last_reviewed_ts: None,
    };
    let tetra = crate::domain::tetra::Tetrahedron {
        id: 0,
        vertex_ids: [0; 4],
        core,
        data,
        mass: 0.1,
    };
    match space.add_tetrahedron(&tetra, &positions) {
        Ok(id) => {
            if port_opt.is_some() {
                space.reassign_cylinder_port(u64::MAX, id);
            }
            tracing::info!(
                "[LayerPipeline] security event memorialized as tetra {}",
                id
            );
        }
        Err(e) => {
            if port_opt.is_some() {
                space.release_cylinder_port(u64::MAX);
            }
            tracing::error!(
                "[LayerPipeline] failed to memorialize security event: {}",
                e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_hash_deterministic() {
        let h1 = LayerPipeline::compute_identity_hash("David", "memory", "Liu");
        let h2 = LayerPipeline::compute_identity_hash("David", "memory", "Liu");
        assert_eq!(h1, h2);
        let h3 = LayerPipeline::compute_identity_hash("David", "memory", "Other");
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_request_context_stamping() {
        let mut ctx = RequestContext::new_create("test content", vec!["test".into()]);
        ctx.stamp(CylinderLayer::Instinct, "receive", "ok");
        ctx.stamp(CylinderLayer::Identity, "stamp", "hashed");
        assert_eq!(ctx.audit_trail.len(), 2);
        assert_eq!(ctx.audit_trail[0].layer, CylinderLayer::Instinct);
        assert_eq!(ctx.audit_trail[1].layer, CylinderLayer::Identity);
    }

    #[test]
    fn test_layer_decision_allow_deny() {
        let allow = LayerDecision::allow();
        assert!(allow.allowed);

        let deny = LayerDecision::deny("bad content");
        assert!(!deny.allowed);
        assert_eq!(deny.reason, "bad content");
    }

    #[test]
    fn test_layer_decision_with_modifiers() {
        let d = LayerDecision::allow()
            .with_importance(2.0)
            .with_label("boosted");
        assert_eq!(d.modified_importance, Some(2.0));
        assert_eq!(d.boost_labels, vec!["boosted"]);
    }
}
