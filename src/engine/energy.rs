use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

pub const DEFAULT_MAX_ENERGY: f64 = 10000.0;
pub const CREATE_COST: f64 = 10.0;
pub const PULSE_COST: f64 = 2.0;
pub const RECHARGE_RATE: f64 = 12.0;

pub struct EnergyCenter {
    budget: AtomicI64,
    max_energy: i64,
    recharge_rate: i64,
}

impl EnergyCenter {
    pub fn new(max_energy: f64, recharge_rate: f64, _tx: super::bus::EventSender, _rx: broadcast::Receiver<super::bus::EngineEvent>) -> Self {
        let max_i = (max_energy * 1000.0) as i64;
        Self {
            budget: AtomicI64::new(max_i),
            max_energy: max_i,
            recharge_rate: (recharge_rate * 1000.0) as i64,
        }
    }

    pub fn budget(&self) -> f64 {
        self.budget.load(Ordering::SeqCst) as f64 / 1000.0
    }

    pub fn available(&self) -> f64 {
        self.budget()
    }

    /// Attempt to consume energy. Returns false if insufficient or amount is non-positive.
    /// Atomic — can be called from any thread.
    pub fn consume(&self, amount: f64) -> bool {
        if amount <= 0.0 {
            return false;
        }
        let amount_i = (amount * 1000.0) as i64;
        loop {
            let current = self.budget.load(Ordering::SeqCst);
            if current < amount_i {
                return false;
            }
            let new = current - amount_i;
            if self.budget.compare_exchange(current, new, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return true;
            }
        }
    }

    /// Replenish energy, capped at max.
    pub fn replenish(&self, amount: f64) {
        let amount_i = (amount * 1000.0) as i64;
        loop {
            let current = self.budget.load(Ordering::SeqCst);
            let new = (current + amount_i).min(self.max_energy);
            if self.budget.compare_exchange(current, new, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return;
            }
        }
    }

    pub async fn run_detached(center: Arc<EnergyCenter>, mut rx: broadcast::Receiver<super::bus::EngineEvent>) {
        loop {
            match rx.recv().await {
                Ok(super::bus::EngineEvent::Shutdown) => break,
                Ok(_) => {
                    center.replenish(center.recharge_rate as f64 / 1000.0);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[test]
    fn consume_sufficient() {
        let (tx, _) = broadcast::channel(1);
        let rx = tx.subscribe();
        let e = EnergyCenter::new(100.0, 0.0, tx, rx);
        assert!(e.consume(30.0));
        assert!((e.budget() - 70.0).abs() < 0.01);
    }

    #[test]
    fn consume_insufficient() {
        let (tx, _) = broadcast::channel(1);
        let rx = tx.subscribe();
        let e = EnergyCenter::new(10.0, 0.0, tx, rx);
        assert!(!e.consume(30.0));
        assert!((e.budget() - 10.0).abs() < 0.01);
    }

    #[test]
    fn replenish() {
        let (tx, _) = broadcast::channel(1);
        let rx = tx.subscribe();
        let e = EnergyCenter::new(100.0, 0.0, tx, rx);
        e.consume(50.0);
        e.replenish(20.0);
        assert!((e.budget() - 70.0).abs() < 0.01);
    }

    #[test]
    fn replenish_cap() {
        let (tx, _) = broadcast::channel(1);
        let rx = tx.subscribe();
        let e = EnergyCenter::new(100.0, 0.0, tx, rx);
        e.replenish(200.0);
        assert!((e.budget() - 100.0).abs() < 0.01);
    }
}
