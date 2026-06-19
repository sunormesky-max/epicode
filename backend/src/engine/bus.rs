use tokio::sync::broadcast;

use crate::domain::tetra::TetraId;
use crate::domain::vertex::Point3;

/// Events flowing through the engine bus.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    TetrahedronCreated(TetraId),
    TetrahedronMoved(TetraId, Point3),
    TetrahedronRemoved(TetraId),
    PulseSent {
        origin: TetraId,
        ttl: u32,
    },
    EnergyLow {
        remaining: f64,
    },
    DecisionTick,
    ClusterSplit {
        from: u64,
        groups: Vec<Vec<TetraId>>,
    },
    ClusterMerged {
        a: u64,
        b: u64,
        result: u64,
    },
    AutoPulse {
        count: u32,
    },
    Shutdown,
}

pub type EventSender = broadcast::Sender<EngineEvent>;
pub type EventReceiver = broadcast::Receiver<EngineEvent>;

/// The event bus connects all engine centers.
/// One sender, six receivers.
pub struct EventBus {
    tx: EventSender,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> EventReceiver {
        self.tx.subscribe()
    }

    pub fn publish(&self, event: EngineEvent) {
        let _ = self.tx.send(event);
    }

    pub fn sender(&self) -> EventSender {
        self.tx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn all_receivers_get_event() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        let mut rx3 = bus.subscribe();
        let mut rx4 = bus.subscribe();
        let mut rx5 = bus.subscribe();
        let mut rx6 = bus.subscribe();

        bus.publish(EngineEvent::DecisionTick);

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
        assert!(rx3.try_recv().is_ok());
        assert!(rx4.try_recv().is_ok());
        assert!(rx5.try_recv().is_ok());
        assert!(rx6.try_recv().is_ok());
    }

    #[tokio::test]
    async fn multiple_events_in_order() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(EngineEvent::DecisionTick);
        bus.publish(EngineEvent::Shutdown);

        match rx.try_recv().unwrap() {
            EngineEvent::DecisionTick => {}
            _ => panic!("expected DecisionTick"),
        }
        match rx.try_recv().unwrap() {
            EngineEvent::Shutdown => {}
            _ => panic!("expected Shutdown"),
        }
    }
}
