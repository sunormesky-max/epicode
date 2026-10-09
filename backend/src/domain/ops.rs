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
    }
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
}
