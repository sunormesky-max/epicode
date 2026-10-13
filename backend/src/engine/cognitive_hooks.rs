use crate::domain::space::Space;

use super::cognitive::CognitiveEngine;
use super::gateway::GatewayCenter;
use super::storage::StorageManager;

pub struct CognitiveHooksCtx<'a> {
    pub space: &'a Space,
    pub storage: &'a StorageManager,
    pub gateway: &'a GatewayCenter,
    pub cognitive: &'a CognitiveEngine,
}

pub fn generate_aliases(
    ctx: &CognitiveHooksCtx,
    round: usize,
    tetras: &[crate::domain::tetra::TetraMeta],
) {
    let content_tetras: Vec<_> = tetras
        .iter()
        .filter(|t| !t.labels.iter().any(|l| l.starts_with("meta-")))
        .collect();
    let total = content_tetras.len();
    if total == 0 {
        return;
    }
    let offset = (round * 8) % total;
    let needs_aliases: Vec<(u64, String, Vec<String>)> = content_tetras
        .iter()
        .cycle()
        .skip(offset)
        .take(8)
        .map(|t| (t.id, t.content.clone(), t.labels.clone()))
        .collect();

    if needs_aliases.is_empty() {
        return;
    }

    match ctx.cognitive.generate_aliases(needs_aliases) {
        Ok(alias_results) => {
            for (id, new_aliases) in alias_results {
                let existing = ctx
                    .space
                    .get_tetrahedron(id)
                    .map(|t| t.data.aliases.clone())
                    .unwrap_or_default();
                let mut merged = existing.clone();
                for a in &new_aliases {
                    if !merged.contains(a) {
                        merged.push(a.clone());
                    }
                }
                if merged.len() > 10 {
                    merged.drain(0..merged.len() - 10);
                }
                // 字段级:只合并本次增删,不覆盖并发写入的别名;持久化锁内合并后的实际值
                let desired = merged;
                let merged = match ctx.space.patch_aliases(id, &existing, &desired) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("[Cognitive] alias update failed for {}: {}", id, e);
                        continue;
                    }
                };
                if let Err(e) = ctx.storage.update_aliases(id, &merged) {
                    tracing::warn!(
                        "[Cognitive] alias persist failed for {}: {}, rolling back",
                        id,
                        e
                    );
                    if let Err(re) = ctx.space.patch_aliases(id, &desired, &existing) {
                        tracing::error!("[Cognitive] ROLLBACK FAILED for {}: {}", id, re);
                    }
                } else {
                    tracing::info!(
                        "[Cognitive] aliases for #{}: {:?} (total {})",
                        id,
                        new_aliases,
                        merged.len()
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!("[Cognitive] alias generation failed: {}", e);
        }
    }
}

pub fn reclassify_memories(
    ctx: &CognitiveHooksCtx,
    round: usize,
    tetras: &[crate::domain::tetra::TetraMeta],
) {
    let content_tetras: Vec<_> = tetras
        .iter()
        .filter(|t| {
            !t.labels
                .iter()
                .any(|l| l.starts_with("meta-") || l.starts_with("bridge"))
        })
        .collect();
    let total = content_tetras.len();
    if total == 0 {
        return;
    }

    let mut sorted: Vec<_> = content_tetras.iter().collect();
    sorted.sort_by(|a, b| {
        a.mass
            .partial_cmp(&b.mass)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let offset = (round * 8) % total;
    let targets: Vec<_> = sorted.iter().cycle().skip(offset).take(8).collect();

    for t in &targets {
        let id = t.id;
        let old_labels = t.labels.clone();
        if old_labels.is_empty() {
            continue;
        }

        if let Ok(new_labels) = ctx.cognitive.classify_content(&t.content) {
            if new_labels != old_labels {
                let desired = new_labels;
                let new_labels = match ctx.space.patch_labels(id, &old_labels, &desired) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("[Reclassify] update labels failed for {}: {}", id, e);
                        continue;
                    }
                };
                if let Err(e) = ctx.storage.update_labels(id, &new_labels) {
                    tracing::warn!(
                        "[Reclassify] persist labels failed for {}: {}, rolling back",
                        id,
                        e
                    );
                    if let Err(re) = ctx.space.patch_labels(id, &desired, &old_labels) {
                        tracing::error!("[Reclassify] ROLLBACK FAILED for {}: {}", id, re);
                    }
                } else {
                    ctx.gateway.update_label_index(id, &old_labels, &new_labels);
                    tracing::info!("[Reclassify] #{}: {:?} -> {:?}", id, old_labels, new_labels);
                }
            }
        }
    }
}

pub fn extract_entities(
    ctx: &CognitiveHooksCtx,
    round: usize,
    tetras: &[crate::domain::tetra::TetraMeta],
) {
    let content_tetras: Vec<_> = tetras
        .iter()
        .filter(|t| {
            !t.labels.iter().any(|l| l.starts_with("meta-"))
                && !t.labels.iter().any(|l| l.starts_with("entity."))
                && !t.content.is_empty()
        })
        .collect();
    let total = content_tetras.len();
    if total == 0 {
        return;
    }

    let offset = (round * 5) % total;
    let batch: Vec<(u64, String)> = content_tetras
        .iter()
        .cycle()
        .skip(offset)
        .take(5)
        .map(|t| (t.id, t.content.clone()))
        .collect();

    if batch.is_empty() {
        return;
    }

    match ctx.cognitive.extract_entities(batch) {
        Ok(entity_results) => {
            for (id, entities) in entity_results {
                if let Some(t) = ctx.space.get_tetrahedron(id) {
                    let mut new_labels = t.data.labels.clone();
                    for e in &entities {
                        if !new_labels.contains(e) {
                            new_labels.push(e.clone());
                        }
                    }
                    let old_labels = t.data.labels.clone();
                    if let Err(err) = ctx.space.update_labels(id, new_labels.clone()) {
                        tracing::warn!("[Entity] update labels failed for {}: {}", id, err);
                    } else if let Err(err) = ctx.storage.update_labels(id, &new_labels) {
                        tracing::warn!("[Entity] persist failed for {}: {}, rolling back", id, err);
                        if let Err(re) = ctx.space.update_labels(id, old_labels) {
                            tracing::error!("[Entity] ROLLBACK FAILED for {}: {}", id, re);
                        }
                    } else {
                        ctx.gateway.update_label_index(id, &old_labels, &new_labels);
                        ctx.gateway.mark_dirty(id);
                        tracing::info!("[Entity] #{}: +{}", id, entities.join(","));
                    }
                }
            }
        }
        Err(e) => {
            tracing::debug!("[Entity] extract failed: {}", e);
        }
    }
}
