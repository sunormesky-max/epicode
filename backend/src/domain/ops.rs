//! 字段级记忆操作(op-log 原语)。
//!
//! 后台流程(dream / governor / 调度器)不再"读快照 → 克隆整个 payload → 写回",
//! 而是产出一组类型化操作,由 [`Space::apply_ops`](super::space::Space::apply_ops)
//! 在**单次写锁**内原子应用:
//! - 只触碰声明的字段 → 不会覆盖并发的用户编辑(无 lost update);
//! - 每个操作都是幂等的(重复应用为 no-op)并报告是否真的改变了状态 →
//!   调用方只对"真的变了"的记忆计数,阶段天然幂等;
//! - 守卫条件(如 enforced 不可隔离)在锁内对**当前**状态求值,而不是对过期快照。

use super::tetra::MemoryPayload;

/// 生命周期状态,由标签/有效期推导(兼容现有存储格式,不新增持久化字段)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Active,
    Quarantined,
    Superseded,
}

impl Lifecycle {
    pub fn of(p: &MemoryPayload) -> Self {
        if p.labels.iter().any(|l| l == "superseded") || p.valid_to.is_some() {
            Lifecycle::Superseded
        } else if p.labels.iter().any(|l| l == "quarantine") {
            Lifecycle::Quarantined
        } else {
            Lifecycle::Active
        }
    }
}

/// 质量下限(与 Space::update_mass 的 clamp 一致)。
pub const MASS_MIN: f64 = 0.1;
pub const MASS_MAX: f64 = 100.0;

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryOp {
    AddLabel(String),
    /// importance = min(importance, cap)
    CapImportance(f64),
    SetImportance(f64),
    /// mass += delta(夹在 [MASS_MIN, MASS_MAX])
    AdjustMass(f64),
    /// mass = min(mass, cap)——用于降级,替代原先语义相反的 `update_mass(+0.05)`
    CapMass(f64),
    /// Active → Quarantined(enforced 或已非 Active 时为 no-op)
    Quarantine {
        importance_cap: f64,
        mass_cap: f64,
    },
    /// Active/Quarantined → Superseded(enforced 或已 Superseded 时为 no-op)
    Supersede {
        at: i64,
        importance_factor: f64,
        importance_floor: f64,
        mass_cap: f64,
    },
    /// TTL/僵尸墓碑:仅当仍为活记忆(无 valid_to、非 enforced)且(可选)importance
    /// 仍低于阈值时,设置 valid_to/invalidated_at 与墓碑 importance。
    Tombstone {
        at: i64,
        importance: f64,
        only_if_importance_below: Option<f64>,
    },
    /// 比较后设置:仅当 importance 仍等于评估时看到的值才写入(衰减不覆盖并发调整)。
    CasImportance {
        expect: f64,
        set: f64,
    },
    /// importance = min(importance + delta, max)
    AddImportance {
        delta: f64,
        max: f64,
    },
    /// access_count = 0
    ResetAccessCount,
}

/// 在可变引用上应用一个操作,返回是否改变了状态。
pub fn apply_op(data: &mut MemoryPayload, mass: &mut f64, op: &MemoryOp) -> bool {
    match op {
        MemoryOp::AddLabel(l) => {
            if data.labels.iter().any(|x| x == l) {
                false
            } else {
                data.labels.push(l.clone());
                true
            }
        }
        MemoryOp::CapImportance(c) => {
            if data.importance > *c {
                data.importance = *c;
                true
            } else {
                false
            }
        }
        MemoryOp::SetImportance(v) => {
            if (data.importance - v).abs() > f64::EPSILON {
                data.importance = *v;
                true
            } else {
                false
            }
        }
        MemoryOp::AdjustMass(d) => {
            let n = (*mass + d).clamp(MASS_MIN, MASS_MAX);
            let changed = (n - *mass).abs() > f64::EPSILON;
            *mass = n;
            changed
        }
        MemoryOp::CapMass(c) => {
            let c = c.max(MASS_MIN);
            if *mass > c {
                *mass = c;
                true
            } else {
                false
            }
        }
        MemoryOp::Quarantine {
            importance_cap,
            mass_cap,
        } => {
            if data.enforced || Lifecycle::of(data) != Lifecycle::Active {
                return false;
            }
            apply_op(data, mass, &MemoryOp::AddLabel("quarantine".into()));
            apply_op(data, mass, &MemoryOp::CapImportance(*importance_cap));
            apply_op(data, mass, &MemoryOp::CapMass(*mass_cap));
            true
        }
        MemoryOp::Supersede {
            at,
            importance_factor,
            importance_floor,
            mass_cap,
        } => {
            if data.enforced || Lifecycle::of(data) == Lifecycle::Superseded {
                return false;
            }
            apply_op(data, mass, &MemoryOp::AddLabel("superseded".into()));
            data.valid_to = Some(*at);
            if data.invalidated_at.is_none() {
                data.invalidated_at = Some(*at);
            }
            data.importance = (data.importance * importance_factor).max(*importance_floor);
            apply_op(data, mass, &MemoryOp::CapMass(*mass_cap));
            true
        }
        MemoryOp::Tombstone {
            at,
            importance,
            only_if_importance_below,
        } => {
            if data.enforced || data.valid_to.is_some() {
                return false;
            }
            if let Some(th) = only_if_importance_below {
                if data.importance >= *th {
                    return false;
                }
            }
            data.valid_to = Some(*at);
            if data.invalidated_at.is_none() {
                data.invalidated_at = Some(*at);
            }
            data.importance = *importance;
            true
        }
        MemoryOp::CasImportance { expect, set } => {
            if (data.importance - expect).abs() > f64::EPSILON
                || (data.importance - set).abs() <= f64::EPSILON
            {
                return false;
            }
            data.importance = *set;
            true
        }
        MemoryOp::AddImportance { delta, max } => {
            let n = (data.importance + delta).min(*max);
            if (n - data.importance).abs() <= f64::EPSILON {
                return false;
            }
            data.importance = n;
            true
        }
        MemoryOp::ResetAccessCount => {
            if data.access_count == 0 {
                return false;
            }
            data.access_count = 0;
            true
        }
    }
}

/// 三方字段合并:`desired` 是调用方基于快照 `base` 在锁外算好的新 payload。
/// 只把 `desired` 相对 `base` 真正改动的字段写到 `cur`(当前锁内状态)上;
/// labels / aliases 按集合差(增/删)合并,而不是整份覆盖。
/// 未改动的字段保留 `cur` 中的并发写入。返回 (是否改变, 可检索字段是否改变, 标签是否改变)。
pub fn patch(
    cur: &mut MemoryPayload,
    base: &MemoryPayload,
    desired: MemoryPayload,
) -> (bool, bool, bool) {
    let mut changed = false;
    let mut searchable = false;
    macro_rules! field {
        ($f:ident) => {
            if desired.$f != base.$f && cur.$f != desired.$f {
                cur.$f = desired.$f.clone();
                changed = true;
            }
        };
    }
    if desired.content != base.content && cur.content != desired.content {
        cur.content = desired.content.clone();
        changed = true;
        searchable = true;
    }
    field!(content_hash);
    field!(timestamp);
    field!(embedding);
    field!(importance);
    field!(enforced);
    field!(rationale);
    field!(access_count);
    field!(memory_type);
    field!(identity_stamp);
    field!(source_agent);
    field!(valid_from);
    field!(valid_to);
    field!(expired_at);
    field!(invalidated_at);
    field!(memory_class);
    field!(last_reviewed_ts);
    let labels_changed = merge_set(&mut cur.labels, &base.labels, &desired.labels);
    if merge_set(&mut cur.aliases, &base.aliases, &desired.aliases) {
        searchable = true;
        changed = true;
    }
    changed |= labels_changed;
    (changed, searchable, labels_changed)
}

pub fn merge_set(cur: &mut Vec<String>, base: &[String], desired: &[String]) -> bool {
    // 快路径:调用方没动这个集合(最常见),O(n) 比较后直接返回
    if base == desired {
        return false;
    }
    let before = cur.len();
    let mut changed = false;
    let removed: Vec<&String> = base.iter().filter(|x| !desired.contains(x)).collect();
    if !removed.is_empty() {
        cur.retain(|x| !removed.contains(&x));
        changed |= cur.len() != before;
    }
    for x in desired {
        if !base.contains(x) && !cur.contains(x) {
            cur.push(x.clone());
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_are_idempotent() {
        let mut d = MemoryPayload {
            importance: 1.0,
            ..Default::default()
        };
        let mut m = 1.0;
        let q = MemoryOp::Quarantine {
            importance_cap: 0.3,
            mass_cap: 0.1,
        };
        assert!(apply_op(&mut d, &mut m, &q));
        let snap = (d.clone(), m);
        assert!(!apply_op(&mut d, &mut m, &q));
        assert_eq!(
            (d.labels.clone(), d.importance, m),
            (snap.0.labels, snap.0.importance, snap.1)
        );
        let s = MemoryOp::Supersede {
            at: 5,
            importance_factor: 0.15,
            importance_floor: 0.3,
            mass_cap: 0.1,
        };
        assert!(apply_op(&mut d, &mut m, &s));
        assert!(!apply_op(&mut d, &mut m, &s));
        assert_eq!(Lifecycle::of(&d), Lifecycle::Superseded);
    }

    #[test]
    fn enforced_is_never_demoted() {
        let mut d = MemoryPayload {
            enforced: true,
            importance: 1.0,
            ..Default::default()
        };
        let mut m = 1.0;
        let q = MemoryOp::Quarantine {
            importance_cap: 0.3,
            mass_cap: 0.1,
        };
        assert!(!apply_op(&mut d, &mut m, &q));
        assert_eq!(m, 1.0);

        let before = (
            d.labels.clone(),
            d.valid_to,
            d.invalidated_at,
            d.importance,
            m,
        );
        let s = MemoryOp::Supersede {
            at: 5,
            importance_factor: 0.15,
            importance_floor: 0.3,
            mass_cap: 0.1,
        };
        assert!(!apply_op(&mut d, &mut m, &s));
        assert_eq!(
            (
                d.labels.clone(),
                d.valid_to,
                d.invalidated_at,
                d.importance,
                m
            ),
            before
        );
    }

    #[test]
    fn tombstone_and_cas_respect_current_state() {
        let mut p = MemoryPayload {
            importance: 0.1,
            ..Default::default()
        };
        let mut m = 1.0;
        let t = MemoryOp::Tombstone {
            at: 7,
            importance: 0.01,
            only_if_importance_below: Some(0.15),
        };
        // 并发提升过 importance → 不再是僵尸,不墓碑
        p.importance = 0.5;
        assert!(!apply_op(&mut p, &mut m, &t));
        p.importance = 0.1;
        assert!(apply_op(&mut p, &mut m, &t));
        assert_eq!(p.valid_to, Some(7));
        assert_eq!(p.invalidated_at, Some(7));
        assert!(!apply_op(&mut p, &mut m, &t), "idempotent");

        let mut q = MemoryPayload {
            importance: 0.9,
            ..Default::default()
        };
        let cas = MemoryOp::CasImportance {
            expect: 1.0,
            set: 0.4,
        };
        assert!(!apply_op(&mut q, &mut m, &cas), "stale expectation skipped");
        q.importance = 1.0;
        assert!(apply_op(&mut q, &mut m, &cas));
        assert_eq!(q.importance, 0.4);

        assert!(apply_op(
            &mut q,
            &mut m,
            &MemoryOp::AddImportance {
                delta: 5.0,
                max: 3.5
            }
        ));
        assert_eq!(q.importance, 3.5);
        q.access_count = 4;
        assert!(apply_op(&mut q, &mut m, &MemoryOp::ResetAccessCount));
        assert!(!apply_op(&mut q, &mut m, &MemoryOp::ResetAccessCount));
    }

    #[test]
    fn patch_keeps_concurrent_fields_and_merges_label_sets() {
        let base = MemoryPayload {
            labels: vec!["a".into(), "quarantine".into()],
            importance: 0.1,
            ..Default::default()
        };
        // 调用方:去掉 quarantine、加 b、改 importance
        let mut desired = base.clone();
        desired.labels.retain(|l| l != "quarantine");
        desired.labels.push("b".into());
        desired.importance = 0.5;
        // 并发:用户加了 u、改了 memory_class
        let mut cur = base.clone();
        cur.labels.push("u".into());
        cur.memory_class = Some("episodic".into());
        let (changed, _, labels) = patch(&mut cur, &base, desired);
        assert!(changed && labels);
        assert_eq!(cur.labels, vec!["a", "u", "b"]);
        assert_eq!(cur.importance, 0.5);
        assert_eq!(cur.memory_class.as_deref(), Some("episodic"));
        // 无改动的补丁是 no-op
        let snap = cur.clone();
        assert_eq!(patch(&mut cur, &snap, snap.clone()), (false, false, false));
    }
}
